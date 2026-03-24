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
        let conn = self.conn.lock().expect("FtsIndex mutex poisoned");
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
        let conn = self.conn.lock().expect("FtsIndex mutex poisoned");
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

        let conn = self.conn.lock().expect("FtsIndex mutex poisoned");
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
}
