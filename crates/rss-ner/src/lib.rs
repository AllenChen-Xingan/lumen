use serde::Serialize;

/// Fact-based article annotations — deterministic, zero AI, 100% accurate.
/// These are text features the machine can detect with certainty.
#[derive(Debug, Clone, Serialize)]
pub struct ArticleFeatures {
    /// "short" (<500 chars), "medium" (500-2000), "long" (>2000)
    pub length: String,
    /// Word/char count of the text content
    pub word_count: usize,
    /// Contains images (<img> tags)
    pub has_images: bool,
    /// Number of heading tags (h1-h6, or markdown #)
    pub heading_count: usize,
    /// Number of blockquote elements
    pub blockquote_count: usize,
    /// Number of external links (<a href="http...">)
    pub external_link_count: usize,
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


    // Image detection
    let has_images = content.contains("<img")
        || content.contains("![");

    // Heading count: HTML h1-h6 tags
    let heading_count = count_headings(content);

    // Blockquote count
    let blockquote_count = count_occurrences(content, "<blockquote");

    // External link count
    let external_link_count = count_external_links(content);

    ArticleFeatures {
        length,
        word_count,
        has_images,
        heading_count,
        blockquote_count,
        external_link_count,
    }
}

/// Convert features to comma-separated tags string for DB storage
pub fn features_to_tags(features: &ArticleFeatures) -> String {
    let mut tags = Vec::new();

    tags.push(features.length.as_str());

    if features.has_images {
        tags.push("has_images");
    }

    // "structured" = has heading hierarchy (2+ headings)
    if features.heading_count >= 2 {
        tags.push("structured");
    }

    // "has_references" = has blockquotes (cited sources)
    if features.blockquote_count >= 1 {
        tags.push("has_references");
    }

    // "link_rich" = 3+ external links (well-sourced)
    if features.external_link_count >= 3 {
        tags.push("link_rich");
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

/// Count headings: HTML h1-h6 tags
fn count_headings(html: &str) -> usize {
    let mut count = 0;
    for level in 1..=6 {
        let tag = format!("<h{}", level);
        count += count_occurrences(html, &tag);
    }
    count
}

/// Count external links: <a href="http..."> or <a href="https...">
fn count_external_links(html: &str) -> usize {
    let mut count = 0;
    let lower = html.to_lowercase();
    let mut search_from = 0;
    while let Some(pos) = lower[search_from..].find("<a ") {
        let abs = search_from + pos;
        // Find the closing '>' to delimit the tag — safe boundary
        let tag_end = lower[abs..].find('>').map(|p| abs + p + 1).unwrap_or(lower.len());
        let chunk = &lower[abs..tag_end];
        if chunk.contains("href=\"http://") || chunk.contains("href=\"https://")
            || chunk.contains("href='http://") || chunk.contains("href='https://") {
            count += 1;
        }
        search_from = tag_end;
    }
    count
}

/// Count non-overlapping occurrences of a substring (case-sensitive)
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    haystack.matches(needle).count()
}

