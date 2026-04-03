#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use tauri::Emitter;

// ── Data structs (local, no rss-core dependency) ──────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Feed {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub added_at: String,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub last_modified_header: Option<String>,
    #[serde(default)]
    pub folder_id: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Article {
    pub id: i64,
    pub feed_id: i64,
    pub title: String,
    pub url: Option<String>,
    pub content: Option<String>,
    pub summary: Option<String>,
    pub published_at: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub fetched_at: String,
    pub tldr: Option<String>,
    pub guid: Option<String>,
    pub full_content: Option<String>,
    pub tags: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FeedResult {
    pub feed_id: i64,
    pub title: String,
    pub url: String,
    pub article_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct FetchResult {
    pub feed_id: i64,
    pub title: String,
    pub new_articles: usize,
    pub error: Option<String>,
    #[serde(default)]
    pub not_modified: Option<bool>,
}

/// Fact-based smart view
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SmartView {
    pub name: String,
    pub description: Option<String>,
    pub article_count: i64,
}

// ── CLI helpers ───────────────────────────────────────────────────────────

fn cli_path() -> String {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap();
    let cli = dir.join("lumen");
    if cli.exists() {
        return cli.to_string_lossy().to_string();
    }
    let cli_exe = dir.join("lumen.exe");
    if cli_exe.exists() {
        return cli_exe.to_string_lossy().to_string();
    }
    "lumen".to_string()
}

fn cli(args: &[&str]) -> Result<serde_json::Value, String> {
    let mut cmd = std::process::Command::new(cli_path());
    cmd.args(args);
    // Hide the console window on Windows
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let output = cmd.output()
        .map_err(|e| format!("Failed to run lumen: {}", e))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let envelope: serde_json::Value = serde_json::from_str(&stdout)
        .map_err(|e| format!("Invalid CLI output: {} — raw: {}", e, stdout))?;
    if envelope["ok"].as_bool() == Some(true) {
        Ok(envelope["result"].clone())
    } else {
        Err(envelope["error"]
            .as_str()
            .unwrap_or("Unknown error")
            .to_string())
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────

#[tauri::command]
fn list_feeds() -> Result<Vec<Feed>, String> {
    let result = cli(&["list"])?;
    let feeds: Vec<Feed> = serde_json::from_value(result["feeds"].clone())
        .map_err(|e| format!("Failed to parse feeds: {}", e))?;
    Ok(feeds)
}

#[tauri::command]
fn add_feed(url: String) -> Result<FeedResult, String> {
    let result = cli(&["add", &url])?;
    let feed_id = result["feed_id"]
        .as_i64()
        .ok_or("Missing feed_id")?;
    let title = result["title"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let feed_url = result["url"]
        .as_str()
        .unwrap_or(&url)
        .to_string();
    let article_count = result["articles_added"]
        .as_u64()
        .unwrap_or(0) as usize;
    Ok(FeedResult {
        feed_id,
        title,
        url: feed_url,
        article_count,
    })
}

#[tauri::command]
fn remove_feed(id: i64) -> Result<bool, String> {
    cli(&["remove", &id.to_string()])?;
    Ok(true)
}

#[tauri::command]
async fn fetch_feeds(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    let _handle = std::thread::spawn(move || {
        let result = cli(&["fetch"]);
        match result {
            Ok(ref val) => {
                let results: Vec<FetchResult> = serde_json::from_value(val["results"].clone())
                    .unwrap_or_default();
                let _ = app.emit("fetch-complete", serde_json::json!({
                    "ok": true,
                    "results": results,
                }));
            }
            Err(ref e) => {
                let _ = app.emit("fetch-complete", serde_json::json!({
                    "ok": false,
                    "error": e.clone(),
                }));
            }
        }
        result
    });
    // Return immediately so UI doesn't freeze
    Ok(serde_json::json!({"status": "fetching"}))
}

#[tauri::command]
fn list_articles(feed_id: Option<i64>, unread_only: bool, count: Option<usize>, offset: Option<usize>) -> Result<serde_json::Value, String> {
    let page_size = count.unwrap_or(50);
    let skip = offset.unwrap_or(0);
    // Fetch enough articles from CLI to cover offset + page_size
    let fetch_count = (skip + page_size + 1).to_string(); // +1 to detect hasMore
    let mut args: Vec<&str> = vec!["articles", "--count", &fetch_count];
    let id_str;
    if let Some(id) = feed_id {
        id_str = id.to_string();
        args.push("--feed");
        args.push(&id_str);
    }
    if unread_only {
        args.push("--unread");
    }
    let result = cli(&args)?;
    let all_articles: Vec<Article> = serde_json::from_value(result["articles"].clone())
        .map_err(|e| format!("Failed to parse articles: {}", e))?;
    let total_fetched = all_articles.len();
    let page: Vec<&Article> = all_articles.iter().skip(skip).take(page_size).collect();
    let has_more = total_fetched > skip + page_size;
    Ok(serde_json::json!({
        "articles": page,
        "has_more": has_more,
        "offset": skip,
        "count": page.len(),
    }))
}

#[tauri::command]
fn mark_read(id: i64) -> Result<bool, String> {
    cli(&["mark-read", &id.to_string()])?;
    Ok(true)
}

#[tauri::command]
fn toggle_star(id: i64) -> Result<bool, String> {
    cli(&["star", &id.to_string()])?;
    Ok(true)
}

#[tauri::command]
fn search_articles(query: String) -> Result<Vec<Article>, String> {
    let result = cli(&["search", &query, "--count", "9999"])?;
    let articles: Vec<Article> = serde_json::from_value(result["articles"].clone())
        .map_err(|e| format!("Failed to parse articles: {}", e))?;
    Ok(articles)
}

#[tauri::command]
fn fetch_full_text(id: i64) -> Result<String, String> {
    let result = cli(&["fetch-full-text", &id.to_string()])?;
    result["html"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Missing html in result".to_string())
}

#[tauri::command]
async fn import_opml(data: String) -> Result<Vec<String>, String> {
    // Run CLI import in a background thread to avoid freezing the UI
    let handle = std::thread::spawn(move || {
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join(format!("lumen-import-{}.opml", std::process::id()));
        std::fs::write(&tmp_path, &data)
            .map_err(|e| format!("Failed to write temp file: {}", e))?;

        let path_str = tmp_path.to_string_lossy().to_string();
        let result = cli(&["import", &path_str]);

        let _ = std::fs::remove_file(&tmp_path);

        let result = result?;

        let mut status: Vec<String> = Vec::new();
        if let Some(imported) = result["imported"].as_array() {
            for item in imported {
                if let Some(s) = item.as_str() {
                    status.push(s.to_string());
                }
            }
        }
        if let Some(errors) = result["errors"].as_array() {
            for item in errors {
                if let Some(s) = item.as_str() {
                    status.push(s.to_string());
                }
            }
        }
        Ok::<Vec<String>, String>(status)
    });
    handle.join().map_err(|_| "Import thread panicked".to_string())?
}

#[tauri::command]
fn export_opml() -> Result<String, String> {
    let result = cli(&["export"])?;
    result["opml"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "Missing opml in result".to_string())
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Folder {
    pub id: Option<i64>,
    pub name: String,
    #[serde(rename = "type")]
    pub folder_type: String,
    pub query: Option<String>,
    pub article_count: Option<i64>,
}

#[tauri::command]
fn list_folders() -> Result<Vec<Folder>, String> {
    let result = cli(&["folders"])?;
    let folders: Vec<Folder> = serde_json::from_value(result["folders"].clone())
        .map_err(|e| format!("Failed to parse folders: {}", e))?;
    Ok(folders)
}

#[tauri::command]
fn folder_articles(tag: String, count: Option<usize>) -> Result<Vec<Article>, String> {
    let count_str = (count.unwrap_or(100)).to_string();
    let result = cli(&["folders", "articles", &tag, "--count", &count_str])?;
    let articles: Vec<Article> = serde_json::from_value(result["articles"].clone())
        .map_err(|e| format!("Failed to parse articles: {}", e))?;
    Ok(articles)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AnnotateResult {
    pub articles_annotated: i64,
    pub tags_assigned: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FolderCount {
    pub name: String,
    pub count: i64,
}

#[tauri::command]
fn annotate_articles() -> Result<AnnotateResult, String> {
    let result = cli(&["_annotate"])?;
    let annotate: AnnotateResult = serde_json::from_value(result.clone())
        .map_err(|e| format!("Failed to parse annotate result: {}", e))?;
    Ok(annotate)
}

#[tauri::command]
fn create_folder(name: String) -> Result<Folder, String> {
    let result = cli(&["folders", "create", &name])?;
    let id = result["id"].as_i64();
    let folder_name = result["name"].as_str().unwrap_or(&name).to_string();
    Ok(Folder {
        id,
        name: folder_name,
        folder_type: "manual".to_string(),
        query: None,
        article_count: None,
    })
}

#[tauri::command]
fn delete_folder(id: i64) -> Result<bool, String> {
    cli(&["folders", "remove", &id.to_string()])?;
    Ok(true)
}

#[tauri::command]
fn move_feed(feed_id: i64, folder_id: Option<i64>) -> Result<bool, String> {
    let mut args = vec!["move-feed".to_string(), feed_id.to_string()];
    if let Some(fid) = folder_id {
        args.push("--folder".to_string());
        args.push(fid.to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    cli(&arg_refs)?;
    Ok(true)
}

#[tauri::command]
fn folder_counts() -> Result<Vec<FolderCount>, String> {
    let result = cli(&["folders"])?;
    let folders: Vec<FolderCount> = serde_json::from_value(result["folders"].clone())
        .map_err(|e| format!("Failed to parse folder counts: {}", e))?;
    Ok(folders)
}

// ── Main ──────────────────────────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
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
            fetch_full_text,
            list_folders,
            folder_articles,
            folder_counts,
            annotate_articles,
            create_folder,
            delete_folder,
            move_feed,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
