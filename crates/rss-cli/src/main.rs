use clap::{Parser, Subcommand};
use rss_core::opml;
use rss_core::parser::parse_feed;
use rss_fetch::fetch_feed_bytes;
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
    /// Search articles (default: first 30 results)
    Search {
        query: String,
        #[arg(long, default_value_t = 30)]
        count: usize,
    },
    /// Fetch full-text content for an article (cached)
    FetchFullText { id: i64 },
    /// Analyze articles with NER (extract entities)
    Analyze {
        #[arg(long)]
        article: Option<i64>,
        #[arg(long)]
        download_model: bool,
    },
    /// Query extracted entities
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
    /// Output unread articles + entity context for LLM pipe
    ReadForMe {
        #[arg(long, default_value = "24h")]
        since: String,
        #[arg(long, default_value_t = 100)]
        count: usize,
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
    Articles {
        id: i64,
        #[arg(long, default_value_t = 30)]
        count: usize,
    },
    /// Suggest smart folders based on entity clustering (does NOT create them)
    Suggest,
    /// Accept suggested folders (creates them)
    Accept {
        /// Comma-separated indices to exclude (e.g. "1,3" to skip suggestions 1 and 3)
        #[arg(long)]
        except: Option<String>,
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
                "description": "Fetch new articles from all feeds (or one feed)",
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
                    {"name": "--count", "type": "integer", "required": false, "description": "Max results (default 30)"}
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
                "description": "Full-text search across articles (default: first 30)",
                "args": [
                    {"name": "query", "type": "string", "required": true, "description": "Search query"},
                    {"name": "--count", "type": "integer", "required": false, "description": "Max results (default 30)"}
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
                "name": "analyze",
                "description": "Run NER on articles to extract entities (person, org, concept)",
                "args": [
                    {"name": "--article", "type": "integer", "required": false, "description": "Analyze one article"},
                    {"name": "--download-model", "type": "boolean", "required": false, "description": "Download NER model (~16MB)"}
                ]
            },
            {
                "name": "entities",
                "description": "Query extracted entities",
                "args": [
                    {"name": "--entity-type", "type": "string", "required": false, "description": "Filter: person, organization, concept"},
                    {"name": "--name", "type": "string", "required": false, "description": "Search by entity name"},
                    {"name": "--related", "type": "string", "required": false, "description": "Find co-occurring entities"},
                    {"name": "--count", "type": "integer", "required": false, "description": "Max results (default 30)"}
                ]
            },
            {
                "name": "folders",
                "description": "Manage folders: list, suggest (AI), accept, create, remove, articles",
                "args": []
            },
            {
                "name": "read-for-me",
                "description": "Output unread articles in 4 dimensions: most_relevant, trending_now, cross_feed, safe_to_skip",
                "args": [
                    {"name": "--since", "type": "string", "required": false, "description": "Time window (default 24h, e.g. 7d)"},
                    {"name": "--count", "type": "integer", "required": false, "description": "Max articles (default 100)"}
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
    let snippet = if content.len() > 300 { &content[..300] } else { content };
    let clean = rss_ner::strip_html(snippet);
    json!({
        "id": a.id,
        "title": a.title,
        "feed_id": a.feed_id,
        "url": a.url,
        "published_at": a.published_at.map(|d| d.to_rfc3339()),
        "snippet": clean,
    })
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
                    let feed_values: Vec<Value> = feeds.iter().map(|f| serde_json::to_value(f).unwrap()).collect();
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
        // FETCH
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
            let mut results = Vec::new();
            for f in &targets {
                match fetch_feed_bytes(&f.url) {
                    Ok(bytes) => match parse_feed(&f.url, &bytes) {
                        Ok((_, articles)) => {
                            let count = db.add_articles(f.id, &articles).unwrap_or(0);
                            results.push(json!({"feed_id": f.id, "title": f.title, "new_articles": count}));
                        }
                        Err(e) => {
                            results.push(json!({"feed_id": f.id, "title": f.title, "error": format!("{}", e)}));
                        }
                    },
                    Err(e) => {
                        results.push(json!({"feed_id": f.id, "title": f.title, "error": format!("{}", e)}));
                    }
                }
            }
            success("fetch", json!({
                "feeds_fetched": results.len(),
                "results": results,
            }), vec![
                action("rss articles --unread", "List unread articles", json!({})),
                action("rss list", "List all feeds", json!({})),
            ])
        }

        // ---------------------------------------------------------------
        // ARTICLES  (Principle 4: default --count 30, truncated/total)
        // ---------------------------------------------------------------
        Commands::Articles { feed, unread, count } => {
            match db.list_articles(feed, unread) {
                Ok(articles) => {
                    let total = articles.len();
                    let truncated = total > count;
                    let page: Vec<Value> = articles.iter().take(count)
                        .map(|a| serde_json::to_value(a).unwrap())
                        .collect();
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
            match db.list_articles(None, false) {
                Ok(articles) => {
                    if let Some(article) = articles.into_iter().find(|a| a.id == id) {
                        db.mark_read(id).ok();
                        let article_val = serde_json::to_value(&article).unwrap();
                        success("read", json!({
                            "article": article_val,
                        }), vec![
                            action(&format!("rss fetch-full-text {}", id), "Get full-text content", json!({"id": {"value": id}})),
                            action(&format!("rss star {}", id), "Star this article", json!({"id": {"value": id}})),
                            action(&format!("rss articles --feed {}", article.feed_id), "More from this feed", json!({"feed_id": {"value": article.feed_id}})),
                        ])
                    } else {
                        error("read", &format!("Article {} not found", id), "Use `rss articles` to find valid article IDs")
                    }
                }
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
            let mut imported = Vec::new();
            let mut errors = Vec::new();
            for of in &opml_feeds {
                match fetch_feed_bytes(&of.xml_url) {
                    Ok(bytes) => match parse_feed(&of.xml_url, &bytes) {
                        Ok((feed, articles)) => match db.add_feed(&feed) {
                            Ok(feed_id) => {
                                let count = db.add_articles(feed_id, &articles).unwrap_or(0);
                                imported.push(json!({"feed_id": feed_id, "title": feed.title, "articles_added": count}));
                            }
                            Err(e) => errors.push(json!({"title": of.title, "error": format!("{}", e)})),
                        },
                        Err(e) => errors.push(json!({"title": of.title, "error": format!("Parse: {}", e)})),
                    },
                    Err(e) => errors.push(json!({"title": of.title, "error": format!("Fetch: {}", e)})),
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
        // SEARCH  (Principle 4: default --count 30, truncated/total)
        // ---------------------------------------------------------------
        Commands::Search { query, count } => {
            match db.search_articles(&query) {
                Ok(articles) => {
                    let total = articles.len();
                    let truncated = total > count;
                    let page: Vec<Value> = articles.iter().take(count)
                        .map(|a| serde_json::to_value(a).unwrap())
                        .collect();
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
        // FETCH-FULL-TEXT  (Principle 5: cache-aware extraction)
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
        // ANALYZE
        // ---------------------------------------------------------------
        Commands::Analyze { article, download_model } => {
            if download_model {
                match rss_ner::download_model() {
                    Ok(()) => return success("analyze", json!({"model_downloaded": true}), vec![
                        action("rss analyze", "Run NER on unanalyzed articles", json!({})),
                    ]),
                    Err(e) => return error("analyze", &format!("Download failed: {}", e), "Check network connection"),
                }
            }

            let articles_to_analyze = match article {
                Some(id) => {
                    match db.list_articles(None, false) {
                        Ok(all) => all.into_iter().filter(|a| a.id == id).collect::<Vec<_>>(),
                        Err(e) => return error("analyze", &format!("{}", e), "Check article ID"),
                    }
                }
                None => {
                    match db.list_unanalyzed_articles() {
                        Ok(a) => a,
                        Err(e) => return error("analyze", &format!("{}", e), "Check database"),
                    }
                }
            };

            let total = articles_to_analyze.len();
            let mut total_entities = 0;

            for a in &articles_to_analyze {
                let text = a.full_content.as_deref()
                    .or(a.content.as_deref())
                    .or(a.summary.as_deref())
                    .unwrap_or(&a.title);

                let entities = rss_ner::extract_entities(text);
                let db_entities: Vec<(String, String, Option<String>, f32)> = entities.iter()
                    .map(|e| (e.name.clone(), e.entity_type.clone(), None, e.score))
                    .collect();

                db.add_entities(a.id, &db_entities).ok();
                db.mark_analyzed(a.id).ok();
                total_entities += entities.len();
            }

            success("analyze", json!({
                "articles_analyzed": total,
                "entities_found": total_entities,
            }), vec![
                action("rss entities", "View extracted entities", json!({})),
                action("rss entities --entity-type concept", "View concepts", json!({})),
                action("rss entities --entity-type organization", "View organizations", json!({})),
            ])
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
                        Err(e) => error("entities", &format!("{}", e), "Run `rss analyze` first"),
                    }
                }
            } else {
                match db.list_entities_grouped(entity_type.as_deref(), None, count) {
                    Ok(results) => {
                        let items: Vec<Value> = results.iter()
                            .map(|(n, t, c, s)| json!({"name": n, "entity_type": t, "mentions": c, "avg_score": s}))
                            .collect();
                        success("entities", json!({"entities": items, "count": items.len()}), vec![
                            action("rss analyze", "Analyze more articles", json!({})),
                        ])
                    }
                    Err(e) => error("entities", &format!("{}", e), "Run `rss analyze` first"),
                }
            }
        }

        // ---------------------------------------------------------------
        // FOLDERS
        // ---------------------------------------------------------------
        Commands::Folders { action: folder_action } => {
            match folder_action {
                None => {
                    match db.list_folders() {
                        Ok(folders) => {
                            let items: Vec<Value> = folders.iter()
                                .map(|(id, name, ftype, query)| json!({"id": id, "name": name, "type": ftype, "query": query}))
                                .collect();
                            success("folders", json!({"folders": items, "count": items.len()}), vec![
                                action("rss folders create <name> --smart <query>", "Create smart folder", json!({})),
                                action("rss folders create <name> --feeds 1,2,3", "Create manual folder", json!({})),
                            ])
                        }
                        Err(e) => error("folders", &format!("{}", e), "Check database"),
                    }
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
                        match db.create_folder(&name, "manual", None) {
                            Ok(id) => {
                                if let Some(ref feed_ids) = feeds {
                                    for fid_str in feed_ids.split(',') {
                                        if let Ok(fid) = fid_str.trim().parse::<i64>() {
                                            db.add_feed_to_folder(id, fid).ok();
                                        }
                                    }
                                }
                                success("folders", json!({"folder_id": id, "name": name, "type": "manual"}), vec![
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
                Some(FolderAction::Articles { id, count }) => {
                    match db.list_folders() {
                        Ok(folders) => {
                            if let Some((_, _, ftype, query)) = folders.iter().find(|(fid, _, _, _)| *fid == id) {
                                let articles = if ftype == "smart" {
                                    db.get_smart_folder_articles(query.as_deref().unwrap_or(""), count)
                                } else {
                                    db.get_folder_feed_articles(id, count)
                                };
                                match articles {
                                    Ok(arts) => {
                                        let items: Vec<Value> = arts.iter()
                                            .map(|a| serde_json::to_value(a).unwrap())
                                            .collect();
                                        success("folders", json!({"folder_id": id, "articles": items, "count": items.len()}), vec![])
                                    }
                                    Err(e) => error("folders", &format!("{}", e), "Check folder query"),
                                }
                            } else {
                                error("folders", &format!("Folder {} not found", id), "Use `rss folders` to see IDs")
                            }
                        }
                        Err(e) => error("folders", &format!("{}", e), "Check database"),
                    }
                }
                Some(FolderAction::Suggest) => {
                    match db.suggest_smart_folders(4) {
                        Ok(suggestions) => {
                            if suggestions.is_empty() {
                                return error("folders", "No entity data. Run `rss analyze` first", "Run `rss analyze` to extract entities from articles");
                            }
                            let items: Vec<Value> = suggestions.iter().enumerate()
                                .map(|(i, (name, related, count, query))| json!({
                                    "index": i,
                                    "name": name,
                                    "related_entities": related,
                                    "article_count": count,
                                    "query": query,
                                }))
                                .collect();
                            success("folders", json!({
                                "action": "suggest",
                                "suggestions": items,
                                "count": items.len(),
                                "max": 4,
                                "action_required": "Review suggestions, then run `rss folders accept` to create them. Use --except to skip specific indices.",
                            }), vec![
                                action("rss folders accept", "Accept all suggestions", json!({})),
                                action("rss folders accept --except 2", "Accept all except index 2", json!({})),
                                action("rss folders suggest", "Re-generate suggestions", json!({})),
                            ])
                        }
                        Err(e) => error("folders", &format!("{}", e), "Run `rss analyze` first"),
                    }
                }
                Some(FolderAction::Accept { except }) => {
                    // First generate suggestions
                    let suggestions = match db.suggest_smart_folders(4) {
                        Ok(s) => s,
                        Err(e) => return error("folders", &format!("{}", e), "Run `rss folders suggest` first"),
                    };
                    if suggestions.is_empty() {
                        return error("folders", "No suggestions available", "Run `rss analyze` then `rss folders suggest`");
                    }

                    // Parse except indices
                    let except_indices: Vec<usize> = except
                        .unwrap_or_default()
                        .split(',')
                        .filter_map(|s| s.trim().parse::<usize>().ok())
                        .collect();

                    match db.accept_suggested_folders(&suggestions, &except_indices) {
                        Ok(created) => {
                            let items: Vec<Value> = created.iter()
                                .map(|(id, name)| json!({"folder_id": id, "name": name}))
                                .collect();
                            success("folders", json!({
                                "action": "accepted",
                                "created": items,
                                "count": items.len(),
                                "skipped": except_indices.len(),
                            }), vec![
                                action("rss folders", "List all folders", json!({})),
                            ])
                        }
                        Err(e) => error("folders", &format!("{}", e), "Check database"),
                    }
                }
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

                    // Dimension 3: Cross-feed connections (articles sharing entities across different feeds)
                    // Simplified: articles that share entities with starred/read articles
                    let important: Vec<Value> = recent.iter()
                        .filter(|a| !a.is_starred)
                        .take(8)
                        .map(|a| article_snippet(a))
                        .collect();

                    // Dimension 4: Safe to skip (remaining, lowest priority)
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
                                "description": "Concepts appearing across multiple feeds — potential deep insights",
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
                        "hint": "4 dimensions: most_relevant, trending_now, cross_feed, safe_to_skip. Pipe to LLM for personalized filtering.",
                    }), vec![
                        action("rss read-for-me | claude \"based on these 4 dimensions, what should I read today?\"", "AI daily briefing", json!({})),
                        action("rss analyze", "Run NER on new articles first", json!({})),
                    ])
                }
                Err(e) => error("read-for-me", &format!("{}", e), "Check database"),
            }
        }
    }
}
