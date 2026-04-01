use ort::session::Session;
use ort::value::Tensor;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct Entity {
    pub name: String,
    pub entity_type: String, // one of DEFAULT_ENTITY_TYPES
    pub score: f32,
}

/// 可约束的复杂: max 4 types matching human working memory capacity
pub const DEFAULT_ENTITY_TYPES: &[&str] = &[
    "person",        // 人物
    "organization",  // 组织/公司
    "concept",       // 概念/技术/主题 (merged)
    "event",         // 事件 (person+org+time intersection)
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

pub fn is_model_available() -> bool {
    let dir = model_dir();
    dir.join("tokenizer.json").exists() && dir.join("model_int8.onnx").exists()
}

pub fn is_embed_model_available() -> bool {
    let dir = model_dir();
    dir.join("embed_model_int8.onnx").exists() && dir.join("embed_tokenizer.json").exists()
}

/// Download TinyBERT NER model from HuggingFace (~16MB total)
pub fn download_model() -> Result<(), Box<dyn std::error::Error>> {
    let dir = model_dir();
    let base = "https://huggingface.co/onnx-community/TinyBERT-finetuned-NER-ONNX/resolve/main";

    let files = [
        ("onnx/model_int8.onnx", "model_int8.onnx"),
        ("tokenizer.json", "tokenizer.json"),
    ];

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;

    for (remote, local) in &files {
        let path = dir.join(local);
        if path.exists() {
            continue;
        }
        let url = format!("{}/{}", base, remote);
        let bytes = client.get(&url).send()?.bytes()?;
        std::fs::write(&path, &bytes)?;
    }

    // Embedding model: all-MiniLM-L6-v2 for semantic topic clustering
    let embed_files = [
        ("onnx/model_int8.onnx", "embed_model_int8.onnx"),
        ("tokenizer.json", "embed_tokenizer.json"),
    ];

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

    Ok(())
}

// ── RAKE Keyword Extraction (pure Rust, no model) ──

/// Simple RAKE-like keyword extraction for concepts/topics
fn rake_extract(text: &str, max_keywords: usize) -> Vec<Entity> {
    // Common English stop words
    let stop_words: std::collections::HashSet<&str> = [
        "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by",
        "from", "is", "was", "are", "were", "be", "been", "being", "have", "has", "had", "do",
        "does", "did", "will", "would", "could", "should", "may", "might", "can", "shall", "not",
        "no", "nor", "so", "if", "then", "than", "that", "this", "these", "those", "it", "its",
        "i", "you", "he", "she", "we", "they", "me", "him", "her", "us", "them", "my", "your",
        "his", "our", "their", "what", "which", "who", "whom", "how", "when", "where", "why",
        "all", "each", "every", "both", "few", "more", "most", "other", "some", "such", "any",
        "only", "own", "same", "very", "just", "also", "about", "up", "out", "into", "over",
        "after", "before", "between", "under", "again", "further", "once", "here", "there",
        "because", "while", "during", "through", "above", "below", "new", "said", "like", "get",
        "got", "make", "made", "go", "going", "one", "two", "first", "last", "long", "great",
        "little", "just", "still", "back", "much", "many", "well", "now", "even", "also", "way",
        "use", "used", "using",
    ]
    .iter()
    .copied()
    .collect();

    // Split into words, filter stop words, count frequencies
    let words: Vec<&str> = text
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .filter(|w| w.len() > 2 && !stop_words.contains(&w.to_lowercase().as_str()))
        .collect();

    // Count word frequencies
    let mut freq: HashMap<String, usize> = HashMap::new();
    for w in &words {
        *freq.entry(w.to_string()).or_insert(0) += 1;
    }

    // Also extract bigrams (two-word phrases)
    let mut bigram_freq: HashMap<String, usize> = HashMap::new();
    for pair in words.windows(2) {
        let a = pair[0].to_lowercase();
        let b = pair[1].to_lowercase();
        if !stop_words.contains(a.as_str())
            && !stop_words.contains(b.as_str())
            && a.len() > 2
            && b.len() > 2
        {
            let bigram = format!("{} {}", pair[0], pair[1]);
            *bigram_freq.entry(bigram).or_insert(0) += 1;
        }
    }

    // Score: frequency * word_length_bonus
    let mut scored: Vec<(String, f32)> = freq
        .iter()
        .filter(|(_, &count)| count >= 2) // must appear at least twice
        .map(|(word, &count)| {
            let len_bonus = if word.len() > 6 { 1.5 } else { 1.0 };
            (word.clone(), count as f32 * len_bonus)
        })
        .collect();

    // Add bigrams with higher weight
    for (bigram, count) in &bigram_freq {
        if *count >= 2 {
            scored.push((bigram.clone(), *count as f32 * 2.0));
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    // Normalize scores to 0-1 range
    let max_score = scored.first().map(|s| s.1).unwrap_or(1.0);

    scored
        .into_iter()
        .take(max_keywords)
        .map(|(name, score)| Entity {
            name,
            entity_type: "concept".to_string(),
            score: score / max_score,
        })
        .collect()
}

// ── TinyBERT NER (ONNX) ──

/// NER labels from CoNLL-2003 (TinyBERT fine-tuned)
const NER_LABELS: &[&str] = &[
    "O", "B-PER", "I-PER", "B-ORG", "I-ORG", "B-LOC", "I-LOC", "B-MISC", "I-MISC",
];

fn label_to_type(label: &str) -> Option<&'static str> {
    match label {
        "B-PER" | "I-PER" => Some("person"),
        "B-ORG" | "I-ORG" => Some("organization"),
        "B-LOC" | "I-LOC" => None, // location demoted to attribute, not top-level
        "B-MISC" | "I-MISC" => Some("event"), // MISC often captures events
        _ => None,
    }
}

fn ner_extract(text: &str) -> Result<Vec<Entity>, Box<dyn std::error::Error>> {
    if !is_model_available() {
        return Ok(vec![]); // graceful fallback
    }

    let dir = model_dir();
    let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
        .map_err(|e| format!("Tokenizer error: {}", e))?;
    let mut session = Session::builder()?
        .commit_from_file(dir.join("model_int8.onnx"))?;

    // Truncate to 512 tokens (model max)
    let encoding = tokenizer
        .encode(text, true)
        .map_err(|e| format!("Encoding error: {}", e))?;
    let max_len = 512.min(encoding.get_ids().len());

    let ids: Vec<i64> = encoding.get_ids()[..max_len]
        .iter()
        .map(|&x| x as i64)
        .collect();
    let mask: Vec<i64> = encoding.get_attention_mask()[..max_len]
        .iter()
        .map(|&x| x as i64)
        .collect();
    let len = ids.len();

    let input_ids = Tensor::from_array(([1usize, len], ids))?;
    let attention_mask = Tensor::from_array(([1usize, len], mask))?;

    let outputs = session.run(ort::inputs![
        "input_ids" => input_ids,
        "attention_mask" => attention_mask,
    ])?;

    // Output shape: [1, seq_len, num_labels], returned as (&Shape, &[f32])
    let (shape, logits_data) = outputs[0].try_extract_tensor::<f32>()?;
    let num_labels = if shape.len() == 3 {
        shape[2] as usize
    } else {
        NER_LABELS.len()
    };

    // Decode entities
    let mut entities: Vec<Entity> = Vec::new();
    let mut current_entity: Option<(String, &str, f32)> = None; // (text, type, max_score)

    let offsets = encoding.get_offsets();

    for i in 0..max_len {
        let start_idx = i * num_labels;
        let token_logits = &logits_data[start_idx..start_idx + num_labels];

        // Softmax to get probabilities
        let max_val = token_logits
            .iter()
            .cloned()
            .fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = token_logits.iter().map(|&x| (x - max_val).exp()).sum();

        let (best_idx, best_prob) = token_logits
            .iter()
            .enumerate()
            .map(|(idx, &x)| (idx, (x - max_val).exp() / exp_sum))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .unwrap();

        let label = NER_LABELS.get(best_idx).copied().unwrap_or("O");

        if label.starts_with("B-") {
            // Save previous entity if any
            if let Some((name, etype, score)) = current_entity.take() {
                let trimmed = name.trim();
                if !trimmed.is_empty() && trimmed.len() > 1 {
                    entities.push(Entity {
                        name: trimmed.to_string(),
                        entity_type: etype.to_string(),
                        score,
                    });
                }
            }
            // Start new entity (skip unmapped labels like LOC)
            if let Some(etype) = label_to_type(label) {
                let (start, end) = offsets[i];
                let token_text = &text[start..end.min(text.len())];
                current_entity = Some((token_text.to_string(), etype, best_prob));
            }
        } else if label.starts_with("I-") {
            // Continue entity
            if let Some((ref mut name, _, ref mut score)) = current_entity {
                let (start, end) = offsets[i];
                let token_text = &text[start..end.min(text.len())];
                // Check if it's a continuation (##subword) or new word
                if token_text.starts_with("##") {
                    name.push_str(&token_text[2..]);
                } else {
                    name.push(' ');
                    name.push_str(token_text);
                }
                if best_prob > *score {
                    *score = best_prob;
                }
            }
        } else {
            // O label -- flush current entity
            if let Some((name, etype, score)) = current_entity.take() {
                let trimmed = name.trim();
                if !trimmed.is_empty() && trimmed.len() > 1 {
                    entities.push(Entity {
                        name: trimmed.to_string(),
                        entity_type: etype.to_string(),
                        score,
                    });
                }
            }
        }
    }
    // Flush last entity
    if let Some((name, etype, score)) = current_entity {
        let trimmed = name.trim();
        if !trimmed.is_empty() && trimmed.len() > 1 {
            entities.push(Entity {
                name: trimmed.to_string(),
                entity_type: etype.to_string(),
                score,
            });
        }
    }

    // Deduplicate: group by name (case-insensitive), keep highest score
    let mut deduped: HashMap<String, Entity> = HashMap::new();
    for e in entities {
        let key = e.name.to_lowercase();
        let entry = deduped.entry(key).or_insert(e.clone());
        if e.score > entry.score {
            *entry = e;
        }
    }

    Ok(deduped.into_values().collect())
}

// ── Public API ──

/// Strip HTML tags for NER processing
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

/// Extract all entities from text (NER + RAKE keywords)
/// This is the main entry point.
pub fn extract_entities(text: &str) -> Vec<Entity> {
    let clean = strip_html(text);
    // Limit input to ~5000 chars for performance
    let truncated = if clean.len() > 5000 {
        &clean[..5000]
    } else {
        &clean
    };

    let mut all = Vec::new();

    // NER entities (person, org, event)
    if let Ok(ner_entities) = ner_extract(truncated) {
        all.extend(ner_entities);
    }

    // RAKE keywords (concepts)
    let keywords = rake_extract(truncated, 10);
    all.extend(keywords);

    // Dedup across NER + RAKE: group by lowercase name, keep highest score
    let mut deduped: HashMap<String, Entity> = HashMap::new();
    for e in all {
        let key = e.name.to_lowercase();
        let entry = deduped.entry(key).or_insert(e.clone());
        if e.score > entry.score {
            *entry = e;
        }
    }

    deduped.into_values().collect()
}

// ── Sentence Embedding + K-means Clustering ──

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

/// Cluster article entity vectors into K topics using K-means
/// Input: Vec<(article_id, entity_text, embedding)>
/// Output: Vec<(topic_label, article_ids)> — up to K clusters
pub fn cluster_into_topics(
    items: &[(i64, String, Vec<f32>)],
    k: usize,
) -> Vec<(String, Vec<i64>)> {
    if items.is_empty() || k == 0 {
        return vec![];
    }
    let k = k.min(items.len());
    let dim = 384;

    // Initialize centroids: pick K items spread evenly
    let mut centroids: Vec<Vec<f32>> = Vec::new();
    let step = items.len() / k;
    for i in 0..k {
        centroids.push(items[(i * step).min(items.len() - 1)].2.clone());
    }

    // K-means iterations (max 20)
    let mut assignments = vec![0usize; items.len()];
    for _ in 0..20 {
        let mut changed = false;

        // Assign each item to nearest centroid
        for (i, (_, _, emb)) in items.iter().enumerate() {
            let best = centroids
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| {
                    cosine_similarity(emb, a)
                        .partial_cmp(&cosine_similarity(emb, b))
                        .unwrap()
                })
                .map(|(idx, _)| idx)
                .unwrap_or(0);
            if assignments[i] != best {
                assignments[i] = best;
                changed = true;
            }
        }

        if !changed {
            break;
        }

        // Update centroids
        for c in 0..k {
            let mut new_centroid = vec![0.0f32; dim];
            let mut count = 0;
            for (i, (_, _, emb)) in items.iter().enumerate() {
                if assignments[i] == c {
                    for d in 0..dim {
                        new_centroid[d] += emb[d];
                    }
                    count += 1;
                }
            }
            if count > 0 {
                for d in 0..dim {
                    new_centroid[d] /= count as f32;
                }
                centroids[c] = new_centroid;
            }
        }
    }

    // Build clusters: for each cluster, find the item closest to centroid as label
    let mut clusters: Vec<(String, Vec<i64>)> = Vec::new();
    for c in 0..k {
        let member_indices: Vec<usize> = assignments
            .iter()
            .enumerate()
            .filter(|(_, &a)| a == c)
            .map(|(i, _)| i)
            .collect();

        if member_indices.is_empty() {
            continue;
        }

        // Find the member closest to centroid — its text becomes the topic label
        let best_member = member_indices
            .iter()
            .max_by(|&&a, &&b| {
                cosine_similarity(&items[a].2, &centroids[c])
                    .partial_cmp(&cosine_similarity(&items[b].2, &centroids[c]))
                    .unwrap()
            })
            .unwrap();

        let label = items[*best_member].1.clone();
        let article_ids: Vec<i64> = member_indices.iter().map(|&i| items[i].0).collect();

        clusters.push((label, article_ids));
    }

    // Sort by cluster size descending
    clusters.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    clusters
}

/// Given article entity strings, cluster them into <=4 semantic topics
/// Returns: Vec<(topic_label, article_ids)>
pub fn suggest_topics(
    article_entities: &[(i64, String)],
) -> Result<Vec<(String, Vec<i64>)>, Box<dyn std::error::Error>> {
    if !is_embed_model_available() {
        return Err("Embedding model not found. Run `rss analyze --download-model` first.".into());
    }

    // Embed all article entity strings
    let mut items: Vec<(i64, String, Vec<f32>)> = Vec::new();
    for (article_id, entity_text) in article_entities {
        if let Ok(emb) = embed_text(entity_text) {
            items.push((*article_id, entity_text.clone(), emb));
        }
    }

    if items.is_empty() {
        return Ok(vec![]);
    }

    // Cluster into 4 topics
    Ok(cluster_into_topics(&items, 4))
}
