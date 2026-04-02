use clap::{Parser, Subcommand};
use rss_core::opml;
use rss_core::parser::parse_feed;
use rss_fetch::{fetch_feed_bytes, fetch_feed_bytes_with_client, fetch_feed_conditional_with_client, FetchOutcome};
use rayon::prelude::*;
use rss_store::Database;
use serde::Serialize;
use serde_json::{json, Value};
use std::process::ExitCode;
use rss_ner;
use chrono;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(name = "rss", about = "Agent-native RSS reader CLI — all output is JSON")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a feed by URL
    Add { url: String },
    /// List all feeds
    List,
    /// Remove a feed by ID
    Remove { id: i64 },
    /// Fetch new articles from all feeds (or one feed)
    Fetch {
        #[arg(long)]
        feed: Option<i64>,
    },
    /// List articles (default: newest 30)
    Articles {
        #[arg(long)]
        feed: Option<i64>,
        #[arg(long)]
        unread: bool,
        #[arg(long, default_value_t = 30)]
        count: usize,
        /// Compact output: id, title, source, published_at, tldr, tags, url (no content)
        #[arg(long)]
        compact: bool,
        /// Output one JSON object per line (for piping/grep)
        #[arg(long)]
        json_lines: bool,
    },
    /// Read an article by ID (marks it read)
    Read { id: i64 },
    /// Mark article as read
    MarkRead { id: i64 },
    /// Star / unstar an article
    Star { id: i64 },
    /// Import feeds from OPML file
    Import { path: String },
    /// Export feeds as OPML
    Export,
    /// Search articles using FTS5 full-text search (default: first 30 results)
    Search {
        query: String,
        #[arg(long, default_value_t = 30)]
        count: usize,
        /// Filter by feed ID
        #[arg(long)]
        feed: Option<i64>,
        /// Time filter: "24h", "7d", "30d"
        #[arg(long)]
        since: Option<String>,
        /// Compact output (no content)
        #[arg(long)]
        compact: bool,
        /// Sort order: "relevance" (BM25 ranking) or "date" (newest first)
        #[arg(long, default_value = "relevance")]
        sort: String,
    },
    /// Fetch full-text content for an article (cached)
    FetchFullText { id: i64 },
    /// Internal: annotate articles with fact-based features (length, has_code, has_steps, etc)
    #[command(name = "_annotate", hide = true)]
    Annotate {
        #[arg(long)]
        article: Option<i64>,
        /// Force re-annotate all articles (clear existing tags first)
        #[arg(long)]
        force: bool,
    },
    /// Internal: per-tag engagement diagnostics (read/star/deep-read rates)
    #[command(name = "_classify_stats", hide = true)]
    ClassifyStats,
    /// Internal: generate tldr for articles that don't have one
    #[command(name = "_summarize", hide = true)]
    Summarize,
    /// Internal: query extracted entities
    #[command(name = "_entities", hide = true)]
    Entities {
        #[arg(long)]
        entity_type: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        related: Option<String>,
        #[arg(long, default_value_t = 30)]
        count: usize,
    },
    /// Manage folders
    Folders {
        #[command(subcommand)]
        action: Option<FolderAction>,
    },
    /// Move a feed to a folder (or uncategorize it)
    MoveFeed {
        /// Feed ID to move
        feed_id: i64,
        /// Folder ID (omit to uncategorize)
        #[arg(long)]
        folder: Option<i64>,
    },
    /// Output unread articles + entity context for LLM pipe
    ReadForMe {
        #[arg(long, default_value = "24h")]
        since: String,
        #[arg(long, default_value_t = 100)]
        count: usize,
    },
    /// Show health status for all feeds
    FeedHealth {
        #[arg(long)]
        feed: Option<i64>,
        #[arg(long)]
        compact: bool,
    },
}

#[derive(Subcommand)]
enum FolderAction {
    Create {
        name: String,
        #[arg(long)]
        smart: Option<String>,
        #[arg(long)]
        feeds: Option<String>,
    },
    Remove { id: i64 },
    /// List articles in a folder: by ID (manual folders) or by name (cognitive folders)
    Articles {
        /// Smart view name (unread/long/tutorial/recent) or numeric ID for manual folders
        name: String,
        #[arg(long, default_value_t = 30)]
        count: usize,
    },
}

// ---------------------------------------------------------------------------
// JSON envelope helpers
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct SuccessEnvelope {
    ok: bool,
    command: String,
    result: Value,
    next_actions: Vec<Value>,
}

#[derive(Serialize)]
struct ErrorEnvelope {
    ok: bool,
    command: String,
    error: String,
    fix: String,
}

fn success(command: &str, result: Value, next_actions: Vec<Value>) -> ExitCode {
    let env = SuccessEnvelope {
        ok: true,
        command: command.to_string(),
        result,
        next_actions,
    };
    println!("{}", serde_json::to_string(&env).unwrap());
    ExitCode::from(0)
}

fn error(command: &str, err: &str, fix: &str) -> ExitCode {
    let env = ErrorEnvelope {
        ok: false,
        command: command.to_string(),
        error: err.to_string(),
        fix: fix.to_string(),
    };
    println!("{}", serde_json::to_string(&env).unwrap());
    ExitCode::from(1)
}

// ---------------------------------------------------------------------------
// HATEOAS next-action builder
// ---------------------------------------------------------------------------

fn action(command: &str, description: &str, params: Value) -> Value {
    json!({
        "command": command,
        "description": description,
        "params": params,
    })
}

// ---------------------------------------------------------------------------
// Database path resolution
// ---------------------------------------------------------------------------

fn db_path() -> String {
    if let Ok(path) = std::env::var("RSS_DB_PATH") {
        return path;
    }
    let dir = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let app_dir = dir.join("rss-reader");
    std::fs::create_dir_all(&app_dir).ok();
    app_dir.join("feeds.db").to_string_lossy().to_string()
}

// ---------------------------------------------------------------------------
// Principle 3 — self-describing root command
// ---------------------------------------------------------------------------

