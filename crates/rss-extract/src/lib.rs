use std::process::Command;

/// Convert HTML to clean markdown suitable for agent consumption.
pub fn html_to_markdown(html: &str) -> String {
    html2md::parse_html(html)
}

const NORMAL_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const GOOGLEBOT_UA: &str = "Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)";

pub struct ExtractedContent {
    pub html: String,
    pub text_len: usize,
    pub source: ExtractSource,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExtractSource {
    Legible,
    LegibleGooglebot,
    AgentBrowser,
    ChromeCdp,
}

/// Four-tier full-text extraction: normal UA → Googlebot UA → agent-browser → Chrome CDP.
pub fn extract_full_text(url: &str) -> Result<ExtractedContent, Box<dyn std::error::Error>> {
    // Tier 1: Normal UA + legible (~ms)
    if let Ok(html) = fetch_with_ua(url, NORMAL_UA) {
        if let Ok(content) = try_legible(&html, url) {
            if content.text_len >= 100 {
                return Ok(content);
            }
        }
    }

    // Tier 2: Googlebot UA + legible (~ms, catches SSR pages)
    if let Ok(html) = fetch_with_ua(url, GOOGLEBOT_UA) {
        if let Ok(mut content) = try_legible(&html, url) {
            if content.text_len >= 100 {
                content.source = ExtractSource::LegibleGooglebot;
                return Ok(content);
            }
        }
    }

    // Tier 3: agent-browser for JS-only pages (~seconds)
    if let Ok(content) = try_agent_browser(url) {
        if content.text_len >= 100 {
            return Ok(content);
        }
    }

    // Tier 4: Chrome CDP — user's logged-in browser (paywall/anti-bot bypass)
    try_chrome_cdp(url)
}

fn fetch_with_ua(url: &str, ua: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent(ua)
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

/// Check if Chrome is running with CDP enabled on localhost:9222
fn is_cdp_available() -> bool {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
        .and_then(|c| c.get("http://127.0.0.1:9222/json/version").send())
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Find the cdp-extract.mjs script
fn find_cdp_script() -> Result<String, Box<dyn std::error::Error>> {
    // Check env var first
    if let Ok(path) = std::env::var("CDP_EXTRACT_SCRIPT") {
        if std::path::Path::new(&path).exists() {
            return Ok(path);
        }
    }

    // Try relative to executable
    if let Ok(exe) = std::env::current_exe() {
        // Development: exe is in target/debug/, scripts is in project root
        let mut dir = exe.parent().unwrap().to_path_buf();
        // Go up from target/debug to project root
        for _ in 0..3 {
            let candidate = dir.join("scripts").join("cdp-extract.mjs");
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
            }
            if let Some(parent) = dir.parent() {
                dir = parent.to_path_buf();
            } else {
                break;
            }
        }
    }

    // Try current directory
    let cwd = std::path::Path::new("scripts/cdp-extract.mjs");
    if cwd.exists() {
        return Ok(cwd.to_string_lossy().to_string());
    }

    Err("cdp-extract.mjs not found. Set CDP_EXTRACT_SCRIPT env var.".into())
}

fn try_chrome_cdp(url: &str) -> Result<ExtractedContent, Box<dyn std::error::Error>> {
    if !is_cdp_available() {
        return Err("Chrome CDP not available on port 9222".into());
    }

    let script = find_cdp_script()?;

    let output = Command::new("node")
        .arg(&script)
        .arg(url)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cdp-extract failed: {}", stderr).into());
    }

    let html = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if html.is_empty() {
        return Err("cdp-extract: no content extracted".into());
    }

    let text_len = html.len();
    Ok(ExtractedContent {
        html,
        text_len,
        source: ExtractSource::ChromeCdp,
    })
}
