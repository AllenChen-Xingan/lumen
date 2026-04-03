use rusqlite::Connection;
use rss_core::{Feed, Article};

#[derive(Debug, Clone)]
pub struct FeedHealth {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub last_fetch_at: Option<String>,
    pub last_error: Option<String>,
    pub fail_count: i64,
    pub avg_response_ms: Option<i64>,
}

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
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA cache_size = -64000;
            PRAGMA mmap_size = 268435456;
            PRAGMA temp_store = MEMORY;
            PRAGMA busy_timeout = 5000;
        ")?;
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

        // Migration: add folder_id to feeds for direct feed-to-folder assignment
        let _ = self.conn.execute_batch(
            "ALTER TABLE feeds ADD COLUMN folder_id INTEGER REFERENCES folders(id);"
        );
        // Cleanup: clear dangling folder_id references
        let _ = self.conn.execute_batch(
            "UPDATE feeds SET folder_id = NULL WHERE folder_id IS NOT NULL AND folder_id NOT IN (SELECT id FROM folders);"
        );

        // Track which articles have been analyzed
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN analyzed INTEGER NOT NULL DEFAULT 0;"
        );

        // Migration: add tags column for cognitive folder classification
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN tags TEXT NOT NULL DEFAULT '';"
        );

        // Migration: add tldr column for agent-friendly summaries
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN tldr TEXT;"
        );

        // Migration: add word_count for precise length-based filtering
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN word_count INTEGER NOT NULL DEFAULT 0;"
        );

        // Migration: add numeric feature columns for smart view queries
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN heading_count INTEGER NOT NULL DEFAULT 0;"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN external_link_count INTEGER NOT NULL DEFAULT 0;"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles ADD COLUMN blockquote_count INTEGER NOT NULL DEFAULT 0;"
        );

        // Indexes for smart view queries
        let _ = self.conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_articles_word_count ON articles(word_count);"
        );
        // Migration: drop obsolete code_block_count column (no longer used)
        let _ = self.conn.execute_batch(
            "DROP INDEX IF EXISTS idx_articles_code_block_count;"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE articles DROP COLUMN code_block_count;"
        );

        // Migration: feed health monitoring columns
        let _ = self.conn.execute_batch("ALTER TABLE feeds ADD COLUMN last_fetch_at TEXT;");
        let _ = self.conn.execute_batch("ALTER TABLE feeds ADD COLUMN last_error TEXT;");
        let _ = self.conn.execute_batch("ALTER TABLE feeds ADD COLUMN fail_count INTEGER NOT NULL DEFAULT 0;");
        let _ = self.conn.execute_batch("ALTER TABLE feeds ADD COLUMN avg_response_ms INTEGER;");

        // Migration: add ETag/Last-Modified cache headers to feeds for conditional fetch
        let _ = self.conn.execute_batch(
            "ALTER TABLE feeds ADD COLUMN etag TEXT;"
        );
        let _ = self.conn.execute_batch(
            "ALTER TABLE feeds ADD COLUMN last_modified_header TEXT;"
        );

        // Rejected suggestions (feedback loop)
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

        // FTS5 full-text search index (external content table backed by articles)
        self.conn.execute_batch("
            CREATE VIRTUAL TABLE IF NOT EXISTS articles_fts USING fts5(
                title, content, summary, tags,
                content='articles', content_rowid='id',
                tokenize='unicode61 remove_diacritics 2'
            );

            CREATE TRIGGER IF NOT EXISTS articles_fts_insert AFTER INSERT ON articles BEGIN
                INSERT INTO articles_fts(rowid, title, content, summary, tags)
                VALUES (new.id, new.title, new.content, new.summary, new.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS articles_fts_delete AFTER DELETE ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, content, summary, tags)
                VALUES ('delete', old.id, old.title, old.content, old.summary, old.tags);
            END;

            CREATE TRIGGER IF NOT EXISTS articles_fts_update AFTER UPDATE OF title, content, summary, tags ON articles BEGIN
                INSERT INTO articles_fts(articles_fts, rowid, title, content, summary, tags)
                VALUES ('delete', old.id, old.title, old.content, old.summary, old.tags);
                INSERT INTO articles_fts(rowid, title, content, summary, tags)
                VALUES (new.id, new.title, new.content, new.summary, new.tags);
            END;
        ")?;

        // Meta table for tracking migration versions
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _meta (key TEXT PRIMARY KEY, value TEXT);"
        )?;

        // Rebuild FTS index if not yet done (populates from existing articles)
        let fts_version: Option<String> = self.conn.query_row(
            "SELECT value FROM _meta WHERE key = 'fts_version'", [], |row| row.get(0)
        ).ok();
        if fts_version.as_deref() != Some("1") {
            self.conn.execute_batch("INSERT INTO articles_fts(articles_fts) VALUES('rebuild');")?;
            self.conn.execute("INSERT OR REPLACE INTO _meta (key, value) VALUES ('fts_version', '1')", [])?;
        }

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
        let mut stmt = self.conn.prepare("SELECT id, title, url, site_url, description, added_at, etag, last_modified_header FROM feeds")?;
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
                etag: row.get(6)?,
                last_modified_header: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(feeds)
    }

    /// Update ETag and Last-Modified cache headers for a feed after a successful fetch.
    pub fn update_feed_cache_headers(&self, feed_id: i64, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "UPDATE feeds SET etag = ?1, last_modified_header = ?2 WHERE id = ?3",
            rusqlite::params![etag, last_modified, feed_id],
        )?;
        Ok(())
    }

    pub fn remove_feed(&self, id: i64) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute("DELETE FROM feeds WHERE id = ?1", [id])?;
        Ok(changed > 0)
    }

    pub fn add_articles(&self, feed_id: i64, articles: &[Article]) -> Result<usize, rusqlite::Error> {
        let tx = self.conn.unchecked_transaction()?;
        let mut count = 0;
        for article in articles {
            let result = tx.execute(
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
        tx.commit()?;
        Ok(count)
    }

    /// Parse an Article from a row with columns:
    /// id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
    fn row_to_article(row: &rusqlite::Row) -> Result<Article, rusqlite::Error> {
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
            tldr: row.get(11)?,
            tags: row.get::<_, Option<String>>(12).ok().flatten(),
        })
    }

    pub fn set_article_tldr(&self, article_id: i64, tldr: &str) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute(
            "UPDATE articles SET tldr = ?1 WHERE id = ?2",
            rusqlite::params![tldr, article_id],
        )?;
        Ok(changed > 0)
    }

    /// Search articles with a time filter. `since` is a duration string like "24h", "7d", "30d".
    /// When `order_by_date` is false, results are ranked by BM25 relevance (title weighted 10x).
    pub fn search_articles_since(&self, query: &str, since: &str, limit: usize, feed_id: Option<i64>, order_by_date: bool) -> Result<Vec<Article>, rusqlite::Error> {
        let after = since_to_rfc3339(since);
        self.search_articles_timerange(query, Some(&after), None, limit, feed_id, order_by_date)
    }

    /// Search articles within a time range. `after`/`before` are RFC3339 or "YYYY-MM-DD" strings.
    /// Either can be None for an open-ended range.
    pub fn search_articles_timerange(&self, query: &str, after: Option<&str>, before: Option<&str>, limit: usize, feed_id: Option<i64>, order_by_date: bool) -> Result<Vec<Article>, rusqlite::Error> {
        // Empty query: fall back to time-filtered listing on regular table
        if query.trim().is_empty() {
            let mut sql = String::from(
                "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
                 FROM articles WHERE 1=1"
            );
            let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            let mut idx = 1;
            if let Some(a) = after {
                sql.push_str(&format!(" AND (published_at >= ?{} OR published_at IS NULL)", idx));
                params.push(Box::new(a.to_string()));
                idx += 1;
            }
            if let Some(b) = before {
                sql.push_str(&format!(" AND published_at <= ?{}", idx));
                params.push(Box::new(b.to_string()));
                idx += 1;
            }
            if feed_id.is_some() {
                sql.push_str(&format!(" AND feed_id = ?{}", idx));
                params.push(Box::new(feed_id.unwrap()));
                idx += 1;
            }
            let _ = idx;
            sql.push_str(&format!(" ORDER BY published_at DESC LIMIT {}", limit));
            let mut stmt = self.conn.prepare(&sql)?;
            let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
            let articles = stmt.query_map(param_refs.as_slice(), |row| {
                Self::row_to_article(row)
            })?.collect::<Result<Vec<_>, _>>()?;
            return Ok(articles);
        }

        let fts_query = prepare_fts_query(query);
        let order_clause = if order_by_date {
            "a.published_at DESC"
        } else {
            "bm25(articles_fts, 10.0, 1.0, 5.0, 2.0)"
        };
        let mut sql = format!(
            "SELECT a.id, a.feed_id, a.guid, a.title, a.url, a.content, a.summary, a.published_at, a.is_read, a.is_starred, a.fetched_at, a.tldr, a.full_content, a.tags
             FROM articles_fts fts
             JOIN articles a ON a.id = fts.rowid
             WHERE articles_fts MATCH ?1"
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        params.push(Box::new(fts_query));
        let mut idx = 2;
        if let Some(a) = after {
            sql.push_str(&format!(" AND (a.published_at >= ?{} OR a.published_at IS NULL)", idx));
            params.push(Box::new(a.to_string()));
            idx += 1;
        }
        if let Some(b) = before {
            sql.push_str(&format!(" AND a.published_at <= ?{}", idx));
            params.push(Box::new(b.to_string()));
            idx += 1;
        }
        if feed_id.is_some() {
            sql.push_str(&format!(" AND a.feed_id = ?{}", idx));
            params.push(Box::new(feed_id.unwrap()));
            idx += 1;
        }
        let _ = idx;
        sql.push_str(&format!(" ORDER BY {} LIMIT {}", order_clause, limit));
        let mut stmt = self.conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let articles = stmt.query_map(param_refs.as_slice(), |row| {
            Self::row_to_article(row)
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// Get feed statistics: (total_articles, unread_count, last_fetched)
    pub fn get_feed_stats(&self, feed_id: i64) -> Result<(i64, i64, Option<String>), rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT COUNT(*), SUM(CASE WHEN is_read = 0 THEN 1 ELSE 0 END), MAX(fetched_at)
             FROM articles WHERE feed_id = ?1"
        )?;
        stmt.query_row([feed_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1).unwrap_or(0),
                row.get::<_, Option<String>>(2)?,
            ))
        })
    }

    pub fn get_feed_folder_id(&self, feed_id: i64) -> Result<Option<i64>, rusqlite::Error> {
        self.conn.query_row(
            "SELECT folder_id FROM feeds WHERE id = ?1",
            [feed_id],
            |row| row.get(0),
        )
    }

    /// Get a single article by ID
    pub fn get_article(&self, article_id: i64) -> Result<Option<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
             FROM articles WHERE id = ?1"
        )?;
        match stmt.query_row([article_id], |row| Self::row_to_article(row)) {
            Ok(a) => Ok(Some(a)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Get the feed title for a given feed_id
    pub fn get_feed_title(&self, feed_id: i64) -> Result<Option<String>, rusqlite::Error> {
        let mut stmt = self.conn.prepare("SELECT title FROM feeds WHERE id = ?1")?;
        match stmt.query_row([feed_id], |row| row.get::<_, String>(0)) {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// List articles missing tldr
    pub fn list_articles_without_tldr(&self) -> Result<Vec<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
             FROM articles WHERE tldr IS NULL
             ORDER BY published_at DESC"
        )?;
        let articles = stmt.query_map([], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    pub fn list_articles(&self, feed_id: Option<i64>, unread_only: bool) -> Result<Vec<Article>, rusqlite::Error> {
        let mut sql = String::from("SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags FROM articles WHERE 1=1");
        if let Some(fid) = feed_id {
            sql.push_str(&format!(" AND feed_id = {}", fid));
        }
        if unread_only {
            sql.push_str(" AND is_read = 0");
        }
        sql.push_str(" ORDER BY published_at DESC");

        let mut stmt = self.conn.prepare(&sql)?;
        let articles = stmt.query_map([], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    pub fn mark_read(&self, article_id: i64) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute("UPDATE articles SET is_read = 1 WHERE id = ?1", [article_id])?;
        Ok(changed > 0)
    }

    /// Search articles using FTS5 full-text search.
    /// When `order_by_date` is false, results are ranked by BM25 relevance (title weighted 10x).
    pub fn search_articles(&self, query: &str, feed_id: Option<i64>, order_by_date: bool) -> Result<Vec<Article>, rusqlite::Error> {
        if query.trim().is_empty() {
            return self.list_articles(feed_id, false);
        }

        let fts_query = prepare_fts_query(query);
        let order_clause = if order_by_date {
            "a.published_at DESC"
        } else {
            "bm25(articles_fts, 10.0, 1.0, 5.0, 2.0)"
        };
        let mut sql = format!(
            "SELECT a.id, a.feed_id, a.guid, a.title, a.url, a.content, a.summary, a.published_at, a.is_read, a.is_starred, a.fetched_at, a.tldr, a.full_content, a.tags
             FROM articles_fts fts
             JOIN articles a ON a.id = fts.rowid
             WHERE articles_fts MATCH ?1"
        );
        if feed_id.is_some() {
            sql.push_str(" AND a.feed_id = ?2");
        }
        sql.push_str(&format!(" ORDER BY {}", order_clause));
        let mut stmt = self.conn.prepare(&sql)?;
        let articles = if let Some(fid) = feed_id {
            stmt.query_map(rusqlite::params![fts_query, fid], |row| {
                Self::row_to_article(row)
            })?.collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(rusqlite::params![fts_query], |row| {
                Self::row_to_article(row)
            })?.collect::<Result<Vec<_>, _>>()?
        };
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

    pub fn get_article_title(&self, article_id: i64) -> Result<Option<String>, rusqlite::Error> {
        let mut stmt = self.conn.prepare("SELECT title FROM articles WHERE id = ?1")?;
        match stmt.query_row([article_id], |row| row.get::<_, String>(0)) {
            Ok(t) => Ok(Some(t)),
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
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
             FROM articles WHERE analyzed = 0 AND (content IS NOT NULL OR summary IS NOT NULL)
             ORDER BY published_at DESC"
        )?;
        let articles = stmt.query_map([], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
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

    pub fn get_smart_folder_articles(&self, query: &str, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        let mut sql = String::from(
            "SELECT DISTINCT a.id, a.feed_id, a.guid, a.title, a.url, a.content, a.summary, a.published_at, a.is_read, a.is_starred, a.fetched_at, a.tldr, a.full_content, a.tags
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
        let articles = stmt.query_map([], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// Cluster entities into top N topic groups for smart folder suggestions.
    /// Uses co-occurrence: entities that appear together in articles form a cluster.
    /// Returns: Vec<(cluster_name, entity_names_csv, article_count, query_string)>
    pub fn suggest_smart_folders(&self, max_folders: usize) -> Result<Vec<(String, String, i64, String)>, rusqlite::Error> {
        // Layer 1: Get rejected entities to exclude
        let rejected = self.get_rejected_entities().unwrap_or_default();
        // Layer 2: Infer min entity length from rejection patterns
        let min_len = self.infer_min_entity_length().unwrap_or(2);

        // Step 1: Get top entities by frequency (concepts first, then orgs)
        let mut stmt = self.conn.prepare(
            "SELECT name, entity_type, COUNT(DISTINCT article_id) as article_cnt
             FROM entities
             GROUP BY name, entity_type
             HAVING article_cnt >= 2
             ORDER BY
                 CASE entity_type WHEN 'concept' THEN 0 WHEN 'organization' THEN 1 ELSE 2 END,
                 article_cnt DESC
             LIMIT 30"
        )?;
        let top_entities: Vec<(String, String, i64)> = stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?.collect::<Result<Vec<_>, _>>()?;

        if top_entities.is_empty() {
            return Ok(vec![]);
        }

        // Step 2: Greedily pick top entities as cluster seeds, skipping rejected/short/overlapping
        let mut used_articles: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let mut suggestions = Vec::new();

        for (name, etype, count) in &top_entities {
            if suggestions.len() >= max_folders {
                break;
            }

            // Layer 1: Skip rejected entities
            if rejected.iter().any(|r| r.eq_ignore_ascii_case(name)) {
                continue;
            }

            // Layer 2: Skip entities shorter than inferred minimum
            if name.len() < min_len {
                continue;
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

    /// Reset all smart folders: delete them, record reason, add old names to rejected list
    pub fn reset_smart_folders(&self, reason: &str) -> Result<usize, rusqlite::Error> {
        // Get current smart folder names to reject them
        let smart_names: Vec<String> = self.conn.prepare(
            "SELECT name FROM folders WHERE folder_type = 'smart'"
        )?.query_map([], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?;

        // Delete all smart folders
        let deleted = self.conn.execute(
            "DELETE FROM folders WHERE folder_type = 'smart'", []
        )?;

        // Record reason
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO reset_reasons (reason, reset_at) VALUES (?1, ?2)",
            rusqlite::params![reason, now],
        )?;

        // Add old folder names to rejected list so they don't resurface
        self.reject_entities(&smart_names)?;

        Ok(deleted)
    }

    /// Get all reset reasons (for LLM context)
    pub fn get_reset_reasons(&self) -> Result<Vec<(String, String)>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT reason, reset_at FROM reset_reasons ORDER BY reset_at DESC"
        )?;
        let results = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    /// Record rejected entity names so future suggestions skip them
    pub fn reject_entities(&self, names: &[String]) -> Result<usize, rusqlite::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut count = 0;
        for name in names {
            self.conn.execute(
                "INSERT INTO rejected_suggestions (entity_name, rejected_at) VALUES (?1, ?2)",
                rusqlite::params![name, now],
            )?;
            count += 1;
        }
        Ok(count)
    }

    /// Get all rejected entity names
    pub fn get_rejected_entities(&self) -> Result<Vec<String>, rusqlite::Error> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT entity_name FROM rejected_suggestions")?;
        let results = stmt.query_map([], |row| row.get(0))?.collect::<Result<Vec<_>, _>>()?;
        Ok(results)
    }

    /// Infer minimum entity name length from rejection patterns (Layer 2 heuristic)
    /// If user keeps rejecting short entities, raise the bar
    pub fn infer_min_entity_length(&self) -> Result<usize, rusqlite::Error> {
        let rejected = self.get_rejected_entities()?;
        if rejected.len() < 3 {
            return Ok(2); // default: at least 2 chars
        }
        let avg_len: f64 = rejected.iter().map(|s| s.len() as f64).sum::<f64>() / rejected.len() as f64;
        // If average rejected entity is short (≤5 chars), user prefers longer/more specific entities
        if avg_len <= 5.0 {
            Ok(6) // raise minimum to 6 chars
        } else {
            Ok(2)
        }
    }

    pub fn get_folder_feed_articles(&self, folder_id: i64, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT a.id, a.feed_id, a.guid, a.title, a.url, a.content, a.summary, a.published_at, a.is_read, a.is_starred, a.fetched_at, a.tldr, a.full_content, a.tags
             FROM articles a JOIN folder_feeds ff ON a.feed_id = ff.feed_id
             WHERE ff.folder_id = ?1 ORDER BY a.published_at DESC LIMIT {}", limit
        ))?;
        let articles = stmt.query_map([folder_id], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
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
            "SELECT id, title, url, site_url, description, added_at, etag, last_modified_header FROM feeds WHERE folder_id = ?1"
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
                etag: row.get(6)?,
                last_modified_header: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(feeds)
    }

    // ── Tag classification methods ──

    /// Set classification tags for an article (comma-separated, e.g. "新知,深度")
    pub fn set_article_tags(&self, article_id: i64, tags: &str) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute(
            "UPDATE articles SET tags = ?1 WHERE id = ?2",
            rusqlite::params![tags, article_id],
        )?;
        Ok(changed > 0)
    }

    /// Store all numeric feature columns alongside tags
    pub fn set_article_features(
        &self,
        article_id: i64,
        tags: &str,
        word_count: usize,
        heading_count: usize,
        external_link_count: usize,
        blockquote_count: usize,
    ) -> Result<bool, rusqlite::Error> {
        let changed = self.conn.execute(
            "UPDATE articles SET tags = ?1, word_count = ?2, heading_count = ?3, external_link_count = ?4, blockquote_count = ?5 WHERE id = ?6",
            rusqlite::params![tags, word_count as i64, heading_count as i64, external_link_count as i64, blockquote_count as i64, article_id],
        )?;
        Ok(changed > 0)
    }

    /// Get articles matching a specific tag, ordered by published_at desc
    pub fn get_articles_by_tag(&self, tag: &str, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        let pattern = format!("%{}%", tag);
        let sql = format!(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
             FROM articles WHERE tags LIKE ?1 ORDER BY published_at DESC LIMIT {}",
            limit
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let articles = stmt.query_map(rusqlite::params![pattern], |row| {
            Self::row_to_article(row)
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// Get long-form articles: word_count based, with CJK/English distinction
    /// Falls back to tag-based query for articles not yet re-annotated (word_count = 0)
    pub fn get_long_form_articles(&self, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        let sql = format!(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
             FROM articles WHERE (word_count >= 800 OR (word_count = 0 AND tags LIKE '%long%'))
             ORDER BY published_at DESC LIMIT {}",
            limit
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let articles = stmt.query_map([], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// Count long-form articles
    pub fn count_long_form_articles(&self) -> Result<i64, rusqlite::Error> {
        self.conn.query_row(
            "SELECT COUNT(*) FROM articles WHERE (word_count >= 800 OR (word_count = 0 AND tags LIKE '%long%'))",
            [],
            |row| row.get(0),
        )
    }


    /// Get articles that haven't been classified yet (alias: list_untagged_articles)
    pub fn get_unclassified_articles(&self) -> Result<Vec<Article>, rusqlite::Error> {
        self.list_untagged_articles()
    }

    /// List articles without tags
    pub fn list_untagged_articles(&self) -> Result<Vec<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
             FROM articles WHERE tags = '' AND (content IS NOT NULL OR summary IS NOT NULL)
             ORDER BY published_at DESC"
        )?;
        let articles = stmt.query_map([], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// List untagged articles with LIMIT/OFFSET for chunked processing
    pub fn list_untagged_articles_chunk(&self, offset: usize, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
             FROM articles WHERE tags = '' AND (content IS NOT NULL OR summary IS NOT NULL)
             ORDER BY published_at DESC LIMIT ?1 OFFSET ?2"
        )?;
        let articles = stmt.query_map(rusqlite::params![limit as i64, offset as i64], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// List all articles with LIMIT/OFFSET for chunked processing
    pub fn list_articles_chunk(&self, offset: usize, limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
             FROM articles ORDER BY published_at DESC LIMIT ?1 OFFSET ?2"
        )?;
        let articles = stmt.query_map(rusqlite::params![limit as i64, offset as i64], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// Batch update article features within a single transaction
    pub fn batch_set_article_features(
        &self,
        updates: &[(i64, String, usize, usize, usize, usize)],
    ) -> Result<usize, rusqlite::Error> {
        let tx = self.conn.unchecked_transaction()?;
        let mut count = 0;
        for (article_id, tags, word_count, heading_count, external_link_count, blockquote_count) in updates {
            let changed = tx.execute(
                "UPDATE articles SET tags = ?1, word_count = ?2, heading_count = ?3, external_link_count = ?4, blockquote_count = ?5 WHERE id = ?6",
                rusqlite::params![tags, *word_count as i64, *heading_count as i64, *external_link_count as i64, *blockquote_count as i64, article_id],
            )?;
            count += changed;
        }
        tx.commit()?;
        Ok(count)
    }

    /// Clear all article tags (used during reset/re-classify)
    pub fn clear_all_tags(&self) -> Result<usize, rusqlite::Error> {
        let changed = self.conn.execute("UPDATE articles SET tags = ''", [])?;
        Ok(changed)
    }

    /// Count articles matching a specific tag
    pub fn count_articles_by_tag(&self, tag: &str) -> Result<i64, rusqlite::Error> {
        let pattern = format!("%{}%", tag);
        self.conn.query_row(
            "SELECT COUNT(*) FROM articles WHERE tags LIKE ?1",
            rusqlite::params![pattern],
            |row| row.get(0),
        )
    }

    /// Count articles matching any of the given tags
    pub fn count_articles_with_any_tag(&self, tags: &[&str]) -> Result<i64, rusqlite::Error> {
        if tags.is_empty() {
            return Ok(0);
        }
        let conditions: Vec<String> = tags.iter()
            .map(|t| format!("tags LIKE '%{}%'", t))
            .collect();
        let sql = format!("SELECT COUNT(*) FROM articles WHERE {}", conditions.join(" OR "));
        self.conn.query_row(&sql, [], |row| row.get(0))
    }

    /// Get articles matching any of the given tags
    pub fn get_articles_with_any_tag(&self, tags: &[&str], limit: usize) -> Result<Vec<Article>, rusqlite::Error> {
        if tags.is_empty() {
            return Ok(vec![]);
        }
        let conditions: Vec<String> = tags.iter()
            .map(|t| format!("tags LIKE '%{}%'", t))
            .collect();
        let sql = format!(
            "SELECT id, feed_id, guid, title, url, content, summary, published_at, is_read, is_starred, fetched_at, tldr, full_content, tags
             FROM articles WHERE {} ORDER BY published_at DESC LIMIT {}",
            conditions.join(" OR "), limit
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let articles = stmt.query_map([], |row| Self::row_to_article(row))?.collect::<Result<Vec<_>, _>>()?;
        Ok(articles)
    }

    /// Count articles for each fact-based tag
    pub fn count_all_tags(&self) -> Result<Vec<(String, i64)>, rusqlite::Error> {
        let mut results = Vec::new();
        for tag in &["short", "medium", "long", "has_images", "structured", "has_references", "link_rich"] {
            let count = self.count_articles_by_tag(tag)?;
            if count > 0 {
                results.push((tag.to_string(), count));
            }
        }
        // Unannotated count
        let unannotated: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM articles WHERE tags = '' OR tags IS NULL",
            [],
            |row| row.get(0),
        )?;
        if unannotated > 0 {
            results.push(("unannotated".to_string(), unannotated));
        }
        Ok(results)
    }

    /// Per-tag engagement stats for harness tuning diagnostics
    /// Returns: Vec<(tag, total, read, starred, deep_read)>
    pub fn tag_engagement_stats(&self) -> Result<Vec<(String, i64, i64, i64, i64)>, rusqlite::Error> {
        let mut results = Vec::new();
        for tag in &["short", "medium", "long", "has_images", "structured", "has_references", "link_rich"] {
            let pattern = format!("%{}%", tag);
            let row: (i64, i64, i64, i64) = self.conn.query_row(
                "SELECT COUNT(*), SUM(is_read), SUM(is_starred),
                        SUM(CASE WHEN full_content IS NOT NULL THEN 1 ELSE 0 END)
                 FROM articles WHERE tags LIKE ?1 AND published_at > datetime('now', '-30 days')",
                rusqlite::params![pattern],
                |row| Ok((row.get(0)?, row.get::<_, i64>(1).unwrap_or(0),
                           row.get::<_, i64>(2).unwrap_or(0), row.get::<_, i64>(3).unwrap_or(0))),
            )?;
            results.push((tag.to_string(), row.0, row.1, row.2, row.3));
        }
        // Unclassified engagement
        let row: (i64, i64, i64, i64) = self.conn.query_row(
            "SELECT COUNT(*), SUM(is_read), SUM(is_starred),
                    SUM(CASE WHEN full_content IS NOT NULL THEN 1 ELSE 0 END)
             FROM articles WHERE tags = '' AND published_at > datetime('now', '-30 days')
             AND (content IS NOT NULL OR summary IS NOT NULL)",
            [],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1).unwrap_or(0),
                       row.get::<_, i64>(2).unwrap_or(0), row.get::<_, i64>(3).unwrap_or(0))),
        )?;
        results.push(("未分类".to_string(), row.0, row.1, row.2, row.3));
        Ok(results)
    }

    /// List feeds not in any folder
    pub fn list_uncategorized_feeds(&self) -> Result<Vec<Feed>, rusqlite::Error> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, url, site_url, description, added_at, etag, last_modified_header FROM feeds WHERE folder_id IS NULL"
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
                etag: row.get(6)?,
                last_modified_header: row.get(7)?,
            })
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(feeds)
    }

    // ── Feed health methods ──

    /// Record a successful fetch: reset errors, update response time
    pub fn record_fetch_success(&self, feed_id: i64, response_ms: i64) -> Result<bool, rusqlite::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let changed = self.conn.execute(
            "UPDATE feeds SET last_fetch_at = ?1, last_error = NULL, fail_count = 0,
             avg_response_ms = COALESCE((avg_response_ms + ?2) / 2, ?2)
             WHERE id = ?3",
            rusqlite::params![now, response_ms, feed_id],
        )?;
        Ok(changed > 0)
    }

    /// Record a failed fetch: increment fail_count, store error
    pub fn record_fetch_failure(&self, feed_id: i64, error_message: &str) -> Result<bool, rusqlite::Error> {
        let now = chrono::Utc::now().to_rfc3339();
        let changed = self.conn.execute(
            "UPDATE feeds SET last_fetch_at = ?1, last_error = ?2, fail_count = fail_count + 1
             WHERE id = ?3",
            rusqlite::params![now, error_message, feed_id],
        )?;
        Ok(changed > 0)
    }

    /// Get feed health data for all feeds or a specific one
    pub fn get_feed_health(&self, feed_id: Option<i64>) -> Result<Vec<FeedHealth>, rusqlite::Error> {
        let mut sql = String::from(
            "SELECT id, title, url, last_fetch_at, last_error, fail_count, avg_response_ms FROM feeds"
        );
        if feed_id.is_some() {
            sql.push_str(" WHERE id = ?1");
        }
        sql.push_str(" ORDER BY fail_count DESC, title ASC");
        let mut stmt = self.conn.prepare(&sql)?;
        let results = if let Some(fid) = feed_id {
            stmt.query_map(rusqlite::params![fid], |row| {
                Ok(FeedHealth {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    url: row.get(2)?,
                    last_fetch_at: row.get(3)?,
                    last_error: row.get(4)?,
                    fail_count: row.get(5)?,
                    avg_response_ms: row.get(6)?,
                })
            })?.collect::<Result<Vec<_>, _>>()?
        } else {
            stmt.query_map([], |row| {
                Ok(FeedHealth {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    url: row.get(2)?,
                    last_fetch_at: row.get(3)?,
                    last_error: row.get(4)?,
                    fail_count: row.get(5)?,
                    avg_response_ms: row.get(6)?,
                })
            })?.collect::<Result<Vec<_>, _>>()?
        };
        Ok(results)
    }
}

/// Prepare a raw query string for FTS5 MATCH.
/// If the query contains FTS5 operators (AND, OR, NOT, NEAR, quotes, wildcards),
/// pass it through as-is for advanced users. Otherwise, wrap each word as a
/// quoted prefix search: `"word"*` joined by implicit AND.
fn prepare_fts_query(raw: &str) -> String {
    let has_operators = raw.contains(" AND ")
        || raw.contains(" OR ")
        || raw.contains(" NOT ")
        || raw.contains(" NEAR")
        || raw.contains('"')
        || raw.contains('*');
    if has_operators {
        raw.to_string()
    } else {
        raw.split_whitespace()
            .map(|w| format!("\"{}\"*", w.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Convert relative duration ("24h", "7d", "30d") to RFC3339 cutoff timestamp.
fn since_to_rfc3339(since: &str) -> String {
    let hours: i64 = if since.ends_with('d') {
        since.trim_end_matches('d').parse::<i64>().unwrap_or(1) * 24
    } else if since.ends_with('h') {
        since.trim_end_matches('h').parse::<i64>().unwrap_or(24)
    } else {
        24 * 365
    };
    (chrono::Utc::now() - chrono::Duration::hours(hours)).to_rfc3339()
}