fn describe_commands() -> Value {
    json!({
        "name": "rss",
        "description": "Agent-native RSS reader CLI — all output is JSON",
        "commands": [
            {
                "name": "add",
                "description": "Add a feed by URL",
                "args": [
                    {"name": "url", "type": "string", "required": true, "description": "RSS/Atom feed URL"}
                ]
            },
            {
                "name": "list",
                "description": "List all subscribed feeds",
                "args": []
            },
            {
                "name": "remove",
                "description": "Remove a feed by ID",
                "args": [
                    {"name": "id", "type": "integer", "required": true, "description": "Feed ID"}
                ]
            },
            {
                "name": "fetch",
                "description": "Fetch new articles from all feeds (or one feed), then auto-classify",
                "args": [
                    {"name": "--feed", "type": "integer", "required": false, "description": "Limit to one feed ID"}
                ]
            },
            {
                "name": "articles",
                "description": "List articles (default: newest 30)",
                "args": [
                    {"name": "--feed", "type": "integer", "required": false, "description": "Filter by feed ID"},
                    {"name": "--unread", "type": "boolean", "required": false, "description": "Only unread articles"},
                    {"name": "--count", "type": "integer", "required": false, "description": "Max results (default 30)"},
                    {"name": "--compact", "type": "boolean", "required": false, "description": "Agent-friendly compact output (no content)"},
                    {"name": "--json-lines", "type": "boolean", "required": false, "description": "One JSON per line (for piping)"}
                ]
            },
            {
                "name": "read",
                "description": "Read an article by ID (marks it read)",
                "args": [
                    {"name": "id", "type": "integer", "required": true, "description": "Article ID"}
                ]
            },
            {
                "name": "mark-read",
                "description": "Mark an article as read",
                "args": [
                    {"name": "id", "type": "integer", "required": true, "description": "Article ID"}
                ]
            },
            {
                "name": "star",
                "description": "Star / unstar an article",
                "args": [
                    {"name": "id", "type": "integer", "required": true, "description": "Article ID"}
                ]
            },
            {
                "name": "import",
                "description": "Import feeds from an OPML file",
                "args": [
                    {"name": "path", "type": "string", "required": true, "description": "Path to OPML file"}
                ]
            },
            {
                "name": "export",
                "description": "Export all feeds as OPML",
                "args": []
            },
            {
                "name": "search",
                "description": "FTS5 full-text search across articles with BM25 relevance ranking. Supports boolean operators (AND, OR, NOT), phrase matching (\"exact phrase\"), prefix wildcards (term*), and NEAR queries.",
                "args": [
                    {"name": "query", "type": "string", "required": true, "description": "Search query (plain words, or FTS5 syntax: AND/OR/NOT, \"phrase\", prefix*)"},
                    {"name": "--count", "type": "integer", "required": false, "description": "Max results (default 30)"},
                    {"name": "--feed", "type": "integer", "required": false, "description": "Filter by feed ID"},
                    {"name": "--since", "type": "string", "required": false, "description": "Time filter: 24h, 7d, 30d"},
                    {"name": "--compact", "type": "boolean", "required": false, "description": "Agent-friendly compact output"},
                    {"name": "--sort", "type": "string", "required": false, "description": "Sort: 'relevance' (BM25, default) or 'date' (newest first)"}
                ]
            },
            {
                "name": "fetch-full-text",
                "description": "Fetch and cache full-text content for an article",
                "args": [
                    {"name": "id", "type": "integer", "required": true, "description": "Article ID"}
                ]
            },
            {
                "name": "folders",
                "description": "Manage folders: list, create, remove, articles <name>, reset",
                "args": []
            },
            {
                "name": "folders articles <name>",
                "description": "List articles in a smart view (unread/long/tutorial/recent) or manual folder by ID",
                "args": [
                    {"name": "name", "type": "string", "required": true, "description": "Folder name or ID"}
                ]
            },
            {
                "name": "move-feed",
                "description": "Move a feed to a folder (or uncategorize it)",
                "args": [
                    {"name": "feed_id", "type": "integer", "required": true, "description": "Feed ID to move"},
                    {"name": "--folder", "type": "integer", "required": false, "description": "Folder ID (omit to uncategorize)"}
                ]
            },
            {
                "name": "read-for-me",
                "description": "Output unread articles in 4 dimensions for LLM processing",
                "args": [
                    {"name": "--since", "type": "string", "required": false, "description": "Time window (default 24h, e.g. 7d)"},
                    {"name": "--count", "type": "integer", "required": false, "description": "Max articles (default 100)"}
                ]
            },
            {
                "name": "feed-health",
                "description": "Show health status for all feeds (response times, errors, fail counts)",
                "args": [
                    {"name": "--feed", "type": "integer", "required": false, "description": "Filter to a specific feed ID"},
                    {"name": "--compact", "type": "boolean", "required": false, "description": "Compact output (omit url, last_fetch_at, last_error)"}
                ]
            }
        ]
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn article_snippet(a: &rss_core::Article) -> Value {
    let content = a.content.as_deref().or(a.summary.as_deref()).unwrap_or("");
    let snippet = if content.len() > 300 {
        let end = content.char_indices().take_while(|&(i, _)| i < 300).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(content.len().min(300));
        &content[..end]
    } else { content };
    let clean = rss_ner::strip_html(snippet);
    json!({
        "id": a.id,
        "title": a.title,
        "feed_id": a.feed_id,
        "url": a.url,
        "published_at": a.published_at.map(|d| d.to_rfc3339()),
        "snippet": clean,
        "tldr": a.tldr,
    })
}

/// Generate a tldr from article text: extract first meaningful sentence, cap at 150 chars.
fn generate_tldr(article: &rss_core::Article) -> Option<String> {
    let raw = article.full_content.as_deref()
        .or(article.content.as_deref())
        .or(article.summary.as_deref())
        .unwrap_or(&article.title);

    // Strip HTML and take first 500 chars
    let clean = rss_ner::strip_html(raw);
    let text = clean.trim();
    if text.is_empty() {
        return None;
    }
    let truncated = if text.len() > 500 {
        let end = text.char_indices().take_while(|&(i, _)| i < 500).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(text.len().min(500));
        &text[..end]
    } else { text };

    // Find first sentence boundary
    let sentence = extract_first_sentence(truncated);
    let result = if sentence.len() > 150 {
        let end = sentence.char_indices().take_while(|&(i, _)| i < 147).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(sentence.len().min(147));
        format!("{}...", &sentence[..end])
    } else {
        sentence
    };

    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

/// Extract first meaningful sentence from text
fn extract_first_sentence(text: &str) -> String {
    // Skip leading whitespace/newlines
    let text = text.trim();

    // Try to find sentence-ending punctuation
    for (i, c) in text.char_indices() {
        if (c == '.' || c == '!' || c == '?' || c == '\u{3002}' /* Chinese period */) && i > 10 {
            // Check it's not an abbreviation (e.g., "U.S.")
            let end = i + c.len_utf8();
            let next_char = text[end..].chars().next();
            if next_char.map_or(true, |nc| nc.is_whitespace() || nc == '\n') {
                return text[..end].to_string();
            }
        }
        // Also break on newline if we have enough text
        if c == '\n' && i > 20 {
            return text[..i].trim().to_string();
        }
    }

    // No sentence boundary found, just take first ~150 chars at a char boundary
    if text.len() > 150 {
        let end = text.char_indices().take_while(|&(i, _)| i < 147).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(147.min(text.len()));
        format!("{}...", &text[..end])
    } else {
        text.to_string()
    }
}

/// Build compact article representation for agent scanning
fn compact_article(a: &rss_core::Article, db: &Database) -> Value {
    let feed_title = db.get_feed_title(a.feed_id).ok().flatten().unwrap_or_default();
    let tags: Vec<String> = db.get_article_entities(a.id).unwrap_or_default();
    let tags_str = tags.into_iter().take(5).collect::<Vec<_>>().join(",");
    json!({
        "id": a.id,
        "title": a.title,
        "source": feed_title,
        "published_at": a.published_at.map(|d| d.to_rfc3339()),
        "tldr": a.tldr,
        "tags": tags_str,
        "url": a.url,
    })
}

// ── Fact-based annotation: deterministic text feature detection ──

/// Annotate articles with factual features. No AI, no guessing.
/// Returns (annotated_count, tag_count).
fn annotate_articles(db: &Database, force: bool, single_article: Option<i64>) -> Result<(usize, usize), String> {
    if force && single_article.is_none() {
        db.clear_all_tags().map_err(|e| format!("{}", e))?;
    }

    // Single article mode: no chunking needed
    if let Some(id) = single_article {
        let articles = match db.list_articles(None, false) {
            Ok(all) => all.into_iter().filter(|a| a.id == id).collect::<Vec<_>>(),
            Err(e) => return Err(format!("{}", e)),
        };
        let mut total_tags = 0;
        for a in &articles {
            let content = a.full_content.as_deref()
                .or(a.content.as_deref())
                .or(a.summary.as_deref())
                .unwrap_or("");
            let features = rss_ner::detect_features(&a.title, content);
            let tags_str = rss_ner::features_to_tags(&features);
            total_tags += tags_str.matches(',').count() + 1;
            db.set_article_features(
                a.id, &tags_str, features.word_count, features.heading_count,
                features.code_block_count, features.external_link_count, features.blockquote_count,
            ).ok();
        }
        return Ok((articles.len(), total_tags));
    }

    // Chunked processing: fetch 500 articles at a time
    const CHUNK_SIZE: usize = 500;
    let mut total = 0;
    let mut total_tags = 0;
    let mut offset = 0;

    loop {
        let articles = if force {
            db.list_articles_chunk(offset, CHUNK_SIZE).map_err(|e| format!("{}", e))?
        } else {
            db.list_untagged_articles_chunk(offset, CHUNK_SIZE).map_err(|e| format!("{}", e))?
        };

        if articles.is_empty() {
            break;
        }

        let chunk_len = articles.len();
        let mut updates: Vec<(i64, String, usize, usize, usize, usize, usize)> = Vec::with_capacity(chunk_len);

        for a in &articles {
            let content = a.full_content.as_deref()
                .or(a.content.as_deref())
                .or(a.summary.as_deref())
                .unwrap_or("");

            let features = rss_ner::detect_features(&a.title, content);
            let tags_str = rss_ner::features_to_tags(&features);
            total_tags += tags_str.matches(',').count() + 1;
            updates.push((
                a.id, tags_str, features.word_count, features.heading_count,
                features.code_block_count, features.external_link_count, features.blockquote_count,
            ));
        }

        db.batch_set_article_features(&updates).map_err(|e| format!("{}", e))?;
        total += chunk_len;

        // For untagged mode, always use offset 0 since processed articles leave the result set.
        // For force mode, advance offset since all articles are returned regardless.
        if force {
            offset += chunk_len;
        }

        if chunk_len < CHUNK_SIZE {
            break;
        }
    }

    Ok((total, total_tags))
}

// ---------------------------------------------------------------------------
// Smart view names (fact-based, deterministic)
// ---------------------------------------------------------------------------

const SMART_VIEW_NAMES: &[(&str, &str)] = &[
    ("unread", "Unread articles"),
    ("long", "Long-form articles"),
    ("tutorial", "Articles with code or steps"),
    ("recent", "Today's articles"),
];

fn is_smart_view(name: &str) -> bool {
    SMART_VIEW_NAMES.iter().any(|(n, _)| *n == name)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Principle 3: no subcommand → self-describe
    let command = match cli.command {
        None => {
            return success("rss", describe_commands(), vec![
                action("rss list", "List all feeds", json!({})),
                action("rss add <url>", "Add a new feed", json!({"url": {"type": "string"}})),
                action("rss articles", "List recent articles", json!({})),
            ]);
        }
        Some(c) => c,
    };

    let db = match Database::open(&db_path()) {
        Ok(db) => db,
        Err(e) => {
            return error("rss", &format!("Failed to open database: {}", e), "Check RSS_DB_PATH or filesystem permissions");
        }
    };

    match command {
        // ---------------------------------------------------------------
        // ADD
        // ---------------------------------------------------------------
        Commands::Add { url } => {
            let bytes = match fetch_feed_bytes(&url) {
                Ok(b) => b,
                Err(e) => return error("add", &format!("Fetch failed: {}", e), "Check the URL is a valid RSS/Atom feed"),
            };
            let (feed, articles) = match parse_feed(&url, &bytes) {
                Ok(pair) => pair,
                Err(e) => return error("add", &format!("Parse failed: {}", e), "Ensure the URL points to a valid RSS/Atom feed"),
            };
            match db.add_feed(&feed) {
                Ok(feed_id) => {
                    let count = db.add_articles(feed_id, &articles).unwrap_or(0);
                    success("add", json!({
                        "feed_id": feed_id,
                        "title": feed.title,
                        "url": feed.url,
                        "articles_added": count,
                    }), vec![
                        action("rss articles --feed {feed_id}", "List articles for this feed", json!({"feed_id": {"value": feed_id}})),
                        action("rss fetch", "Fetch new articles from all feeds", json!({})),
                        action("rss list", "List all feeds", json!({})),
                    ])
                }
                Err(e) => error("add", &format!("Database error: {}", e), "Feed may already exist; use `rss list` to check"),
            }
        }

        // ---------------------------------------------------------------
        // LIST
        // ---------------------------------------------------------------
        Commands::List => {
            match db.list_feeds() {
                Ok(feeds) => {
                    let feed_values: Vec<Value> = feeds.iter().map(|f| {
                        let mut val = serde_json::to_value(f).unwrap();
                        if let Ok((total, unread, last_fetched)) = db.get_feed_stats(f.id) {
                            val["article_count"] = json!(total);
                            val["unread_count"] = json!(unread);
                            val["last_fetched"] = json!(last_fetched);
                        }
                        if let Ok(fid) = db.get_feed_folder_id(f.id) {
                            val["folder_id"] = json!(fid);
                        }
                        val
                    }).collect();
                    let mut next = vec![
                        action("rss add <url>", "Add a new feed", json!({"url": {"type": "string"}})),
                        action("rss articles", "List all articles", json!({})),
                    ];
                    for f in &feeds {
                        next.push(action(
                            &format!("rss articles --feed {}", f.id),
                            &format!("Articles from {}", f.title),
                            json!({"feed_id": {"value": f.id}}),
                        ));
                    }
                    success("list", json!({
                        "feeds": feed_values,
                        "count": feeds.len(),
                    }), next)
                }
                Err(e) => error("list", &format!("{}", e), "Check database integrity"),
            }
        }

        // ---------------------------------------------------------------
        // REMOVE
        // ---------------------------------------------------------------
        Commands::Remove { id } => {
            match db.remove_feed(id) {
                Ok(true) => success("remove", json!({"removed_feed_id": id}), vec![
                    action("rss list", "List remaining feeds", json!({})),
                ]),
                Ok(false) => error("remove", &format!("Feed {} not found", id), "Use `rss list` to see valid feed IDs"),
                Err(e) => error("remove", &format!("{}", e), "Use `rss list` to see valid feed IDs"),
            }
        }

        // ---------------------------------------------------------------
        // FETCH (+ auto-classify new articles)
        // ---------------------------------------------------------------
        Commands::Fetch { feed } => {
            let feeds = match db.list_feeds() {
                Ok(f) => f,
                Err(e) => return error("fetch", &format!("{}", e), "Check database"),
            };
            let targets: Vec<_> = match feed {
                Some(fid) => feeds.into_iter().filter(|f| f.id == fid).collect(),
                None => feeds,
            };
            if targets.is_empty() {
                return error("fetch", "No feeds to fetch", "Use `rss add <url>` to add feeds first");
            }

            // Build shared HTTP client for connection pooling
            rayon::ThreadPoolBuilder::new().num_threads(64).build_global().ok();
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build().map_err(|e| format!("{}", e)).unwrap();

            // Phase 1: parallel HTTP fetches (no DB access)
            let fetch_results: Vec<_> = targets.par_iter().map(|f| {
                let t0 = std::time::Instant::now();
                let result = fetch_feed_conditional_with_client(
                    &client, &f.url, f.etag.as_deref(), f.last_modified_header.as_deref()
                ).map_err(|e| format!("{}", e));
                (f, result, t0.elapsed().as_millis() as i64)
            }).collect();

            // Phase 2: sequential DB writes
            let mut results = Vec::new();
            let mut feeds_skipped_304: usize = 0;
            for (f, result, elapsed_ms) in fetch_results {
                match result {
                    Ok(FetchOutcome::NotModified) => {
                        db.record_fetch_success(f.id, elapsed_ms).ok();
                        feeds_skipped_304 += 1;
                        results.push(json!({"feed_id": f.id, "title": f.title, "new_articles": 0, "not_modified": true, "response_ms": elapsed_ms}));
                    }
                    Ok(FetchOutcome::Modified { bytes, etag, last_modified }) => {
                        db.update_feed_cache_headers(f.id, etag.as_deref(), last_modified.as_deref()).ok();
                        match parse_feed(&f.url, &bytes) {
                            Ok((_, articles)) => {
                                let count = db.add_articles(f.id, &articles).unwrap_or(0);
                                db.record_fetch_success(f.id, elapsed_ms).ok();
                                results.push(json!({"feed_id": f.id, "title": f.title, "new_articles": count, "response_ms": elapsed_ms}));
                            }
                            Err(e) => {
                                let err_str = format!("{}", e);
                                db.record_fetch_failure(f.id, &err_str).ok();
                                results.push(json!({"feed_id": f.id, "title": f.title, "error": err_str}));
                            }
                        }
                    }
                    Err(e) => {
                        let err_str = format!("{}", e);
                        db.record_fetch_failure(f.id, &err_str).ok();
                        results.push(json!({"feed_id": f.id, "title": f.title, "error": err_str}));
                    }
                }
            }

            // Auto-annotate new (untagged) articles with fact-based features
            let annotate_result = annotate_articles(&db, false, None).ok();

            // Auto-generate tldr for articles that don't have one yet
            let mut tldrs_generated = 0;
            if let Ok(no_tldr) = db.list_articles_without_tldr() {
                for a in &no_tldr {
                    if let Some(tldr) = generate_tldr(a) {
                        db.set_article_tldr(a.id, &tldr).ok();
                        tldrs_generated += 1;
                    }
                }
            }

            success("fetch", json!({
                "feeds_fetched": results.len(),
                "feeds_skipped_304": feeds_skipped_304,
                "results": results,
                "annotated": annotate_result.map(|(c, t)| json!({"articles": c, "tags": t})),
                "tldrs_generated": tldrs_generated,
            }), vec![
                action("rss articles --unread", "List unread articles", json!({})),
                action("rss list", "List all feeds", json!({})),
            ])
        }

        // ---------------------------------------------------------------
        // ARTICLES
        // ---------------------------------------------------------------
        Commands::Articles { feed, unread, count, compact, json_lines } => {
            match db.list_articles(feed, unread) {
                Ok(articles) => {
                    let total = articles.len();
                    let truncated = total > count;

                    if json_lines {
                        // Output one JSON object per line, no envelope
                        for a in articles.iter().take(count) {
                            let val = if compact {
                                compact_article(a, &db)
                            } else {
                                serde_json::to_value(a).unwrap()
                            };
                            println!("{}", serde_json::to_string(&val).unwrap());
                        }
                        return ExitCode::from(0);
                    }

                    let page: Vec<Value> = if compact {
                        articles.iter().take(count)
                            .map(|a| compact_article(a, &db))
                            .collect()
                    } else {
                        articles.iter().take(count)
                            .map(|a| serde_json::to_value(a).unwrap())
                            .collect()
                    };
                    let mut next: Vec<Value> = Vec::new();
                    for a in articles.iter().take(count) {
                        next.push(action(
                            &format!("rss read {}", a.id),
                            &format!("Read: {}", a.title),
                            json!({"id": {"value": a.id}}),
                        ));
                    }
                    if truncated {
                        next.push(action(
                            &format!("rss articles{}{} --count {}", feed.map_or(String::new(), |f| format!(" --feed {}", f)), if unread { " --unread" } else { "" }, count + 30),
                            "Load more articles",
                            json!({"count": {"value": count + 30}}),
                        ));
                    }
                    next.push(action("rss fetch", "Fetch new articles", json!({})));
                    success("articles", json!({
                        "articles": page,
                        "count": page.len(),
                        "total": total,
                        "truncated": truncated,
                    }), next)
                }
                Err(e) => error("articles", &format!("{}", e), "Check feed ID with `rss list`"),
            }
        }

        // ---------------------------------------------------------------
        // READ
        // ---------------------------------------------------------------
        Commands::Read { id } => {
            match db.get_article(id) {
                Ok(Some(article)) => {
                    db.mark_read(id).ok();
                    let feed_title = db.get_feed_title(article.feed_id).ok().flatten().unwrap_or_default();
                    let tags: Vec<String> = db.get_article_entities(id).unwrap_or_default();
                    let has_full_text = db.get_full_content(id).ok().flatten().is_some();

                    // Content preview: first 500 chars of clean text
                    let raw_content = article.full_content.as_deref()
                        .or(article.content.as_deref())
                        .or(article.summary.as_deref())
                        .unwrap_or("");
                    let clean = rss_ner::strip_html(raw_content);
                    let content_preview = if clean.len() > 500 {
                        let end = clean.char_indices().take_while(|&(i, _)| i < 500).last().map(|(i, c)| i + c.len_utf8()).unwrap_or(clean.len().min(500));
                        &clean[..end]
                    } else { &clean };
                    let word_count = clean.split_whitespace().count();

                    success("read", json!({
                        "id": article.id,
                        "title": article.title,
                        "feed_title": feed_title,
                        "url": article.url,
                        "published_at": article.published_at.map(|d| d.to_rfc3339()),
                        "tldr": article.tldr,
                        "tags": tags.into_iter().take(10).collect::<Vec<_>>().join(","),
                        "content_preview": content_preview,
                        "has_full_text": has_full_text,
                        "word_count": word_count,
                        "is_read": true,
                        "is_starred": article.is_starred,
                    }), vec![
                        action(&format!("rss fetch-full-text {}", id), "Get full-text content", json!({"id": {"value": id}})),
                        action(&format!("rss star {}", id), "Star this article", json!({"id": {"value": id}})),
                        action(&format!("rss articles --feed {}", article.feed_id), "More from this feed", json!({"feed_id": {"value": article.feed_id}})),
                    ])
                }
                Ok(None) => error("read", &format!("Article {} not found", id), "Use `rss articles` to find valid article IDs"),
                Err(e) => error("read", &format!("{}", e), "Check database"),
            }
        }

        // ---------------------------------------------------------------
        // MARK-READ
        // ---------------------------------------------------------------
        Commands::MarkRead { id } => {
            match db.mark_read(id) {
                Ok(true) => success("mark-read", json!({"article_id": id, "is_read": true}), vec![
                    action("rss articles --unread", "List remaining unread", json!({})),
                ]),
                Ok(false) => error("mark-read", &format!("Article {} not found", id), "Use `rss articles` to find valid IDs"),
                Err(e) => error("mark-read", &format!("{}", e), "Check article ID"),
            }
        }

        // ---------------------------------------------------------------
        // STAR
        // ---------------------------------------------------------------
        Commands::Star { id } => {
            match db.toggle_star(id) {
                Ok(true) => success("star", json!({"article_id": id, "toggled": true}), vec![
                    action(&format!("rss read {}", id), "Read the article", json!({"id": {"value": id}})),
                    action("rss articles", "List articles", json!({})),
                ]),
                Ok(false) => error("star", &format!("Article {} not found", id), "Use `rss articles` to find valid IDs"),
                Err(e) => error("star", &format!("{}", e), "Check article ID"),
            }
        }

        // ---------------------------------------------------------------
        // IMPORT
        // ---------------------------------------------------------------
        Commands::Import { path } => {
            let data = match std::fs::read_to_string(&path) {
                Ok(d) => d,
                Err(e) => return error("import", &format!("Cannot read file: {}", e), "Check the file path exists and is readable"),
            };
            let opml_feeds = match opml::parse_opml(&data) {
                Ok(f) => f,
                Err(e) => return error("import", &format!("OPML parse error: {}", e), "Ensure the file is valid OPML"),
            };

            // Build shared HTTP client for connection pooling
            rayon::ThreadPoolBuilder::new().num_threads(64).build_global().ok();
            let client = reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build().map_err(|e| format!("{}", e)).unwrap();

            // Phase 1: parallel HTTP fetches + parsing (no DB access)
            let fetch_results: Vec<_> = opml_feeds.par_iter().map(|of| {
                let result = fetch_feed_bytes_with_client(&client, &of.xml_url)
                    .map_err(|e| format!("Fetch: {}", e))
                    .and_then(|bytes| {
                        parse_feed(&of.xml_url, &bytes)
                            .map_err(|e| format!("Parse: {}", e))
                    });
                (of, result)
            }).collect();

            // Phase 2: sequential DB writes
            let mut imported = Vec::new();
            let mut errors = Vec::new();
            for (of, result) in fetch_results {
                match result {
                    Ok((feed, articles)) => match db.add_feed(&feed) {
                        Ok(feed_id) => {
                            let count = db.add_articles(feed_id, &articles).unwrap_or(0);
                            imported.push(json!({"feed_id": feed_id, "title": feed.title, "articles_added": count}));
                        }
                        Err(e) => errors.push(json!({"title": of.title, "error": format!("{}", e)})),
                    },
                    Err(e) => errors.push(json!({"title": of.title, "error": format!("{}", e)})),
                }
            }
            success("import", json!({
                "imported": imported,
                "errors": errors,
                "imported_count": imported.len(),
                "total_in_opml": opml_feeds.len(),
            }), vec![
                action("rss list", "List all feeds", json!({})),
                action("rss articles", "List articles", json!({})),
            ])
        }

        // ---------------------------------------------------------------
        // EXPORT
        // ---------------------------------------------------------------
        Commands::Export => {
            match db.list_feeds() {
                Ok(feeds) => {
                    let pairs: Vec<(String, String)> = feeds.iter().map(|f| (f.title.clone(), f.url.clone())).collect();
                    let opml_str = opml::generate_opml(&pairs);
                    success("export", json!({
                        "opml": opml_str,
                        "feed_count": pairs.len(),
                    }), vec![
                        action("rss list", "List all feeds", json!({})),
                        action("rss import <path>", "Import from OPML", json!({"path": {"type": "string"}})),
                    ])
                }
                Err(e) => error("export", &format!("{}", e), "Check database"),
            }
        }

        // ---------------------------------------------------------------
        // SEARCH
        // ---------------------------------------------------------------
        Commands::Search { query, count, feed, since, compact, sort } => {
            let order_by_date = sort == "date";
            let search_result = if let Some(ref since_str) = since {
                db.search_articles_since(&query, since_str, count, feed, order_by_date)
            } else {
                db.search_articles(&query, feed, order_by_date)
            };
            match search_result {
                Ok(articles) => {
                    let total = articles.len();
                    let truncated = total > count;
                    let page: Vec<Value> = if compact {
                        articles.iter().take(count)
                            .map(|a| {
                                let mut cv = compact_article(a, &db);
                                // Add match info
                                let title_match = a.title.to_lowercase().contains(&query.to_lowercase());
                                cv["matched_in"] = if title_match { json!("title") } else { json!("content") };
                                cv
                            })
                            .collect()
                    } else {
                        articles.iter().take(count)
                            .map(|a| {
                                let mut v = serde_json::to_value(a).unwrap();
                                let title_match = a.title.to_lowercase().contains(&query.to_lowercase());
                                v["matched_in"] = if title_match { json!("title") } else { json!("content") };
                                v
                            })
                            .collect()
                    };
                    let mut next: Vec<Value> = Vec::new();
                    for a in articles.iter().take(count) {
                        next.push(action(
                            &format!("rss read {}", a.id),
                            &format!("Read: {}", a.title),
                            json!({"id": {"value": a.id}}),
                        ));
                    }
                    if truncated {
                        next.push(action(
                            &format!("rss search \"{}\" --count {}", query, count + 30),
                            "Load more results",
                            json!({"count": {"value": count + 30}}),
                        ));
                    }
                    success("search", json!({
                        "query": query,
                        "since": since,
                        "articles": page,
                        "count": page.len(),
                        "total": total,
                        "truncated": truncated,
                    }), next)
                }
                Err(e) => error("search", &format!("{}", e), "Check query syntax"),
            }
        }

        // ---------------------------------------------------------------
        // FETCH-FULL-TEXT
        // ---------------------------------------------------------------
        Commands::FetchFullText { id } => {
            // Check cache first
            match db.get_full_content(id) {
                Ok(Some(html)) => {
                    return success("fetch-full-text", json!({
                        "article_id": id,
                        "html": html,
                        "cached": true,
                    }), vec![
                        action(&format!("rss read {}", id), "Read article metadata", json!({"id": {"value": id}})),
                        action(&format!("rss star {}", id), "Star this article", json!({"id": {"value": id}})),
                    ]);
                }
                Ok(None) => { /* cache miss — proceed to extract */ }
                Err(e) => {
                    return error("fetch-full-text", &format!("DB error: {}", e), "Check article ID with `rss articles`");
                }
            }

            // Get article URL
            let url = match db.get_article_url(id) {
                Ok(Some(u)) => u,
                Ok(None) => return error("fetch-full-text", &format!("Article {} not found or has no URL", id), "Use `rss articles` to find valid IDs"),
                Err(e) => return error("fetch-full-text", &format!("{}", e), "Check article ID"),
            };

            // Extract
            match rss_extract::extract_full_text(&url) {
                Ok(content) => {
                    // Cache it
                    db.set_full_content(id, &content.html).ok();

                    let source_str = format!("{:?}", content.source);
                    success("fetch-full-text", json!({
                        "article_id": id,
                        "html": content.html,
                        "text_len": content.text_len,
                        "source": source_str,
                        "cached": false,
                    }), vec![
                        action(&format!("rss read {}", id), "Read article metadata", json!({"id": {"value": id}})),
                        action(&format!("rss star {}", id), "Star this article", json!({"id": {"value": id}})),
                    ])
                }
                Err(e) => error("fetch-full-text", &format!("Extraction failed: {}", e), &format!("Try opening {} in a browser", url)),
            }
        }

        // ---------------------------------------------------------------
        // _CLASSIFY_STATS — per-tag engagement diagnostics
        // ---------------------------------------------------------------
        Commands::ClassifyStats => {
            match db.tag_engagement_stats() {
                Ok(stats) => {
                    let items: Vec<Value> = stats.iter()
                        .map(|(tag, total, read, starred, deep)| {
                            let engagement = if *total > 0 {
                                (*read as f64 + *starred as f64 * 3.0 + *deep as f64 * 2.0) / *total as f64
                            } else {
                                0.0
                            };
                            json!({
                                "tag": tag,
                                "total": total,
                                "read": read,
                                "starred": starred,
                                "deep_read": deep,
                                "engagement": (engagement * 100.0).round() / 100.0,
                            })
                        })
                        .collect();
                    success("classify_stats", json!({
                        "window": "30 days",
                        "tags": items,
                        "method": "fact-based annotation (deterministic)",
                    }), vec![])
                }
                Err(e) => error("classify_stats", &format!("{}", e), "Check database"),
            }
        }

        // ---------------------------------------------------------------
        // _SUMMARIZE — generate tldr for articles without one
        // ---------------------------------------------------------------
        Commands::Summarize => {
            match db.list_articles_without_tldr() {
                Ok(articles) => {
                    let mut count = 0;
                    for a in &articles {
                        if let Some(tldr) = generate_tldr(a) {
                            db.set_article_tldr(a.id, &tldr).ok();
                            count += 1;
                        }
                    }
                    success("_summarize", json!({
                        "articles_summarized": count,
                        "articles_scanned": articles.len(),
                    }), vec![
                        action("rss articles --compact", "View articles with tldr", json!({})),
                    ])
                }
                Err(e) => error("_summarize", &format!("{}", e), "Check database"),
            }
        }

        // ---------------------------------------------------------------
        // _CLASSIFY (replaces _analyze)
        // ---------------------------------------------------------------
        Commands::Annotate { article, force } => {
            match annotate_articles(&db, force, article) {
                Ok((total, total_tags)) => {
                    success("_annotate", json!({
                        "articles_annotated": total,
                        "tags_assigned": total_tags,
                    }), vec![
                        action("rss folders articles long", "View long-form articles", json!({})),
                        action("rss folders articles tutorial", "View tutorials", json!({})),
                        action("rss folders articles unread", "View unread", json!({})),
                    ])
                }
                Err(e) => error("_annotate", &e, "Check database"),
            }
        }

        // ---------------------------------------------------------------
        // ENTITIES
        // ---------------------------------------------------------------
        Commands::Entities { entity_type, name, related, count } => {
            if let Some(ref rel) = related {
                match db.get_related_entities(rel, count) {
                    Ok(results) => {
                        let items: Vec<Value> = results.iter()
                            .map(|(n, t, c)| json!({"name": n, "entity_type": t, "co_occurrences": c}))
                            .collect();
                        success("entities", json!({"related_to": rel, "entities": items, "count": items.len()}), vec![])
                    }
                    Err(e) => error("entities", &format!("{}", e), "Check entity name"),
                }
            } else if let Some(ref n) = name {
                if entity_type.is_none() {
                    match db.get_entity_mentions(n) {
                        Ok(mentions) => {
                            let items: Vec<Value> = mentions.iter()
                                .map(|(aid, t, ctx, s)| json!({"article_id": aid, "entity_type": t, "context": ctx, "score": s}))
                                .collect();
                            success("entities", json!({"name": n, "mentions": items, "count": items.len()}), vec![])
                        }
                        Err(e) => error("entities", &format!("{}", e), "Check entity name"),
                    }
                } else {
                    match db.list_entities_grouped(entity_type.as_deref(), Some(n), count) {
                        Ok(results) => {
                            let items: Vec<Value> = results.iter()
                                .map(|(n, t, c, s)| json!({"name": n, "entity_type": t, "mentions": c, "avg_score": s}))
                                .collect();
                            success("entities", json!({"entities": items, "count": items.len()}), vec![])
                        }
                        Err(e) => error("entities", &format!("{}", e), "Run `rss _classify` first"),
                    }
                }
            } else {
                match db.list_entities_grouped(entity_type.as_deref(), None, count) {
                    Ok(results) => {
                        let items: Vec<Value> = results.iter()
                            .map(|(n, t, c, s)| json!({"name": n, "entity_type": t, "mentions": c, "avg_score": s}))
                            .collect();
                        success("entities", json!({"entities": items, "count": items.len()}), vec![])
                    }
                    Err(e) => error("entities", &format!("{}", e), "Run `rss _classify` first"),
                }
            }
        }

        // ---------------------------------------------------------------
        // FOLDERS
        // ---------------------------------------------------------------
        Commands::Folders { action: folder_action } => {
            match folder_action {
                None => {
                    // List: smart views (fact-based) + manual folders from DB
                    let mut items: Vec<Value> = Vec::new();

                    // Add smart views with article counts
                    // "unread" — count unread articles
                    let unread_count = db.list_articles(None, true).map(|a| a.len()).unwrap_or(0);
                    items.push(json!({"id": null, "name": "unread", "type": "smart_view", "description": "Unread articles", "article_count": unread_count}));

                    // "long" — word_count based long-form detection
                    let long_count = db.count_long_form_articles().unwrap_or(0);
                    items.push(json!({"id": null, "name": "long", "type": "smart_view", "description": "Long-form articles (800+ words)", "article_count": long_count}));

                    // "tutorial" — requires multiple code blocks + steps or sufficient length
                    let tutorial_count = db.count_tutorial_articles().unwrap_or(0);
                    items.push(json!({"id": null, "name": "tutorial", "type": "smart_view", "description": "Tutorials (2+ code blocks with steps)", "article_count": tutorial_count}));

                    // "recent" — today's articles
                    let recent_count = db.search_articles_since("", "24h", 9999, None, true).map(|a| a.len()).unwrap_or(0);
                    items.push(json!({"id": null, "name": "recent", "type": "smart_view", "description": "Today's articles", "article_count": recent_count}));

                    // Add manual folders from DB
                    if let Ok(folders) = db.list_folders() {
                        for (id, name, ftype, query) in &folders {
                            items.push(json!({"id": id, "name": name, "type": ftype, "query": query}));
                        }
                    }

                    success("folders", json!({"folders": items, "count": items.len()}), vec![
                        action("rss folders articles unread", "View unread articles", json!({})),
                        action("rss folders articles long", "View long-form articles", json!({})),
                        action("rss folders articles tutorial", "View tutorials", json!({})),
                        action("rss folders articles recent", "View today's articles", json!({})),
                        action("rss folders create <name>", "Create manual folder", json!({})),
                    ])
                }
                Some(FolderAction::Create { name, smart, feeds }) => {
                    if let Some(ref query) = smart {
                        match db.create_folder(&name, "smart", Some(query)) {
                            Ok(id) => success("folders", json!({"folder_id": id, "name": name, "type": "smart", "query": query}), vec![
                                action(&format!("rss folders articles {}", id), "View folder articles", json!({})),
                            ]),
                            Err(e) => error("folders", &format!("{}", e), "Check query syntax"),
                        }
                    } else {
                        match db.count_manual_folders() {
                            Ok(count) if count >= 4 => {
                                return error("folders", "Maximum 4 manual folders allowed", "Delete a folder first");
                            }
                            _ => {}
                        }
                        match db.create_folder(&name, "manual", None) {
                            Ok(id) => {
                                if let Some(ref feed_ids) = feeds {
                                    for fid_str in feed_ids.split(',') {
                                        if let Ok(fid) = fid_str.trim().parse::<i64>() {
                                            db.add_feed_to_folder(id, fid).ok();
                                        }
                                    }
                                }
                                success("folders", json!({"action": "create", "id": id, "name": name, "type": "manual"}), vec![
                                    action(&format!("rss folders articles {}", id), "View folder articles", json!({})),
                                ])
                            }
                            Err(e) => error("folders", &format!("{}", e), "Check parameters"),
                        }
                    }
                }
                Some(FolderAction::Remove { id }) => {
                    match db.remove_folder(id) {
                        Ok(true) => success("folders", json!({"removed": id}), vec![
                            action("rss folders", "List remaining folders", json!({})),
                        ]),
                        Ok(false) => error("folders", &format!("Folder {} not found", id), "Use `rss folders` to see IDs"),
                        Err(e) => error("folders", &format!("{}", e), "Check folder ID"),
                    }
                }
                Some(FolderAction::Articles { name, count }) => {
                    // Smart views: fact-based queries
                    if is_smart_view(&name) {
                        let result = match name.as_str() {
                            "unread" => db.list_articles(None, true),
                            "long" => db.get_long_form_articles(count),
                            "tutorial" => db.get_tutorial_articles(count),
                            "recent" => db.search_articles_since("", "24h", count, None, true),
                            _ => unreachable!(),
                        };
                        match result {
                            Ok(arts) => {
                                let items: Vec<Value> = arts.iter().take(count)
                                    .map(|a| serde_json::to_value(a).unwrap())
                                    .collect();
                                success("folders", json!({"view": name, "articles": items, "count": items.len()}), vec![])
                            }
                            Err(e) => error("folders", &format!("{}", e), "Check database"),
                        }
                    } else if let Ok(id) = name.parse::<i64>() {
                        // Manual folder by ID
                        match db.get_folder_feed_articles(id, count) {
                            Ok(arts) => {
                                let items: Vec<Value> = arts.iter()
                                    .map(|a| serde_json::to_value(a).unwrap())
                                    .collect();
                                success("folders", json!({"folder_id": id, "articles": items, "count": items.len()}), vec![])
                            }
                            Err(e) => error("folders", &format!("{}", e), "Use `rss folders` to see IDs"),
                        }
                    } else {
                        error("folders", &format!("Unknown view: {}. Use one of: unread, long, tutorial, recent, or a numeric folder ID", name), "Use `rss folders` to list available views")
                    }
                }
            }
        }

        // ---------------------------------------------------------------
        // MOVE-FEED
        // ---------------------------------------------------------------
        Commands::MoveFeed { feed_id, folder } => {
            match db.move_feed_to_folder(feed_id, folder) {
                Ok(true) => success("feeds", json!({
                    "action": "move",
                    "feed_id": feed_id,
                    "folder_id": folder,
                }), vec![]),
                Ok(false) => error("feeds", "Feed not found", "Check feed ID"),
                Err(e) => error("feeds", &format!("{}", e), "Check IDs"),
            }
        }

        // ---------------------------------------------------------------
        // READ-FOR-ME
        // ---------------------------------------------------------------
        Commands::ReadForMe { since, count } => {
            let hours: i64 = if since.ends_with('d') {
                since.trim_end_matches('d').parse::<i64>().unwrap_or(1) * 24
            } else if since.ends_with('h') {
                since.trim_end_matches('h').parse::<i64>().unwrap_or(24)
            } else {
                24
            };

            match db.list_articles(None, true) {
                Ok(articles) => {
                    let cutoff = chrono::Utc::now() - chrono::Duration::hours(hours);
                    let recent: Vec<&rss_core::Article> = articles.iter()
                        .filter(|a| a.published_at.map(|d| d > cutoff).unwrap_or(true))
                        .take(count)
                        .collect();

                    // Get entity data for 4-dimension analysis
                    let top_entities = db.list_entities_grouped(None, None, 30).unwrap_or_default();

                    // Dimension 1: Most relevant (starred or from feeds user reads most)
                    let starred: Vec<Value> = recent.iter()
                        .filter(|a| a.is_starred)
                        .take(4)
                        .map(|a| article_snippet(a))
                        .collect();

                    // Dimension 2: Trending (entities with high recent frequency)
                    let trending_entities: Vec<Value> = top_entities.iter()
                        .take(8)
                        .map(|(n, t, c, s)| json!({"name": n, "type": t, "mentions": c, "score": s}))
                        .collect();

                    // Dimension 3: Cross-feed connections
                    let important: Vec<Value> = recent.iter()
                        .filter(|a| !a.is_starred)
                        .take(8)
                        .map(|a| article_snippet(a))
                        .collect();

                    // Dimension 4: Safe to skip
                    let skip_count = recent.len().saturating_sub(starred.len() + important.len());

                    success("read-for-me", json!({
                        "period": since,
                        "total_unread": articles.len(),
                        "dimensions": {
                            "most_relevant": {
                                "description": "Articles you starred or align with your interests",
                                "articles": starred,
                                "count": starred.len(),
                            },
                            "trending_now": {
                                "description": "Entities and topics surging in your feeds right now",
                                "entities": trending_entities,
                                "articles": important,
                                "count": important.len(),
                            },
                            "cross_feed": {
                                "description": "Concepts appearing across multiple feeds",
                                "entities": top_entities.iter()
                                    .filter(|(_, _, c, _)| *c >= 3)
                                    .take(4)
                                    .map(|(n, t, c, _)| json!({"name": n, "type": t, "across_articles": c}))
                                    .collect::<Vec<Value>>(),
                            },
                            "safe_to_skip": {
                                "description": "Remaining articles — skip unless a topic catches your eye",
                                "count": skip_count,
                            },
                        },
                    }), vec![
                        action("rss read-for-me | claude \"based on these 4 dimensions, what should I read today?\"", "AI daily briefing", json!({})),
                    ])
                }
                Err(e) => error("read-for-me", &format!("{}", e), "Check database"),
            }
        }

        // ---------------------------------------------------------------
        // FEED-HEALTH
        // ---------------------------------------------------------------
        Commands::FeedHealth { feed, compact } => {
            match db.get_feed_health(feed) {
                Ok(health) => {
                    let mut healthy = 0i64;
                    let mut degraded = 0i64;
                    let mut dead = 0i64;

                    let items: Vec<serde_json::Value> = health.iter().map(|h| {
                        let status = if h.fail_count == 0 {
                            healthy += 1;
                            "healthy"
                        } else if h.fail_count <= 3 {
                            degraded += 1;
                            "degraded"
                        } else {
                            dead += 1;
                            "dead"
                        };
                        if compact {
                            json!({
                                "id": h.id,
                                "title": h.title,
                                "status": status,
                                "fail_count": h.fail_count,
                                "avg_response_ms": h.avg_response_ms,
                            })
                        } else {
                            json!({
                                "id": h.id,
                                "title": h.title,
                                "url": h.url,
                                "status": status,
                                "last_fetch_at": h.last_fetch_at,
                                "last_error": h.last_error,
                                "fail_count": h.fail_count,
                                "avg_response_ms": h.avg_response_ms,
                            })
                        }
                    }).collect();

                    let total = items.len() as i64;
                    success("feed-health", json!({
                        "feeds": items,
                        "summary": {
                            "total": total,
                            "healthy": healthy,
                            "degraded": degraded,
                            "dead": dead,
                        },
                    }), vec![
                        action("rss fetch", "Fetch all feeds (updates health data)", json!({})),
                        action("rss list", "List all feeds", json!({})),
                    ])
                }
                Err(e) => error("feed-health", &format!("{}", e), "Check database"),
            }
        }
    }
}
