use serde::Serialize;

/// Fact-based article annotations — deterministic, zero AI, 100% accurate.
/// These are text features the machine can detect with certainty.
#[derive(Debug, Clone, Serialize)]
pub struct ArticleFeatures {
    /// "short" (<500 chars), "medium" (500-2000), "long" (>2000)
    pub length: String,
    /// Word/char count of the text content
    pub word_count: usize,
    /// Contains code blocks (``` or <code> or <pre>)
    pub has_code: bool,
    /// Contains numbered/ordered list (step-by-step patterns)
    pub has_steps: bool,
    /// Contains images (<img> tags)
    pub has_images: bool,
}

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

/// Detect factual features of an article. No AI, no guessing — pure text analysis.
pub fn detect_features(title: &str, content: &str) -> ArticleFeatures {
    let text = strip_html(content);
    let combined = format!("{} {}", title, text);
    let char_count = combined.chars().count();

    // Length classification by character count
    let length = if char_count < 500 {
        "short"
    } else if char_count < 2000 {
        "medium"
    } else {
        "long"
    }.to_string();

    // Word count (split on whitespace + CJK char counting)
    let word_count = count_words(&combined);

    // Code detection: markdown code fences or HTML code/pre tags
    let has_code = content.contains("```")
        || content.contains("<code")
        || content.contains("<pre")
        || content.contains("&lt;code")
        || content.contains("&lt;pre");

    // Step detection: ordered lists or step-by-step patterns
    let has_steps = detect_steps(content, &text);

    // Image detection
    let has_images = content.contains("<img")
        || content.contains("![");

    ArticleFeatures {
        length,
        word_count,
        has_code,
        has_steps,
        has_images,
    }
}

/// Convert features to comma-separated tags string for DB storage
pub fn features_to_tags(features: &ArticleFeatures) -> String {
    let mut tags = Vec::new();

    tags.push(features.length.as_str());

    if features.has_code {
        tags.push("has_code");
    }
    if features.has_steps {
        tags.push("has_steps");
    }
    if features.has_images {
        tags.push("has_images");
    }

    tags.join(",")
}

// ── Internal helpers ──

fn count_words(text: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if in_word {
                count += 1;
                in_word = false;
            }
        } else if is_cjk(ch) {
            // CJK characters are roughly one "word" each
            if in_word {
                count += 1;
                in_word = false;
            }
            count += 1;
        } else {
            in_word = true;
        }
    }
    if in_word {
        count += 1;
    }
    count
}

fn is_cjk(ch: char) -> bool {
    let cp = ch as u32;
    (0x4E00..=0x9FFF).contains(&cp)     // CJK Unified Ideographs
        || (0x3400..=0x4DBF).contains(&cp) // CJK Extension A
        || (0xF900..=0xFAFF).contains(&cp) // CJK Compatibility
        || (0x3000..=0x303F).contains(&cp) // CJK Punctuation
}

fn detect_steps(html: &str, text: &str) -> bool {
    // HTML ordered list
    if html.contains("<ol") {
        return true;
    }

    // Markdown/text patterns: "1. ", "Step 1", "第一步", "步骤"
    let lines: Vec<&str> = text.lines().collect();
    let mut numbered_count = 0;
    for line in &lines {
        let trimmed = line.trim();
        // "1. something", "2. something" etc
        if trimmed.len() > 2 {
            let first_char = trimmed.chars().next().unwrap_or(' ');
            if first_char.is_ascii_digit() && (trimmed.contains(". ") || trimmed.contains("）") || trimmed.contains(") ")) {
                numbered_count += 1;
            }
        }
    }

    // 3+ numbered items = likely has steps
    if numbered_count >= 3 {
        return true;
    }

    // Chinese step patterns
    text.contains("步骤") || text.contains("第一步") || text.contains("Step ")

}
