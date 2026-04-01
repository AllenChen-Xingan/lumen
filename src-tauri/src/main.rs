#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};

// ── Data structs (local, no rss-core dependency) ──────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Feed {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub added_at: String,
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
}

#[derive(Serialize, Clone, Debug)]
pub struct FeedResult {
    pub feed_id: i64,
    pub title: String,
    pub url: String,
    pub article_count: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FetchResult {
    pub feed_id: i64,
    pub title: String,
    pub new_articles: usize,
    pub error: Option<String>,
}

// ── CLI helpers ───────────────────────────────────────────────────────────

fn cli_path() -> String {
    let exe = std::env::current_exe().unwrap();
    let dir = exe.parent().unwrap();
    let cli = dir.join("rss-cli");
    if cli.exists() {
        return cli.to_string_lossy().to_string();
    }
    let cli_exe = dir.join("rss-cli.exe");
    if cli_exe.exists() {
        return cli_exe.to_string_lossy().to_string();
    }
    "rss-cli".to_string()
}

fn cli(args: &[&str]) -> Result<serde_json::Value, String> {
    let output = std::process::Command::new(cli_path())
        .args(args)
        .output()
        .map_err(|e| format!("Failed to run rss-cli: {}", e))?;
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
fn fetch_feeds() -> Result<Vec<FetchResult>, String> {
    let result = cli(&["fetch"])?;
    let results: Vec<FetchResult> = serde_json::from_value(result["results"].clone())
        .map_err(|e| format!("Failed to parse fetch results: {}", e))?;
    Ok(results)
}

#[tauri::command]
fn list_articles(feed_id: Option<i64>, unread_only: bool) -> Result<Vec<Article>, String> {
    let mut args: Vec<&str> = vec!["articles", "--count", "9999"];
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
    let articles: Vec<Article> = serde_json::from_value(result["articles"].clone())
        .map_err(|e| format!("Failed to parse articles: {}", e))?;
    Ok(articles)
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
fn import_opml(data: String) -> Result<Vec<String>, String> {
    // Write OPML data to a temp file so the CLI can read it
    let tmp_dir = std::env::temp_dir();
    let tmp_path = tmp_dir.join(format!("rss-import-{}.opml", std::process::id()));
    std::fs::write(&tmp_path, &data)
        .map_err(|e| format!("Failed to write temp file: {}", e))?;

    let path_str = tmp_path.to_string_lossy().to_string();
    let result = cli(&["import", &path_str]);

    // Clean up temp file regardless of outcome
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
    Ok(status)
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
    pub id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub folder_type: String,
    pub query: Option<String>,
}

#[tauri::command]
fn list_folders() -> Result<Vec<Folder>, String> {
    let result = cli(&["folders"])?;
    let folders: Vec<Folder> = serde_json::from_value(result["folders"].clone())
        .map_err(|e| format!("Failed to parse folders: {}", e))?;
    Ok(folders)
}

#[tauri::command]
fn folder_articles(id: i64, count: Option<usize>) -> Result<Vec<Article>, String> {
    let count_str = (count.unwrap_or(100)).to_string();
    let result = cli(&["folders", "articles", &id.to_string(), "--count", &count_str])?;
    let articles: Vec<Article> = serde_json::from_value(result["articles"].clone())
        .map_err(|e| format!("Failed to parse articles: {}", e))?;
    Ok(articles)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AnalyzeResult {
    pub articles_analyzed: i64,
    pub entities_found: i64,
}

#[tauri::command]
fn analyze_articles() -> Result<AnalyzeResult, String> {
    let result = cli(&["_analyze"])?;
    let analyze: AnalyzeResult = serde_json::from_value(result.clone())
        .map_err(|e| format!("Failed to parse analyze result: {}", e))?;
    Ok(analyze)
}

#[tauri::command]
fn create_folder(name: String) -> Result<Folder, String> {
    let result = cli(&["folders", "create", &name])?;
    let id = result["id"].as_i64().ok_or("Missing folder id")?;
    let folder_name = result["name"].as_str().unwrap_or(&name).to_string();
    Ok(Folder {
        id,
        name: folder_name,
        folder_type: "manual".to_string(),
        query: None,
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SmartFolderSuggestion {
    pub index: usize,
    pub name: String,
    pub related_entities: Option<String>,
    pub article_count: i64,
    pub query: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ResetResult {
    pub deleted: usize,
    pub reason: String,
    pub new_suggestions: Vec<SmartFolderSuggestion>,
}

#[tauri::command]
fn suggest_folders() -> Result<Vec<SmartFolderSuggestion>, String> {
    let result = cli(&["folders", "suggest"])?;
    let suggestions: Vec<SmartFolderSuggestion> = serde_json::from_value(result["suggestions"].clone())
        .unwrap_or_default();
    Ok(suggestions)
}

#[tauri::command]
fn accept_folders(except: Option<String>) -> Result<Vec<Folder>, String> {
    let mut args = vec!["folders", "accept"];
    let except_str;
    if let Some(ref e) = except {
        except_str = e.clone();
        args.push("--except");
        args.push(&except_str);
    }
    let result = cli(&args)?;
    let folders: Vec<Folder> = serde_json::from_value(result["created"].clone())
        .unwrap_or_default();
    Ok(folders)
}

#[tauri::command]
fn reset_folders(reason: String) -> Result<serde_json::Value, String> {
    let result = cli(&["folders", "reset", "--reason", &reason])?;
    Ok(result)
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
            analyze_articles,
            create_folder,
            delete_folder,
            move_feed,
            suggest_folders,
            accept_folders,
            reset_folders,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
