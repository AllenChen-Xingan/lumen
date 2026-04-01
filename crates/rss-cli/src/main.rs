use clap::{Parser, Subcommand};
use rss_store::Database;
use rss_core::parser::parse_feed;
use rss_fetch::fetch_feed_bytes;

#[derive(Parser)]
#[command(name = "rss", about = "Minimalist RSS reader")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a feed by URL
    Add { url: String },
    /// List all feeds
    List,
    /// Remove a feed by ID
    Remove { id: i64 },
    /// Fetch new articles from all feeds
    Fetch,
    /// List articles
    Articles {
        #[arg(long)]
        feed: Option<i64>,
        #[arg(long)]
        unread: bool,
    },
    /// Read an article
    Read { id: i64 },
    /// Mark article as read
    MarkRead { id: i64 },
    /// Star/unstar an article
    Star { id: i64 },
}

fn db_path() -> String {
    if let Ok(path) = std::env::var("RSS_DB_PATH") {
        return path;
    }
    let dir = dirs::data_local_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let app_dir = dir.join("rss-reader");
    std::fs::create_dir_all(&app_dir).ok();
    app_dir.join("feeds.db").to_string_lossy().to_string()
}

fn main() {
    let cli = Cli::parse();
    let db = Database::open(&db_path()).expect("Failed to open database");

    match cli.command {
        Commands::Add { url } => {
            match fetch_feed_bytes(&url) {
                Ok(bytes) => match parse_feed(&url, &bytes) {
                    Ok((feed, articles)) => {
                        match db.add_feed(&feed) {
                            Ok(feed_id) => {
                                let count = db.add_articles(feed_id, &articles).unwrap_or(0);
                                println!("Added feed: {} ({} articles)", feed.title, count);
                            }
                            Err(e) => eprintln!("Error adding feed: {}", e),
                        }
                    }
                    Err(e) => eprintln!("Error parsing feed: {}", e),
                },
                Err(e) => eprintln!("Error fetching feed: {}", e),
            }
        }
        Commands::List => {
            match db.list_feeds() {
                Ok(feeds) => {
                    if feeds.is_empty() {
                        println!("No feeds. Use 'add <url>' to add one.");
                    } else {
                        for f in feeds {
                            println!("[{}] {} - {}", f.id, f.title, f.url);
                        }
                    }
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::Remove { id } => {
            match db.remove_feed(id) {
                Ok(true) => println!("Feed {} removed.", id),
                Ok(false) => println!("Feed {} not found.", id),
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::Fetch => {
            match db.list_feeds() {
                Ok(feeds) => {
                    for feed in &feeds {
                        match fetch_feed_bytes(&feed.url) {
                            Ok(bytes) => match parse_feed(&feed.url, &bytes) {
                                Ok((_, articles)) => {
                                    let count = db.add_articles(feed.id, &articles).unwrap_or(0);
                                    println!("Fetched {}: {} new articles", feed.title, count);
                                }
                                Err(e) => eprintln!("Error parsing {}: {}", feed.title, e),
                            },
                            Err(e) => eprintln!("Error fetching {}: {}", feed.title, e),
                        }
                    }
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::Articles { feed, unread } => {
            match db.list_articles(feed, unread) {
                Ok(articles) => {
                    if articles.is_empty() {
                        println!("No articles found.");
                    } else {
                        for a in articles {
                            let status = if a.is_read { " " } else { "*" };
                            let star = if a.is_starred { "+" } else { " " };
                            println!("[{}]{}{} {}", a.id, status, star, a.title);
                        }
                    }
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::Read { id } => {
            match db.list_articles(None, false) {
                Ok(articles) => {
                    if let Some(article) = articles.into_iter().find(|a| a.id == id) {
                        println!("# {}\n", article.title);
                        if let Some(url) = &article.url {
                            println!("URL: {}\n", url);
                        }
                        if let Some(content) = &article.content {
                            println!("{}", content);
                        } else if let Some(summary) = &article.summary {
                            println!("{}", summary);
                        } else {
                            println!("(no content)");
                        }
                        db.mark_read(id).ok();
                    } else {
                        println!("Article {} not found.", id);
                    }
                }
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::MarkRead { id } => {
            match db.mark_read(id) {
                Ok(true) => println!("Marked article {} as read.", id),
                Ok(false) => println!("Article {} not found.", id),
                Err(e) => eprintln!("Error: {}", e),
            }
        }
        Commands::Star { id } => {
            match db.toggle_star(id) {
                Ok(true) => println!("Toggled star on article {}.", id),
                Ok(false) => println!("Article {} not found.", id),
                Err(e) => eprintln!("Error: {}", e),
            }
        }
    }
}
