use ort::session::Session;
use ort::value::Tensor;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct Classification {
    pub tag: String,    // "新知" | "动态" | "深度" | "行动"
    pub score: f32,     // cosine similarity to category embedding
}

pub const CATEGORIES: &[(&str, &str)] = &[
    ("新知", "novel discovery, breakthrough, new concept, first-of-kind, emerging technology, scientific finding, paradigm shift"),
    ("动态", "product release, funding round, acquisition, update, partnership, launch, industry news, market movement"),
    ("深度", "in-depth analysis, methodology, investigation, long-form essay, deep dive, research paper, comprehensive review"),
    ("行动", "tutorial, how-to guide, step-by-step, practical advice, tool review, actionable tips, implementation guide"),
];

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

/// Download embedding model from HuggingFace (all-MiniLM-L6-v2)
pub fn download_model() -> Result<(), Box<dyn std::error::Error>> {
    let dir = model_dir();

    let embed_files = [
        ("onnx/model_int8.onnx", "embed_model_int8.onnx"),
        ("tokenizer.json", "embed_tokenizer.json"),
    ];

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    let embed_base = "https://huggingface.co/Xenova/all-MiniLM-L6-v2/resolve/main";
    for (remote, local) in &embed_files {
        let path = dir.join(local);
        if path.exists() {
            continue;
        }
        let url = format!("{}/{}", embed_base, remote);
        let bytes = client.get(&url).send()?.bytes()?;
        std::fs::write(&path, &bytes)?;
    }

    // Precompute category embeddings after model download
    precompute_category_embeddings()?;

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

// ── Sentence Embedding ──

/// Embed text into a 384-dimensional vector using all-MiniLM-L6-v2
pub fn embed_text(text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    if !is_embed_model_available() {
        return Err("Embedding model not found. Run `rss analyze --download-model` first.".into());
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

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    // vectors are already L2-normalized, so dot product = cosine similarity
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

// ── Category Embedding Classification ──

/// Precompute category embeddings and save to model directory
pub fn precompute_category_embeddings() -> Result<(), Box<dyn std::error::Error>> {
    let dir = model_dir();
    let path = dir.join("category_embeddings.bin");

    let mut embeddings: Vec<(String, Vec<f32>)> = Vec::new();
    for (name, desc) in CATEGORIES {
        let emb = embed_text(desc)?;
        embeddings.push((name.to_string(), emb));
    }

    // Simple binary format: for each category, write name_len(u32), name_bytes, then 384 f32s
    let mut data = Vec::new();
    for (name, emb) in &embeddings {
        let name_bytes = name.as_bytes();
        data.extend_from_slice(&(name_bytes.len() as u32).to_le_bytes());
        data.extend_from_slice(name_bytes);
        for &val in emb {
            data.extend_from_slice(&val.to_le_bytes());
        }
    }
    std::fs::write(&path, &data)?;
    Ok(())
}

/// Load precomputed category embeddings from file
pub fn load_category_embeddings() -> Result<Vec<(String, Vec<f32>)>, Box<dyn std::error::Error>> {
    let path = model_dir().join("category_embeddings.bin");
    if !path.exists() {
        // Auto-compute if missing
        precompute_category_embeddings()?;
    }

    let data = std::fs::read(&path)?;
    let mut offset = 0;
    let mut categories = Vec::new();

    while offset < data.len() {
        let name_len = u32::from_le_bytes(data[offset..offset+4].try_into()?) as usize;
        offset += 4;
        let name = String::from_utf8(data[offset..offset+name_len].to_vec())?;
        offset += name_len;

        let mut emb = Vec::with_capacity(384);
        for _ in 0..384 {
            let val = f32::from_le_bytes(data[offset..offset+4].try_into()?);
            emb.push(val);
            offset += 4;
        }
        categories.push((name, emb));
    }

    Ok(categories)
}

/// Classify an article into cognitive folders by embedding cosine similarity.
/// Returns scores for ALL 4 categories (harness layer decides thresholds).
pub fn classify_article(title: &str, summary: &str) -> Result<Vec<Classification>, Box<dyn std::error::Error>> {
    let categories = load_category_embeddings()?;

    // Combine title + summary for embedding
    let text = if summary.is_empty() {
        title.to_string()
    } else {
        format!("{} {}", title, strip_html(summary))
    };

    // Truncate to reasonable length
    let truncated = if text.len() > 2000 { &text[..2000] } else { &text };
    let article_emb = embed_text(truncated)?;

    let mut results: Vec<Classification> = categories.iter()
        .map(|(name, cat_emb)| Classification {
            tag: name.clone(),
            score: cosine_similarity(&article_emb, cat_emb),
        })
        .collect();

    // Sort by score descending
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

    Ok(results)
}
