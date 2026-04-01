use std::process::Command;

pub struct ExtractedContent {
    pub html: String,
    pub text_len: usize,
    pub source: ExtractSource,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExtractSource {
    Legible,
    AgentBrowser,
}

/// Two-tier full-text extraction: legible (fast) → agent-browser (fallback).
pub fn extract_full_text(url: &str) -> Result<ExtractedContent, Box<dyn std::error::Error>> {
    let html = fetch_html(url)?;

    // Fast path: legible (~ms)
    if let Ok(content) = try_legible(&html, url) {
        if content.text_len >= 100 {
            return Ok(content);
        }
    }

    // Fallback: agent-browser for JS-rendered pages (~seconds)
    try_agent_browser(url)
}

fn fetch_html(url: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) RSS-Reader/1.0")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let text = client.get(url).send()?.text()?;
    Ok(text)
}

fn try_legible(html: &str, url: &str) -> Result<ExtractedContent, Box<dyn std::error::Error>> {
    let result = legible::parse(html, Some(url), None)?;
    let text_len = result.text_content.len();
    let content_html = result.content;
    Ok(ExtractedContent {
        html: content_html,
        text_len,
        source: ExtractSource::Legible,
    })
}

fn try_agent_browser(url: &str) -> Result<ExtractedContent, Box<dyn std::error::Error>> {
    // Open URL
    let open = Command::new("agent-browser")
        .args(["open", url])
        .output()?;
    if !open.status.success() {
        return Err(format!(
            "agent-browser open failed: {}",
            String::from_utf8_lossy(&open.stderr)
        ).into());
    }

    // Wait for page to settle
    let _ = Command::new("agent-browser")
        .args(["wait", "3000"])
        .output();

    // Try to extract article content with common selectors
    let selectors = ["article", "main", ".post-content", ".article-content", ".entry-content", "body"];
    let mut best_html = String::new();

    for selector in &selectors {
        let output = Command::new("agent-browser")
            .args(["get", "html", selector])
            .output()?;
        if output.status.success() {
            let html = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if html.len() > best_html.len() {
                best_html = html;
            }
            // If we got a good result from article/main, stop
            if *selector != "body" && best_html.len() > 200 {
                break;
            }
        }
    }

    // Close browser
    let _ = Command::new("agent-browser").arg("close").output();

    if best_html.is_empty() {
        return Err("agent-browser: no content extracted".into());
    }

    let text_len = best_html.len();
    Ok(ExtractedContent {
        html: best_html,
        text_len,
        source: ExtractSource::AgentBrowser,
    })
}
