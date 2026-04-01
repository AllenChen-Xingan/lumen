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

        // Entities table
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

        // Track which articles have been analyzed
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN analyzed INTEGER NOT NULL DEFAULT 0;"
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

    // ── Entity methods ──

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
            "SELECT id, feed_id, title, url, content, summary, published_at, is_read, is_starred, fetched_at
             FROM articles WHERE analyzed = 0 AND (content IS NOT NULL OR summary IS NOT NULL)
             ORDER BY published_at DESC"
        )?;
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

    // ── Folder methods ──

    pub fn create_folder(&self, name: &str, folder_type: &str, query: Option<&str>) -> Result<i64, rusqlite::Error> {
        self.conn.execute(
            "INSERT INTO folders (name, folder_type, query) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, folder_type, query],
        )?;
        Ok(self.conn.last_insert_rowid())
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

    pub fn get_smart_folder_articles(&self, query: &str, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        let mut sql = String::from(
            "SELECT DISTINCT a.id, a.feed_id, a.title, a.url, a.content, a.summary, a.published_at, a.is_read, a.is_starred, a.fetched_at
             FROM articles a JOIN entities e ON a.id = e.article_id WHERE 1=1"
        );
        for part in query.split(" AND ") {
            let part = part.trim();
            if let Some(t) = part.strip_prefix("type:") {
                sql.push_str(&format!(" AND e.entity_type = '{}'", t.replace('\'', "''")));
            } else if let Some(n) = part.strip_prefix("name:") {
                let pattern = n.replace('*', "%").replace('\'', "''");
                sql.push_str(&format!(" AND e.name LIKE '{}'", pattern));
            } else {
                sql.push_str(&format!(" AND e.name LIKE '%{}%'", part.replace('\'', "''")));
            }
        }
        sql.push_str(&format!(" ORDER BY a.published_at DESC LIMIT {}", limit));

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

    /// Cluster entities into top N topic groups for smart folder suggestions.
    /// Uses co-occurrence: entities that appear together in articles form a cluster.
    /// Returns: Vec<(cluster_name, entity_names_csv, article_count, query_string)>
    pub fn suggest_smart_folders(&self, max_folders: usize) -> Result<Vec<(String, String, i64, String)>, rusqlite::Error> {
        // Step 1: Get top entities by frequency (concepts first, then orgs)
        let mut stmt = self.conn.prepare(
            "SELECT name, entity_type, COUNT(DISTINCT article_id) as article_cnt
             FROM entities
             GROUP BY name, entity_type
             HAVING article_cnt >= 2
             ORDER BY
                 CASE entity_type WHEN 'concept' THEN 0 WHEN 'organization' THEN 1 ELSE 2 END,
                 article_cnt DESC
             LIMIT 20"
        )?;
        let top_entities: Vec<(String, String, i64)> = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?.collect::<Result<Vec<_>, _>>()?;

        if top_entities.is_empty() {
            return Ok(vec![]);
        }

        // Step 2: Greedily pick top entities as cluster seeds, skipping overlapping ones
        let mut used_articles: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let mut suggestions = Vec::new();

        for (name, etype, count) in &top_entities {
            if suggestions.len() >= max_folders {
                break;
            }

            // Get articles this entity appears in
            let mut art_stmt = self.conn.prepare(
                "SELECT DISTINCT article_id FROM entities WHERE name = ?1"
            )?;
            let articles: Vec<i64> = art_stmt.query_map([name.as_str()], |row| {
                row.get(0)
            })?.collect::<Result<Vec<_>, _>>()?;

            // Skip if > 50% overlap with already-used articles
            let overlap = articles.iter().filter(|a| used_articles.contains(a)).count();
            if !articles.is_empty() && overlap as f64 / articles.len() as f64 > 0.5 {
                continue;
            }

            // Find co-occurring entities for this cluster
            let mut co_stmt = self.conn.prepare(
                "SELECT e2.name, COUNT(DISTINCT e2.article_id) as cnt
                 FROM entities e1
                 JOIN entities e2 ON e1.article_id = e2.article_id AND e1.name != e2.name
                 WHERE e1.name = ?1
                 GROUP BY e2.name
                 ORDER BY cnt DESC
                 LIMIT 3"
            )?;
            let co_entities: Vec<String> = co_stmt.query_map([name.as_str()], |row| {
                row.get::<_, String>(0)
            })?.collect::<Result<Vec<_>, _>>()?;

            // Build cluster
            let cluster_name = name.clone();
            let mut all_names = vec![name.clone()];
            all_names.extend(co_entities.iter().cloned());
            let names_csv = all_names.join(", ");

            // Build query string for the smart folder
            let query = if etype == "concept" {
                format!("type:concept AND name:{}*", name)
            } else {
                name.clone()
            };

            // Mark these articles as used
            for a in &articles {
                used_articles.insert(*a);
            }

            suggestions.push((cluster_name, names_csv, *count, query));
        }

        Ok(suggestions)
    }

    /// Accept suggested smart folders — create them in DB
    pub fn accept_suggested_folders(&self, suggestions: &[(String, String, i64, String)], except: &[usize]) -> Result<Vec<(i64, String)>, rusqlite::Error> {
        let mut created = Vec::new();
        for (i, (name, _names_csv, _count, query)) in suggestions.iter().enumerate() {
            if except.contains(&i) {
                continue;
            }
            let id = self.create_folder(name, "smart", Some(query))?;
            created.push((id, name.clone()));
        }
        Ok(created)
    }

    pub fn get_folder_feed_articles(&self, folder_id: i64, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT a.id, a.feed_id, a.title, a.url, a.content, a.summary, a.published_at, a.is_read, a.is_starred, a.fetched_at
             FROM articles a JOIN folder_feeds ff ON a.feed_id = ff.feed_id
             WHERE ff.folder_id = ?1 ORDER BY a.published_at DESC LIMIT {}", limit
        ))?;
        let articles = stmt.query_map([folder_id], |row| {
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
}
