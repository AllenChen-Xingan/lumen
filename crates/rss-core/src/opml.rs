/// Simple OPML parser and generator using string matching.
/// No external XML dependency needed.

pub struct OpmlFeed {
    pub title: String,
    pub xml_url: String,
}

/// Parse OPML data, extracting <outline> elements with xmlUrl attributes.
pub fn parse_opml(data: &str) -> Result<Vec<OpmlFeed>, Box<dyn std::error::Error>> {
    let mut feeds = Vec::new();

    for line in data.lines() {
        let trimmed = line.trim();
        if !trimmed.contains("<outline") {
            continue;
        }
        let xml_url = match extract_attr(trimmed, "xmlUrl") {
            Some(u) => u,
            None => continue, // category outline, skip
        };
        let title = extract_attr(trimmed, "title")
            .or_else(|| extract_attr(trimmed, "text"))
            .unwrap_or_else(|| xml_url.clone());
        feeds.push(OpmlFeed { title, xml_url });
    }

    Ok(feeds)
}

/// Generate OPML XML from a list of (title, url) pairs.
pub fn generate_opml(feeds: &[(String, String)]) -> String {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<opml version=\"2.0\">\n");
    out.push_str("  <head><title>RSS Subscriptions</title></head>\n");
    out.push_str("  <body>\n");
    for (title, url) in feeds {
        out.push_str(&format!(
            "    <outline text=\"{}\" title=\"{}\" type=\"rss\" xmlUrl=\"{}\" />\n",
            escape_xml(title),
            escape_xml(title),
            escape_xml(url),
        ));
    }
    out.push_str("  </body>\n");
    out.push_str("</opml>\n");
    out
}

fn extract_attr(s: &str, attr: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr);
    let start = s.find(&pattern)? + pattern.len();
    let rest = &s[start..];
    let end = rest.find('"')?;
    Some(unescape_xml(&rest[..end]))
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn unescape_xml(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let feeds = vec![
            ("Test Feed".to_string(), "https://example.com/feed.xml".to_string()),
            ("Another & Feed".to_string(), "https://example.com/rss".to_string()),
        ];
        let opml = generate_opml(&feeds);
        let parsed = parse_opml(&opml).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "Test Feed");
        assert_eq!(parsed[0].xml_url, "https://example.com/feed.xml");
        assert_eq!(parsed[1].title, "Another & Feed");
    }
}
