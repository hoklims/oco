use std::sync::Mutex;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::error::{Result, RetrievalError};

/// A single full-text search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtsResult {
    pub id: String,
    pub path: String,
    pub snippet: String,
    pub rank: f64,
}

/// Full-text search index backed by SQLite FTS5.
///
/// Wraps the connection in a `Mutex` so that `FtsIndex` is `Send + Sync`,
/// which allows the orchestration loop to run on a spawned tokio task.
pub struct FtsIndex {
    conn: Mutex<Connection>,
}

impl FtsIndex {
    /// Open (or create) an FTS5-backed SQLite database at `db_path`.
    ///
    /// Pass `":memory:"` for a purely in-memory index.
    #[instrument(skip_all, fields(db_path))]
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = if db_path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(db_path)?
        };

        // Enable WAL for better concurrent-read performance.
        conn.pragma_update(None, "journal_mode", "WAL")?;

        // Create the FTS5 virtual table if it does not exist.
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS documents USING fts5(
                id UNINDEXED,
                path UNINDEXED,
                content,
                tokenize = 'porter unicode61'
            );",
        )?;

        debug!("FTS5 index ready at {db_path}");
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Index a single document. If a document with the same `id` already exists
    /// it is deleted first (upsert semantics).
    #[instrument(skip(self, content), fields(id, path))]
    pub fn index_document(&self, id: &str, path: &str, content: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("FTS index lock poisoned".into()))?;
        conn.execute("DELETE FROM documents WHERE id = ?1", [id])?;
        conn.execute(
            "INSERT INTO documents (id, path, content) VALUES (?1, ?2, ?3)",
            [id, path, content],
        )?;
        debug!("indexed document {id}");
        Ok(())
    }

    /// Batch-index multiple documents inside a single transaction.
    ///
    /// Each tuple is `(id, path, content)`.
    #[instrument(skip_all, fields(count = docs.len()))]
    pub fn index_documents_batch(&self, docs: Vec<(String, String, String)>) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("FTS index lock poisoned".into()))?;
        let tx = conn.unchecked_transaction()?;
        {
            let mut delete_stmt = tx.prepare("DELETE FROM documents WHERE id = ?1")?;
            let mut insert_stmt =
                tx.prepare("INSERT INTO documents (id, path, content) VALUES (?1, ?2, ?3)")?;
            for (id, path, content) in &docs {
                delete_stmt.execute([id.as_str()])?;
                insert_stmt.execute([id.as_str(), path.as_str(), content.as_str()])?;
            }
        }
        tx.commit()?;
        debug!("batch-indexed {} documents", docs.len());
        Ok(())
    }

    /// Run a full-text search query against the index.
    ///
    /// Results are ordered by FTS5 rank (lower is better — we negate so that
    /// callers see higher = better).
    #[instrument(skip(self), fields(query, limit))]
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<FtsResult>> {
        // Sanitize query: remove FTS5 special characters to prevent syntax errors.
        let sanitized: String = query
            .chars()
            .map(|c| match c {
                '"' | '\'' | '*' | '+' | '-' | '(' | ')' | '{' | '}' | '[' | ']' | '^' | '~'
                | '?' | ':' | '\\' => ' ',
                _ => c,
            })
            .collect();
        let sanitized = sanitized.trim();
        if sanitized.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("FTS index lock poisoned".into()))?;
        let mut stmt = conn
            .prepare(
                "SELECT id, path, snippet(documents, 2, '<b>', '</b>', '...', 48), rank
             FROM documents
             WHERE documents MATCH ?1
             ORDER BY rank
             LIMIT ?2",
            )
            .map_err(|e| RetrievalError::SearchError(e.to_string()))?;

        let rows = stmt
            .query_map(rusqlite::params![sanitized, limit], |row| {
                Ok(FtsResult {
                    id: row.get(0)?,
                    path: row.get(1)?,
                    snippet: row.get(2)?,
                    // FTS5 rank is negative (lower = more relevant).
                    // Negate so that higher = better for consumers.
                    rank: -row.get::<_, f64>(3)?,
                })
            })
            .map_err(|e| RetrievalError::SearchError(e.to_string()))?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row.map_err(|e| RetrievalError::SearchError(e.to_string()))?);
        }

        debug!("FTS query returned {} results", results.len());
        Ok(results)
    }

    // ── Q3: Incremental indexing ─────────────────────────

    /// Ensure the metadata table exists for tracking indexed file timestamps.
    fn ensure_meta_table(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS index_meta (
                path TEXT PRIMARY KEY,
                mtime_secs INTEGER NOT NULL,
                indexed_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    /// Record that a file was indexed at a given modification time.
    fn record_indexed(conn: &Connection, path: &str, mtime_secs: i64) -> Result<()> {
        conn.execute(
            "INSERT OR REPLACE INTO index_meta (path, mtime_secs, indexed_at)
             VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![path, mtime_secs],
        )?;
        Ok(())
    }

    /// Check which files in the workspace need re-indexing.
    ///
    /// Returns `(to_index, to_remove)`:
    /// - `to_index`: files that are new or modified since last index
    /// - `to_remove`: paths that were indexed but no longer exist on disk
    pub fn needs_reindex(
        &self,
        workspace_files: &[(String, i64)], // (path, mtime_secs)
    ) -> Result<IncrementalPlan> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("FTS index lock poisoned".into()))?;
        Self::ensure_meta_table(&conn)?;

        // Load all known indexed paths and their mtimes.
        let mut stmt = conn
            .prepare("SELECT path, mtime_secs FROM index_meta")
            .map_err(|e| RetrievalError::SearchError(e.to_string()))?;
        let indexed: std::collections::HashMap<String, i64> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| RetrievalError::SearchError(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let workspace_set: std::collections::HashSet<&str> =
            workspace_files.iter().map(|(p, _)| p.as_str()).collect();

        let mut to_index = Vec::new();
        let mut to_remove = Vec::new();
        let mut up_to_date = 0usize;

        // Files that need (re-)indexing.
        for (path, mtime) in workspace_files {
            match indexed.get(path) {
                Some(old_mtime) if old_mtime == mtime => {
                    up_to_date += 1;
                }
                _ => {
                    to_index.push(path.clone());
                }
            }
        }

        // Files that were indexed but are gone from workspace.
        for indexed_path in indexed.keys() {
            if !workspace_set.contains(indexed_path.as_str()) {
                to_remove.push(indexed_path.clone());
            }
        }

        Ok(IncrementalPlan {
            to_index,
            to_remove,
            up_to_date,
        })
    }

    /// Remove indexed documents by path (for files deleted from workspace).
    pub fn remove_documents(&self, paths: &[String]) -> Result<usize> {
        if paths.is_empty() {
            return Ok(0);
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("FTS index lock poisoned".into()))?;
        Self::ensure_meta_table(&conn)?;

        let tx = conn.unchecked_transaction()?;
        let mut count = 0usize;
        {
            let mut del_doc = tx.prepare("DELETE FROM documents WHERE path = ?1")?;
            let mut del_meta = tx.prepare("DELETE FROM index_meta WHERE path = ?1")?;
            for path in paths {
                count += del_doc.execute([path])? as usize;
                del_meta.execute([path])?;
            }
        }
        tx.commit()?;
        debug!("removed {} documents from index", count);
        Ok(count)
    }

    /// Incrementally index documents, recording their modification time.
    ///
    /// Each tuple is `(id, path, content, mtime_secs)`.
    pub fn index_documents_incremental(
        &self,
        docs: Vec<(String, String, String, i64)>,
    ) -> Result<usize> {
        if docs.is_empty() {
            return Ok(0);
        }
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("FTS index lock poisoned".into()))?;
        Self::ensure_meta_table(&conn)?;

        let tx = conn.unchecked_transaction()?;
        let count = docs.len();
        {
            let mut del = tx.prepare("DELETE FROM documents WHERE id = ?1")?;
            let mut ins =
                tx.prepare("INSERT INTO documents (id, path, content) VALUES (?1, ?2, ?3)")?;
            let mut meta = tx.prepare(
                "INSERT OR REPLACE INTO index_meta (path, mtime_secs, indexed_at) VALUES (?1, ?2, datetime('now'))",
            )?;
            for (id, path, content, mtime) in &docs {
                del.execute([id.as_str()])?;
                ins.execute([id.as_str(), path.as_str(), content.as_str()])?;
                meta.execute(rusqlite::params![path, mtime])?;
            }
        }
        tx.commit()?;
        debug!("incrementally indexed {} documents", count);
        Ok(count)
    }
}

