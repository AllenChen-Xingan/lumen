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
        self.conn.execute_batch("PRAGMA foreign_keys = ON;")?;
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
        // Migration: add guid column + unique constraint for dedup
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN guid TEXT NOT NULL DEFAULT '';"
        );
        // Backfill: set guid from url (or id) for existing rows that have empty guid
        let _ = self.conn.execute_batch(
            "UPDATE articles SET guid = COALESCE(NULLIF(url, ''), 'id:' || id) WHERE guid = '';"
        );
        // Dedup: keep the oldest article (lowest id) for each (feed_id, guid) pair
        let _ = self.conn.execute_batch(
            "DELETE FROM articles WHERE id NOT IN (
                SELECT MIN(id) FROM articles GROUP BY feed_id, guid
            );"
        );
        // Dedup by URL: same feed + same url = same article (handles guid changes on republish)
        let _ = self.conn.execute_batch(
            "DELETE FROM articles WHERE url IS NOT NULL AND id NOT IN (
                SELECT MIN(id) FROM articles WHERE url IS NOT NULL GROUP BY feed_id, url
            );"
        );
        // Now safe to create unique index
        let _ = self.conn.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_articles_feed_guid ON articles(feed_id, guid);"
        );

        // Entities table (kept for backward compatibility, no longer used for folders)
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS entities (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                entity_type TEXT NOT NULL,
                article_id INTEGER NOT NULL,
                context TEXT,
                score REAL,
                FOREIGN KEY (article_id) REFERENCES articles(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_entities_name ON entities(name);
            CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);
            CREATE INDEX IF NOT EXISTS idx_entities_article ON entities(article_id);
        ")?;

        // Folders table
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS folders (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                folder_type TEXT NOT NULL DEFAULT 'manual',
                query TEXT
            );
            CREATE TABLE IF NOT EXISTS folder_feeds (
                folder_id INTEGER NOT NULL,
                feed_id INTEGER NOT NULL,
                PRIMARY KEY (folder_id, feed_id),
                FOREIGN KEY (folder_id) REFERENCES folders(id) ON DELETE CASCADE,
                FOREIGN KEY (feed_id) REFERENCES feeds(id) ON DELETE CASCADE
            );
        ")?;

        // Migration: add folder_id to feeds for direct feed-to-folder assignment
        let _ = self.conn.execute_batch(
            "ALTER TABLE feeds ADD COLUMN folder_id INTEGER REFERENCES folders(id);"
        );

        // Track which articles have been analyzed
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN analyzed INTEGER NOT NULL DEFAULT 0;"
        );

        // Migration: add tags column for cognitive folder classification
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN tags TEXT;"
        );

        // Legacy tables (kept, not dropped)
        self.conn.execute_batch("
            CREATE TABLE IF NOT EXISTS rejected_suggestions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                entity_name TEXT NOT NULL,
                rejected_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS reset_reasons (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                reason TEXT NOT NULL,
                reset_at TEXT NOT NULL
            );
        ")?;

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
                "INSERT OR IGNORE INTO articles (feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                rusqlite::params![
                    feed_id,
                    article.guid,
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
        let mut sql = String::from("SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at FROM articles WHERE 1=1");
        if let Some(fid) = feed_id {
            sql.push_str(&format!(" AND feed_id = {}", fid));
        }
        if unread_only {
            sql.push_str(" AND is_read = 0");
        }
        sql.push_str(" ORDER BY published_at DESC");

        let mut stmt = self.conn.prepare(&sql)?;
        let articles = stmt.query_map([], |row| {
            let published_str: Option<String> = row.get(7)?;
            let fetched_str: String = row.get(10)?;
            Ok(Article {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                guid: row.get(2)?,
                title: row.get(3)?,
                url: row.get(4)?,
                content: row.get(5)?,
                summary: row.get(6)?,
                published_at: published_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
                is_read: { let v: i32 = row.get(8)?; v != 0 },
                is_starred: { let v: i32 = row.get(9)?; v != 0 },
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
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at
             FROM articles WHERE title LIKE ?1 OR content LIKE ?1 ORDER BY published_at DESC"
        )?;
        let articles = stmt.query_map(rusqlite::params![pattern], |row| {
            let published_str: Option<String> = row.get(7)?;
            let fetched_str: String = row.get(10)?;
            Ok(Article {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                guid: row.get(2)?,
                title: row.get(3)?,
                url: row.get(4)?,
                content: row.get(5)?,
                summary: row.get(6)?,
                published_at: published_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
                is_read: { let v: i32 = row.get(8)?; v != 0 },
                is_starred: { let v: i32 = row.get(9)?; v != 0 },
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

    // ── Entity methods (kept for backward compat) ──

    pub fn add_entities(&self, article_id: i64, entities: &[(String, String, Option<String>, f32)]) -> Result<usize, rusqlite::Error> {
        let mut count = 0;
        for (name, entity_type, context, score) in entities {
            self.conn.execute(
                "INSERT INTO entities (name, entity_type, article_id, context, score) VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![name, entity_type, article_id, context, *score as f64],
            )?;
            count += 1;
        }
        Ok(count)
    }

    pub fn mark_analyzed(&self, article_id: i64) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute("UPDATE articles SET analyzed = 1 WHERE id = ?1", [article_id])?;
        Ok(changed > 0)
    }

    pub fn list_unanalyzed_articles(&self) -> Result<Vec<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at
             FROM articles WHERE analyzed = 0 AND (content IS NOT NULL OR summary IS NOT NULL)
             ORDER BY published_at DESC"
        )?;
        let articles = stmt.query_map([], |row| {
            let published_str: Option<String> = row.get(7)?;
            let fetched_str: String = row.get(10)?;
            Ok(Article {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                guid: row.get(2)?,
                title: row.get(3)?,
                url: row.get(4)?,
                content: row.get(5)?,
                summary: row.get(6)?,
                published_at: published_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
                is_read: { let v: i32 = row.get(8)?; v != 0 },
                is_starred: { let v: i32 = row.get(9)?; v != 0 },
                fetched_at: chrono::DateTime::parse_from_rfc3339(&fetched_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                full_content: None,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    pub fn list_entities_grouped(&self, entity_type: Option<&str>, name_filter: Option<&str>, limit: usize) -> Result<Vec<(String, String, i64, f64)>, rusqlite::Error> {
        let mut sql = String::from(
            "SELECT name, entity_type, COUNT(*) as cnt, AVG(score) as avg_score FROM entities WHERE 1=1"
        );
        if let Some(t) = entity_type {
            sql.push_str(&format!(" AND entity_type = '{}'", t.replace('\'', "''")));
        }
        if let Some(n) = name_filter {
            sql.push_str(&format!(" AND name LIKE '%{}%'", n.replace('\'', "''")));
        }
        sql.push_str(&format!(" GROUP BY name, entity_type ORDER BY cnt DESC LIMIT {}", limit));

        let mut stmt = self.conn.prepare(&sql)?;
        let results = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    /// Get entity names for a specific article
    pub fn get_article_entities(&self, article_id: i64) -> Result<Vec<String>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT name FROM entities WHERE article_id = ?1 ORDER BY score DESC"
        )?;
        let results = stmt.query_map([article_id], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    pub fn get_entity_mentions(&self, name: &str) -> Result<Vec<(i64, String, Option<String>, f64)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT article_id, entity_type, context, score FROM entities WHERE name = ?1 ORDER BY score DESC"
        )?;
        let results = stmt.query_map([name], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    pub fn get_related_entities(&self, name: &str, limit: usize) -> Result<Vec<(String, String, i64)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT e2.name, e2.entity_type, COUNT(*) as cnt
             FROM entities e1 JOIN entities e2 ON e1.article_id = e2.article_id AND e1.id != e2.id
             WHERE e1.name = ?1 GROUP BY e2.name, e2.entity_type ORDER BY cnt DESC LIMIT {}",
            limit
        ))?;
        let results = stmt.query_map([name], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    // ── Tag methods (new cognitive folder system) ──

    /// Set tags for an article (comma-separated)
    pub fn set_article_tags(&self, article_id: i64, tags: &[String]) -> Result<bool, rusqlite::Error> {
        let tags_str = tags.join(",");
        let changed = self.conn.execute(
            "UPDATE articles SET tags = ?1, analyzed = 1 WHERE id = ?2",
            rusqlite::params![tags_str, article_id],
        )?;
        Ok(changed > 0)
    }

    /// Get articles matching a tag (LIKE query on comma-separated tags)
    pub fn get_articles_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        // Match tag exactly: either the whole field, at start, at end, or in middle
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at
             FROM articles
             WHERE tags = ?1
                OR tags LIKE ?2
                OR tags LIKE ?3
                OR tags LIKE ?4
             ORDER BY published_at DESC LIMIT {}",
            limit
        ))?;
        let exact = tag.to_string();
        let starts = format!("{},%", tag);
        let ends = format!("%,{}", tag);
        let middle = format!("%,{},%", tag);
        let articles = stmt.query_map(rusqlite::params![exact, starts, ends, middle], |row| {
            let published_str: Option<String> = row.get(7)?;
            let fetched_str: String = row.get(10)?;
            Ok(Article {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                guid: row.get(2)?,
                title: row.get(3)?,
                url: row.get(4)?,
                content: row.get(5)?,
                summary: row.get(6)?,
                published_at: published_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
                is_read: { let v: i32 = row.get(8)?; v != 0 },
                is_starred: { let v: i32 = row.get(9)?; v != 0 },
                fetched_at: chrono::DateTime::parse_from_rfc3339(&fetched_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                full_content: None,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// Count articles for a given tag
    pub fn count_articles_by_tag(&self, tag: &str) -> Result<i64, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*) FROM articles
             WHERE tags = ?1
                OR tags LIKE ?2
                OR tags LIKE ?3
                OR tags LIKE ?4"
        )?;
        let exact = tag.to_string();
        let starts = format!("{},%", tag);
        let ends = format!("%,{}", tag);
        let middle = format!("%,{},%", tag);
        stmt.query_row(rusqlite::params![exact, starts, ends, middle], |row| row.get(0))
    }

    /// Clear all tags (for reset + re-classify)
    pub fn clear_all_tags(&self) -> Result<usize, rusqlite::Error> {
        let changed = self.conn.execute(
            "UPDATE articles SET tags = NULL, analyzed = 0 WHERE tags IS NOT NULL", []
        )?;
        Ok(changed)
    }

    /// List articles that have no tags yet (for classification)
    pub fn list_untagged_articles(&self) -> Result<Vec<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at
             FROM articles WHERE (tags IS NULL OR tags = '') AND (content IS NOT NULL OR summary IS NOT NULL)
             ORDER BY published_at DESC"
        )?;
        let articles = stmt.query_map([], |row| {
            let published_str: Option<String> = row.get(7)?;
            let fetched_str: String = row.get(10)?;
            Ok(Article {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                guid: row.get(2)?,
                title: row.get(3)?,
                url: row.get(4)?,
                content: row.get(5)?,
                summary: row.get(6)?,
                published_at: published_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
                is_read: { let v: i32 = row.get(8)?; v != 0 },
                is_starred: { let v: i32 = row.get(9)?; v != 0 },
                fetched_at: chrono::DateTime::parse_from_rfc3339(&fetched_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                full_content: None,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    // ── Folder methods ──

    pub fn create_folder(&self, name: &str, folder_type: &str, query: Option<&str>) -> Result<i64, rusqlite::Error> {
        // Enforce max 4 manual folders constraint
        if folder_type == "manual" {
            let count = self.count_manual_folders()?;
            if count >= 4 {
                return Err(rusqlite::Error::InvalidParameterName(
                    "Maximum of 4 manual folders allowed".to_string(),
                ));
            }
        }
        self.conn.execute(
            "INSERT INTO folders (name, folder_type, query) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, folder_type, query],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Count manual folders
    pub fn count_manual_folders(&self) -> Result<i64, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*) FROM folders WHERE folder_type = 'manual'"
        )?;
        stmt.query_row([], |row| row.get(0))
    }

    pub fn list_folders(&self) -> Result<Vec<(i64, String, String, Option<String>)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare("SELECT id, name, folder_type, query FROM folders ORDER BY name")?;
        let results = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    pub fn remove_folder(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute("DELETE FROM folders WHERE id = ?1", [id])?;
        Ok(changed > 0)
    }

    pub fn add_feed_to_folder(&self, folder_id: i64, feed_id: i64) -> Result<bool, rusqlite::Error> {
        let result = self.conn.execute(
            "INSERT OR IGNORE INTO folder_feeds (folder_id, feed_id) VALUES (?1, ?2)",
            rusqlite::params![folder_id, feed_id],
        );
        Ok(result.map(|n| n > 0).unwrap_or(false))
    }

    pub fn get_folder_feed_articles(&self, folder_id: i64, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT a.id, a.feed_id, a.guid, a.title, a.url, a.content, a.summary, a.published_at, a.is_read, a.is_starred, a.fetched_at
             FROM articles a JOIN folder_feeds ff ON a.feed_id = ff.feed_id
             WHERE ff.folder_id = ?1 ORDER BY a.published_at DESC LIMIT {}", limit
        ))?;
        let articles = stmt.query_map([folder_id], |row| {
            let published_str: Option<String> = row.get(7)?;
            let fetched_str: String = row.get(10)?;
            Ok(Article {
                id: row.get(0)?,
                feed_id: row.get(1)?,
                guid: row.get(2)?,
                title: row.get(3)?,
                url: row.get(4)?,
                content: row.get(5)?,
                summary: row.get(6)?,
                published_at: published_str.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok().map(|dt| dt.with_timezone(&chrono::Utc))),
                is_read: { let v: i32 = row.get(8)?; v != 0 },
                is_starred: { let v: i32 = row.get(9)?; v != 0 },
                fetched_at: chrono::DateTime::parse_from_rfc3339(&fetched_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
                full_content: None,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// Move a feed into a folder (or remove from folder if folder_id is None)
    pub fn move_feed_to_folder(&self, feed_id: i64, folder_id: Option<i64>) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute(
            "UPDATE feeds SET folder_id = ?1 WHERE id = ?2",
            rusqlite::params![folder_id, feed_id],
        )?;
        Ok(changed > 0)
    }

    /// List feeds in a specific folder
    pub fn list_feeds_in_folder(&self, folder_id: i64) -> Result<Vec<Feed>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, url, site_url, description, added_at FROM feeds WHERE folder_id = ?1"
        )?;
        let feeds = stmt.query_map(rusqlite::params![folder_id], |row| {
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

    /// List feeds not in any folder
    pub fn list_uncategorized_feeds(&self) -> Result<Vec<Feed>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, url, site_url, description, added_at FROM feeds WHERE folder_id IS NULL"
        )?;
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
}
