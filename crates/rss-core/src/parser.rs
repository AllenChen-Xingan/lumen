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
        Article {
            id: 0,
            feed_id: 0,
            title: entry.title.map(|t| t.content).unwrap_or_default(),
            url: entry.links.first().map(|l| l.href.clone()),
            content: entry.content.and_then(|c| c.body),
            summary: entry.summary.map(|s| s.content),
            published_at: entry.published.or(entry.updated),
            is_read: false,
            is_starred: false,
            fetched_at: Utc::now(),
        }
    }).collect();

    Ok((feed, articles))
}
