use rusqlite::Connection;
use rss_core::{Feed, Article};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &str) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        let db = Database { conn };
        db.init()?;
        Ok(db)
    }

    fn init(&self) -> Result<(), rusqlite::Error> {
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS feeds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                url TEXT NOT NULL UNIQUE,
                site_url TEXT,
                description TEXT,
                added_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS articles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                feed_id INTEGER NOT NULL,
                title TEXT NOT NULL,
                url TEXT,
                content TEXT,
                summary TEXT,
                published_at TEXT,
                is_read INTEGER NOT NULL DEFAULT 0,
                is_starred INTEGER NOT NULL DEFAULT 0,
                fetched_at TEXT NOT NULL,
                FOREIGN KEY (feed_id) REFERENCES feeds(id) ON DELETE CASCADE
            );
        ")?;
        // Migration: add full_content column if missing
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN full_content TEXT;"
        );
        Ok(())
    }

    pub fn add_feed(&self, feed: &Feed) -> Result<i64, rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO feeds (title, url, site_url, description, added_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![feed.title, feed.url, feed.site_url, feed.description, feed.added_at.to_rfc3339()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_feeds(&self) -> Result<Vec<Feed>, rusqlite::Error> {
        let mut stmt = self.conn.prepare("SELECT id, title, url, site_url, description, added_at FROM feeds")?;
        let feeds = stmt.query_map([], |row| {
            Ok(Feed {
                id: row.get(0)?,
                title: row.get(1)?,
                url: row.get(2)?,
                site_url: row.get(3)?,
                description: row.get(4)?,
                added_at: {
                    let s: String = row.get(5)?;
                    chrono::DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .unwrap_or_else(|_| chrono::Utc::now())
                },
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(feeds)
    }

    pub fn remove_feed(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute("DELETE FROM feeds WHERE id = ?1", [id])?;
        Ok(changed > 0)
    }

    pub fn add_articles(&self, feed_id: i64, articles: &[Article]) -> Result<usize, rusqlite::Error> {
        let mut count = 0;
        for article in articles {
            let result = self.conn.execute(
                "INSERT OR IGNORE INTO articles (feed_id, title, url, content, summary, published_at, is_read, is_starred, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    feed_id,
                    article.title,
                    article.url,
                    article.content,
                    article.summary,
                    article.published_at.map(|dt| dt.to_rfc3339()),
                    article.is_read as i32,
                    article.is_starred as i32,
                    article.fetched_at.to_rfc3339(),
                ],
            );
            if let Ok(n) = result { count += n; }
        }
        Ok(count)
    }

    pub fn list_articles(&self, feed_id: Option<i64>, unread_only: bool) -> Result<Vec<Article>, rusqlite::Error> {
        let mut sql = String::from("SELECT id, feed_id, title, url, content, summary, published_at, is_read, is_starred, fetched_at FROM articles WHERE 1=1");
        if let Some(fid) = feed_id {
            sql.push_str(&format!(" AND feed_id = {}", fid));
        }
        if unread_only {
            sql.push_str(" AND is_read = 0");
        }
        sql.push_str(" ORDER BY published_at DESC");

        let mut stmt = self.conn.prepare(&sql)?;
        let articles = stmt.query_map([], |row| {
            let published_str: Option<String> = row.get(6)?;
            let fetched_str: String = row.get(9)?;
            Ok(Article {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                title: row.get(2)?,
                url: row.get(3)?,
                content: row.get(4)?,
                summary: row.get(5)?,
                published_at: published_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
                is_read: { let v: i32 = row.get(7)?; v != 0 },
                is_starred: { let v: i32 = row.get(8)?; v != 0 },
                fetched_at: chrono::DateTime::parse_from_rfc3339(&fetched_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                full_content: None,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    pub fn mark_read(&self, article_id: i64) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute("UPDATE articles SET is_read = 1 WHERE id = ?1", [article_id])?;
        Ok(changed > 0)
    }

    pub fn search_articles(&self, query: &str) -> Result<Vec<Article>, rusqlite::Error> {
        let pattern = format!("%{}%", query);
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, title, url, content, summary, published_at, is_read, is_starred, fetched_at
             FROM articles WHERE title LIKE ?1 OR content LIKE ?1 ORDER BY published_at DESC"
        )?;
        let articles = stmt.query_map(rusqlite::params![pattern], |row| {
            let published_str: Option<String> = row.get(6)?;
            let fetched_str: String = row.get(9)?;
            Ok(Article {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                title: row.get(2)?,
                url: row.get(3)?,
                content: row.get(4)?,
                summary: row.get(5)?,
                published_at: published_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
                is_read: { let v: i32 = row.get(7)?; v != 0 },
                is_starred: { let v: i32 = row.get(8)?; v != 0 },
                fetched_at: chrono::DateTime::parse_from_rfc3339(&fetched_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                full_content: None,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    pub fn toggle_star(&self, article_id: i64) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute("UPDATE articles SET is_starred = NOT is_starred WHERE id = ?1", [article_id])?;
        Ok(changed > 0)
    }

    pub fn get_full_content(&self, article_id: i64) -> Result<Option<String>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT full_content FROM articles WHERE id = ?1"
        )?;
        match stmt.query_row([article_id], |row| row.get::<_, Option<String>>(0)) {
            Ok(content) => Ok(content),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn set_full_content(&self, article_id: i64, full_content: &str) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute(
            "UPDATE articles SET full_content = ?1 WHERE id = ?2",
            rusqlite::params![full_content, article_id],
        )?;
        Ok(changed > 0)
    }

    pub fn get_article_url(&self, article_id: i64) -> Result<Option<String>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT url FROM articles WHERE id = ?1"
        )?;
        match stmt.query_row([article_id], |row| row.get::<_, Option<String>>(0)) {
            Ok(url) => Ok(url),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
