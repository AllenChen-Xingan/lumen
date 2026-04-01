use ort::session::Session;
use ort::value::Tensor;

use std::path::PathBuf;

/// Fixed cognitive folders for article classification
pub const COGNITIVE_FOLDERS: &[(&str, &str)] = &[
    ("新知", "novel discovery, breakthrough, new concept, first-of-kind, emerging technology, unprecedented finding"),
    ("动态", "product release, funding round, acquisition, update, partnership, launch, version upgrade"),
    ("深度", "in-depth analysis, methodology, investigation, long-form essay, deep dive, comprehensive review"),
    ("行动", "tutorial, how-to guide, review, deal, practical advice, step-by-step, tool recommendation"),
];

/// Cosine similarity threshold for classification
const CLASSIFY_THRESHOLD: f32 = 0.3;

// ── Model Management ──

fn model_dir() -> PathBuf {
    let dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rss-reader")
        .join("models");
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn is_embed_model_available() -> bool {
    let dir = model_dir();
    dir.join("embed_model_int8.onnx").exists() && dir.join("embed_tokenizer.json").exists()
}

/// Download embedding model (all-MiniLM-L6-v2) from HuggingFace
pub fn download_model() -> Result<(), Box<dyn std::error::Error>> {
    let dir = model_dir();

    let embed_files = [
        ("onnx/model_int8.onnx", "embed_model_int8.onnx"),
        ("tokenizer.json", "embed_tokenizer.json"),
    ];

    let embed_base = "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main";
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    for (remote, local) in &embed_files {
        let path = dir.join(local);
        if path.exists() {
            continue;
        }
        let url = format!("{}/{}", embed_base, remote);
        let bytes = client.get(&url).send()?.bytes()?;
        std::fs::write(&path, &bytes)?;
    }

    Ok(())
}

// ── Public API ──

/// Strip HTML tags for text processing
pub fn strip_html(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                result.push(' ');
            }
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Embed text into a 384-dimensional vector using all-MiniLM-L6-v2
pub fn embed_text(text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    if !is_embed_model_available() {
        return Err("Embedding model not found. Run `rss _classify --download-model` first.".into());
    }

    let dir = model_dir();
    let tokenizer = tokenizers::Tokenizer::from_file(dir.join("embed_tokenizer.json"))
        .map_err(|e| format!("Tokenizer error: {}", e))?;
    let mut session = Session::builder()?
        .commit_from_file(dir.join("embed_model_int8.onnx"))?;

    let encoding = tokenizer
        .encode(text, true)
        .map_err(|e| format!("Encoding error: {}", e))?;
    let max_len = 128.min(encoding.get_ids().len()); // short texts, 128 tokens enough

    let ids: Vec<i64> = encoding.get_ids()[..max_len]
        .iter()
        .map(|&x| x as i64)
        .collect();
    let mask: Vec<i64> = encoding.get_attention_mask()[..max_len]
        .iter()
        .map(|&x| x as i64)
        .collect();
    let token_type: Vec<i64> = vec![0i64; max_len];
    let len = ids.len();

    let input_ids = Tensor::from_array(([1usize, len], ids))?;
    let attention_mask_val = Tensor::from_array(([1usize, len], mask.clone()))?;
    let token_type_ids = Tensor::from_array(([1usize, len], token_type))?;

    let outputs = session.run(ort::inputs![
        "input_ids" => input_ids,
        "attention_mask" => attention_mask_val,
        "token_type_ids" => token_type_ids,
    ])?;

    // Output shape: [1, seq_len, 384] — do mean pooling over seq_len
    let (_shape, data) = outputs[0].try_extract_tensor::<f32>()?;
    let hidden_dim = 384;
    let seq_len = max_len;

    // Mean pooling with attention mask
    let mut pooled = vec![0.0f32; hidden_dim];
    let mut mask_sum = 0.0f32;
    for t in 0..seq_len {
        let m = mask[t] as f32;
        mask_sum += m;
        for d in 0..hidden_dim {
            pooled[d] += data[t * hidden_dim + d] * m;
        }
    }
    if mask_sum > 0.0 {
        for d in 0..hidden_dim {
            pooled[d] /= mask_sum;
        }
    }

    // L2 normalize
    let norm: f32 = pooled.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for d in 0..hidden_dim {
            pooled[d] /= norm;
        }
    }

    Ok(pooled)
}

/// Cosine similarity between two L2-normalized vectors (= dot product)
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

// ── Category Embedding Cache ──

fn category_cache_path() -> PathBuf {
    model_dir().join("category_embeddings.bin")
}

/// Precompute and cache the 4 category seed embeddings
pub fn precompute_category_embeddings() -> Result<(), Box<dyn std::error::Error>> {
    let mut data: Vec<u8> = Vec::new();

    for (name, seed_text) in COGNITIVE_FOLDERS {
        let emb = embed_text(seed_text)?;
        // Write: name_len(u32) + name_bytes + 384 floats
        let name_bytes = name.as_bytes();
        data.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(name_bytes);
        for val in &emb {
            data.extend_from_slice(&val.to_le_bytes());
        }
    }

    std::fs::write(category_cache_path(), &data)?;
    Ok(())
}

/// Load cached category embeddings. Returns Vec<(folder_name, embedding)>.
pub fn load_category_embeddings() -> Result<Vec<(String, Vec<f32>)>, Box<dyn std::error::Error>> {
    let path = category_cache_path();
    if !path.exists() {
        // Auto-compute if not cached
        precompute_category_embeddings()?;
    }

    let data = std::fs::read(&path)?;
    let mut cursor = 0;
    let mut result = Vec::new();

    while cursor < data.len() {
        // Read name
        if cursor + 4 > data.len() { break; }
        let name_len = u32::from_le_bytes(data[cursor..cursor+4].try_into()?) as usize;
        cursor += 4;
        if cursor + name_len > data.len() { break; }
        let name = String::from_utf8(data[cursor..cursor+name_len].to_vec())?;
        cursor += name_len;

        // Read 384 floats
        let float_bytes = 384 * 4;
        if cursor + float_bytes > data.len() { break; }
        let mut emb = Vec::with_capacity(384);
        for i in 0..384 {
            let start = cursor + i * 4;
            let val = f32::from_le_bytes(data[start..start+4].try_into()?);
            emb.push(val);
        }
        cursor += float_bytes;

        result.push((name, emb));
    }

    if result.len() != COGNITIVE_FOLDERS.len() {
        return Err(format!(
            "Expected {} category embeddings, found {}",
            COGNITIVE_FOLDERS.len(),
            result.len()
        ).into());
    }

    Ok(result)
}

/// Classify an article by title+summary into cognitive folder tags.
/// Returns tag names where cosine similarity > threshold (not mutually exclusive).
pub fn classify_article(title: &str, summary: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let text = format!("{} {}", title, strip_html(summary));
    let truncated = if text.len() > 2000 { &text[..2000] } else { &text };

    let article_emb = embed_text(truncated)?;
    let categories = load_category_embeddings()?;

    let mut tags = Vec::new();
    for (name, cat_emb) in &categories {
        let sim = cosine_similarity(&article_emb, cat_emb);
        if sim > CLASSIFY_THRESHOLD {
            tags.push(name.clone());
        }
    }

    Ok(tags)
}
