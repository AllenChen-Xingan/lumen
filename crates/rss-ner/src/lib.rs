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
    /// Number of distinct code blocks
    pub code_block_count: usize,
    /// Contains numbered/ordered list (step-by-step patterns)
    pub has_steps: bool,
    /// Average character length of list items (0 if no list)
    pub avg_list_item_length: usize,
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

    // Code detection + count
    let code_block_count = count_code_blocks(content);
    let has_code = code_block_count > 0;

    // Step detection + average list item length
    let (has_steps, avg_list_item_length) = detect_steps_detailed(content, &text);

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
        has_code,
        code_block_count,
        has_steps,
        avg_list_item_length,
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

    if features.has_code {
        tags.push("has_code");
    }
    if features.has_steps {
        tags.push("has_steps");
    }
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

/// Count code blocks: markdown fences (```) and HTML <code>/<pre> tags
fn count_code_blocks(html: &str) -> usize {
    let fence_count = html.matches("```").count() / 2; // pairs of fences
    let code_tags = count_occurrences(html, "<code");
    let pre_tags = count_occurrences(html, "<pre");
    let escaped_code = count_occurrences(html, "&lt;code");
    let escaped_pre = count_occurrences(html, "&lt;pre");
    fence_count + code_tags + pre_tags + escaped_code + escaped_pre
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

/// Detect steps and compute average list item length.
/// Returns (has_steps, avg_list_item_length).
fn detect_steps_detailed(html: &str, text: &str) -> (bool, usize) {
    let mut item_lengths: Vec<usize> = Vec::new();

    // Collect <li> content lengths from HTML
    // Use lowercase copy for both search and extraction to avoid byte-index mismatch
    {
        let lower = html.to_lowercase();
        let mut search_from = 0;
        while let Some(start) = lower[search_from..].find("<li") {
            let abs_start = search_from + start;
            if let Some(gt) = lower[abs_start..].find('>') {
                let content_start = abs_start + gt + 1;
                let content_end = lower[content_start..].find("</li>")
                    .map(|p| content_start + p)
                    .unwrap_or(lower.len());
                let item_text = strip_html(&lower[content_start..content_end]);
                let len = item_text.trim().chars().count();
                if len > 0 {
                    item_lengths.push(len);
                }
                search_from = content_end;
            } else {
                break;
            }
        }
    }

    // Also check text-based numbered lines
    let mut numbered_items: Vec<usize> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.len() > 2 {
            let first_char = trimmed.chars().next().unwrap_or(' ');
            if first_char.is_ascii_digit()
                && (trimmed.contains(". ") || trimmed.contains("）") || trimmed.contains(") "))
            {
                numbered_items.push(trimmed.chars().count());
            }
        }
    }

    // Use whichever source found more items
    let items = if item_lengths.len() >= numbered_items.len() {
        &item_lengths
    } else {
        &numbered_items
    };

    let has_ordered_list = html.contains("<ol");
    let has_step_keywords = text.contains("步骤") || text.contains("第一步") || text.contains("Step ");
    let has_numbered = items.len() >= 3;

    let has_steps = has_ordered_list || has_step_keywords || has_numbered;

    let avg_len = if items.is_empty() {
        0
    } else {
        items.iter().sum::<usize>() / items.len()
    };

    (has_steps, avg_len)
}
