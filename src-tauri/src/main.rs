#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rss_core::{Feed, Article};
use rss_core::parser::parse_feed;
use rss_core::opml;
use rss_fetch::fetch_feed_bytes;
use rss_store::Database;
use serde::Serialize;
use std::sync::Mutex;
use tauri::State;

struct AppState {
    db: Mutex<Database>,
}

#[derive(Serialize)]
struct FeedResult {
    feed: Feed,
    article_count: usize,
}

#[tauri::command]
fn list_feeds(state: State<AppState>) -> Result<Vec<Feed>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_feeds().map_err(|e| e.to_string())
}

#[tauri::command]
fn add_feed(url: String, state: State<AppState>) -> Result<FeedResult, String> {
    let bytes = fetch_feed_bytes(&url).map_err(|e| e.to_string())?;
    let (feed, articles) = parse_feed(&url, &bytes).map_err(|e| e.to_string())?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let feed_id = db.add_feed(&feed).map_err(|e| e.to_string())?;
    let count = db.add_articles(feed_id, &articles).unwrap_or(0);
    let mut saved_feed = feed;
    saved_feed.id = feed_id;
    Ok(FeedResult { feed: saved_feed, article_count: count })
}

#[tauri::command]
fn remove_feed(id: i64, state: State<AppState>) -> Result<bool, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.remove_feed(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn fetch_feeds(state: State<AppState>) -> Result<Vec<String>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let feeds = db.list_feeds().map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for feed in &feeds {
        match fetch_feed_bytes(&feed.url) {
            Ok(bytes) => match parse_feed(&feed.url, &bytes) {
                Ok((_, articles)) => {
                    let count = db.add_articles(feed.id, &articles).unwrap_or(0);
                    results.push(format!("{}: {} new", feed.title, count));
                }
                Err(e) => results.push(format!("{}: parse error: {}", feed.title, e)),
            },
            Err(e) => results.push(format!("{}: fetch error: {}", feed.title, e)),
        }
    }
    Ok(results)
}

#[tauri::command]
fn list_articles(feed_id: Option<i64>, unread_only: bool, state: State<AppState>) -> Result<Vec<Article>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.list_articles(feed_id, unread_only).map_err(|e| e.to_string())
}

#[tauri::command]
fn mark_read(id: i64, state: State<AppState>) -> Result<bool, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.mark_read(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn toggle_star(id: i64, state: State<AppState>) -> Result<bool, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.toggle_star(id).map_err(|e| e.to_string())
}

#[tauri::command]
fn import_opml(data: String, state: State<AppState>) -> Result<Vec<String>, String> {
    let opml_feeds = opml::parse_opml(&data).map_err(|e| e.to_string())?;
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for opml_feed in &opml_feeds {
        match fetch_feed_bytes(&opml_feed.xml_url) {
            Ok(bytes) => match parse_feed(&opml_feed.xml_url, &bytes) {
                Ok((feed, articles)) => {
                    match db.add_feed(&feed) {
                        Ok(feed_id) => {
                            let count = db.add_articles(feed_id, &articles).unwrap_or(0);
                            results.push(format!("Imported: {} ({} articles)", feed.title, count));
                        }
                        Err(e) => results.push(format!("Skip {}: {}", opml_feed.title, e)),
                    }
                }
                Err(e) => results.push(format!("Parse error {}: {}", opml_feed.title, e)),
            },
            Err(e) => results.push(format!("Fetch error {}: {}", opml_feed.title, e)),
        }
    }
    Ok(results)
}

#[tauri::command]
fn export_opml(state: State<AppState>) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let feeds = db.list_feeds().map_err(|e| e.to_string())?;
    let pairs: Vec<(String, String)> = feeds.into_iter().map(|f| (f.title, f.url)).collect();
    Ok(opml::generate_opml(&pairs))
}

#[tauri::command]
fn search_articles(query: String, state: State<AppState>) -> Result<Vec<Article>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.search_articles(&query).map_err(|e| e.to_string())
}

fn main() {
    let db_path = {
        let dir = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
        let app_dir = dir.join("rss-reader");
        std::fs::create_dir_all(&app_dir).ok();
        app_dir.join("feeds.db").to_string_lossy().to_string()
    };

    let db = Database::open(&db_path).expect("Failed to open database");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState { db: Mutex::new(db) })
        .invoke_handler(tauri::generate_handler![
            list_feeds,
            add_feed,
            remove_feed,
            fetch_feeds,
            list_articles,
            mark_read,
            toggle_star,
            import_opml,
            export_opml,
            search_articles,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