/// Plan for incremental re-indexing.
#[derive(Debug, Clone)]
pub struct IncrementalPlan {
    /// Files that need to be (re-)indexed (new or modified).
    pub to_index: Vec<String>,
    /// Files to remove from the index (deleted from workspace).
    pub to_remove: Vec<String>,
    /// Files already up-to-date.
    pub up_to_date: usize,
}

impl IncrementalPlan {
    /// True if no work needs to be done.
    pub fn is_noop(&self) -> bool {
        self.to_index.is_empty() && self.to_remove.is_empty()
    }

    /// Total files that need processing.
    pub fn work_count(&self) -> usize {
        self.to_index.len() + self.to_remove.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_and_search() {
        let idx = FtsIndex::new(":memory:").unwrap();
        idx.index_document(
            "1",
            "src/main.rs",
            "fn main() { println!(\"hello world\"); }",
        )
        .unwrap();
        idx.index_document("2", "src/lib.rs", "pub mod utils; pub mod config;")
            .unwrap();

        let results = idx.search("hello", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "1");
        assert!(results[0].rank > 0.0);
    }

    #[test]
    fn batch_index() {
        let idx = FtsIndex::new(":memory:").unwrap();
        let docs = vec![
            ("a".into(), "a.rs".into(), "alpha bravo charlie".into()),
            ("b".into(), "b.rs".into(), "delta echo foxtrot".into()),
            ("c".into(), "c.rs".into(), "alpha delta golf".into()),
        ];
        idx.index_documents_batch(docs).unwrap();

        let results = idx.search("alpha", 10).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn upsert_semantics() {
        let idx = FtsIndex::new(":memory:").unwrap();
        idx.index_document("1", "a.rs", "old content").unwrap();
        idx.index_document("1", "a.rs", "new content").unwrap();

        let results = idx.search("old", 10).unwrap();
        assert!(results.is_empty());

        let results = idx.search("new", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    // ── Incremental indexing tests ──

    #[test]
    fn needs_reindex_empty_index() {
        let idx = FtsIndex::new(":memory:").unwrap();
        let files = vec![
            ("src/main.rs".into(), 1000i64),
            ("src/lib.rs".into(), 2000i64),
        ];
        let plan = idx.needs_reindex(&files).unwrap();
        assert_eq!(plan.to_index.len(), 2);
        assert!(plan.to_remove.is_empty());
        assert_eq!(plan.up_to_date, 0);
        assert!(!plan.is_noop());
    }

    #[test]
    fn needs_reindex_all_up_to_date() {
        let idx = FtsIndex::new(":memory:").unwrap();
        idx.index_documents_incremental(vec![
            (
                "1".into(),
                "src/main.rs".into(),
                "fn main() {}".into(),
                1000,
            ),
            ("2".into(), "src/lib.rs".into(), "pub mod foo;".into(), 2000),
        ])
        .unwrap();

        let files = vec![
            ("src/main.rs".into(), 1000i64),
            ("src/lib.rs".into(), 2000i64),
        ];
        let plan = idx.needs_reindex(&files).unwrap();
        assert!(plan.to_index.is_empty());
        assert!(plan.to_remove.is_empty());
        assert_eq!(plan.up_to_date, 2);
        assert!(plan.is_noop());
    }

    #[test]
    fn needs_reindex_detects_modified() {
        let idx = FtsIndex::new(":memory:").unwrap();
        idx.index_documents_incremental(vec![(
            "1".into(),
            "src/main.rs".into(),
            "fn main() {}".into(),
            1000,
        )])
        .unwrap();

        // mtime changed from 1000 to 2000
        let files = vec![("src/main.rs".into(), 2000i64)];
        let plan = idx.needs_reindex(&files).unwrap();
        assert_eq!(plan.to_index, vec!["src/main.rs"]);
        assert_eq!(plan.up_to_date, 0);
    }

    #[test]
    fn needs_reindex_detects_deleted() {
        let idx = FtsIndex::new(":memory:").unwrap();
        idx.index_documents_incremental(vec![
            (
                "1".into(),
                "src/main.rs".into(),
                "fn main() {}".into(),
                1000,
            ),
            ("2".into(), "src/old.rs".into(), "// old".into(), 500),
        ])
        .unwrap();

        // src/old.rs no longer in workspace
        let files = vec![("src/main.rs".into(), 1000i64)];
        let plan = idx.needs_reindex(&files).unwrap();
        assert!(plan.to_index.is_empty());
        assert_eq!(plan.to_remove, vec!["src/old.rs"]);
        assert_eq!(plan.up_to_date, 1);
    }

    #[test]
    fn remove_documents_cleans_index() {
        let idx = FtsIndex::new(":memory:").unwrap();
        idx.index_documents_incremental(vec![
            ("1".into(), "a.rs".into(), "alpha bravo".into(), 100),
            ("2".into(), "b.rs".into(), "charlie delta".into(), 200),
        ])
        .unwrap();

        let removed = idx.remove_documents(&["a.rs".into()]).unwrap();
        assert_eq!(removed, 1);

        let results = idx.search("alpha", 10).unwrap();
        assert!(results.is_empty());

        // b.rs still there
        let results = idx.search("charlie", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn incremental_index_records_mtime() {
        let idx = FtsIndex::new(":memory:").unwrap();
        let count = idx
            .index_documents_incremental(vec![(
                "1".into(),
                "a.rs".into(),
                "hello world".into(),
                1000,
            )])
            .unwrap();
        assert_eq!(count, 1);

        // Verify it's searchable
        let results = idx.search("hello", 10).unwrap();
        assert_eq!(results.len(), 1);

        // Verify mtime is tracked (needs_reindex should show up-to-date)
        let plan = idx.needs_reindex(&[("a.rs".into(), 1000)]).unwrap();
        assert!(plan.is_noop());
    }

    #[test]
    fn incremental_plan_work_count() {
        let plan = IncrementalPlan {
            to_index: vec!["a.rs".into(), "b.rs".into()],
            to_remove: vec!["c.rs".into()],
            up_to_date: 5,
        };
        assert_eq!(plan.work_count(), 3);
        assert!(!plan.is_noop());
    }
}
