use feed_rs::parser;
use crate::{Feed, Article};
use chrono::Utc;

pub fn parse_feed(url: &str, data: &[u8]) -> Result<(Feed, Vec<Article>), Box<dyn std::error::Error>> {
    let parsed = parser::parse(data)?;

    let feed = Feed {
        id: 0, // assigned by DB
        title: parsed.title.map(|t| t.content).unwrap_or_else(|| url.to_string()),
        url: url.to_string(),
        site_url: parsed.links.first().map(|l| l.href.clone()),
        description: parsed.description.map(|d| d.content),
        added_at: Utc::now(),
    };

    let articles: Vec<Article> = parsed.entries.into_iter().map(|entry| {
        // guid: prefer entry.id (RSS <guid> / Atom <id>), fallback to url, then title hash
        let guid = if !entry.id.is_empty() {
            entry.id.clone()
        } else if let Some(link) = entry.links.first() {
            link.href.clone()
        } else {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut h = DefaultHasher::new();
            entry.title.as_ref().map(|t| &t.content).unwrap_or(&String::new()).hash(&mut h);
            format!("hash:{}", h.finish())
        };
        Article {
            id: 0,
            feed_id: 0,
            guid,
            title: entry.title.map(|t| t.content).unwrap_or_default(),
            url: entry.links.first().map(|l| l.href.clone()),
            content: entry.content.and_then(|c| c.body),
            summary: entry.summary.map(|s| s.content),
            published_at: entry.published.or(entry.updated),
            is_read: false,
            is_starred: false,
            fetched_at: Utc::now(),
            full_content: None,
            tldr: None,
            tags: None,
        }
    }).collect();

    Ok((feed, articles))
}
