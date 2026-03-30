//! Call graph storage and traversal backed by SQLite.
//!
//! Stores caller→callee edges extracted by `oco-code-intel` and provides
//! BFS-based traversal for impact analysis and route discovery.

use std::collections::{HashSet, VecDeque};
use std::sync::Mutex;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::error::{Result, RetrievalError};

/// A stored call edge with file context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCallEdge {
    /// Source file path where the call occurs.
    pub file: String,
    /// Name of the calling function/method.
    pub caller: String,
    /// Name of the called function/method.
    pub callee: String,
    /// 1-based line number where the call occurs.
    pub line: u32,
    /// 1-based column where the call occurs.
    pub col: u32,
    /// How this call was resolved (direct, member, scoped, dynamic_guess).
    pub edge_type: String,
    /// Resolution confidence (0.0–1.0).
    pub confidence: f32,
}

/// A node in a route (call chain) result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteNode {
    /// The function/method name.
    pub symbol: String,
    /// File where it is defined (if known from an edge).
    pub file: Option<String>,
    /// Depth from the query root (0 = the queried symbol itself).
    pub depth: u32,
}

/// Result of an impact analysis query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactResult {
    /// The symbol whose impact was queried.
    pub target: String,
    /// All callers (direct and transitive) up to max depth.
    pub callers: Vec<RouteNode>,
    /// All callees (direct and transitive) up to max depth.
    pub callees: Vec<RouteNode>,
}

/// Call graph index backed by SQLite.
///
/// Thread-safe via `Mutex<Connection>` (same pattern as [`FtsIndex`](crate::fts::FtsIndex)).
pub struct CallGraphIndex {
    conn: Mutex<Connection>,
}

impl CallGraphIndex {
    /// Open (or create) a call graph SQLite database at `db_path`.
    ///
    /// Pass `":memory:"` for a purely in-memory index.
    #[instrument(skip_all, fields(db_path))]
    pub fn new(db_path: &str) -> Result<Self> {
        let conn = if db_path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            Connection::open(db_path)?
        };

        conn.pragma_update(None, "journal_mode", "WAL")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS calls (
                file       TEXT NOT NULL,
                caller     TEXT NOT NULL,
                callee     TEXT NOT NULL,
                line       INTEGER NOT NULL,
                col        INTEGER NOT NULL,
                edge_type  TEXT NOT NULL DEFAULT 'direct',
                confidence REAL NOT NULL DEFAULT 1.0
            );
            CREATE TABLE IF NOT EXISTS file_meta (
                file         TEXT PRIMARY KEY,
                modified_at  INTEGER NOT NULL,
                edge_count   INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_calls_caller ON calls(caller);
            CREATE INDEX IF NOT EXISTS idx_calls_callee ON calls(callee);
            CREATE INDEX IF NOT EXISTS idx_calls_file ON calls(file);",
        )?;

        debug!("call graph index ready at {db_path}");
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Index call edges for a single file. Replaces all existing edges for that file.
    #[instrument(skip(self, edges), fields(file, count = edges.len()))]
    pub fn index_file_calls(&self, file: &str, edges: &[StoredCallEdge]) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("call graph lock poisoned".into()))?;
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM calls WHERE file = ?1", [file])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO calls (file, caller, callee, line, col, edge_type, confidence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for edge in edges {
                stmt.execute(rusqlite::params![
                    edge.file,
                    edge.caller,
                    edge.callee,
                    edge.line,
                    edge.col,
                    edge.edge_type,
                    edge.confidence
                ])?;
            }
        }
        tx.commit()?;
        debug!("indexed {} call edges for {file}", edges.len());
        Ok(())
    }

    /// Batch index call edges for multiple files in a single transaction.
    #[instrument(skip_all, fields(count = edges.len()))]
    pub fn index_calls_batch(&self, edges: &[StoredCallEdge]) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("call graph lock poisoned".into()))?;

        // Collect unique files to clear
        let files: HashSet<&str> = edges.iter().map(|e| e.file.as_str()).collect();

        let tx = conn.unchecked_transaction()?;
        {
            let mut del_stmt = tx.prepare("DELETE FROM calls WHERE file = ?1")?;
            for file in &files {
                del_stmt.execute([*file])?;
            }
        }
        {
            let mut ins_stmt = tx.prepare(
                "INSERT INTO calls (file, caller, callee, line, col, edge_type, confidence) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for edge in edges {
                ins_stmt.execute(rusqlite::params![
                    edge.file,
                    edge.caller,
                    edge.callee,
                    edge.line,
                    edge.col,
                    edge.edge_type,
                    edge.confidence
                ])?;
            }
        }
        tx.commit()?;
        debug!(
            "batch-indexed {} call edges across {} files",
            edges.len(),
            files.len()
        );
        Ok(())
    }

    /// Find all direct callers of a symbol.
    pub fn callers_of(&self, callee: &str) -> Result<Vec<StoredCallEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("call graph lock poisoned".into()))?;
        let mut stmt =
            conn.prepare("SELECT file, caller, callee, line, col, edge_type, confidence FROM calls WHERE callee = ?1")?;
        let rows = stmt
            .query_map([callee], |row| {
                Ok(StoredCallEdge {
                    file: row.get(0)?,
                    caller: row.get(1)?,
                    callee: row.get(2)?,
                    line: row.get(3)?,
                    col: row.get(4)?,
                    edge_type: row.get(5)?,
                    confidence: row.get(6)?,
                })
            })
            .map_err(|e| RetrievalError::SearchError(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| RetrievalError::SearchError(e.to_string()))
    }

    /// Find all direct callees of a symbol.
    pub fn callees_of(&self, caller: &str) -> Result<Vec<StoredCallEdge>> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("call graph lock poisoned".into()))?;
        let mut stmt =
            conn.prepare("SELECT file, caller, callee, line, col, edge_type, confidence FROM calls WHERE caller = ?1")?;
        let rows = stmt
            .query_map([caller], |row| {
                Ok(StoredCallEdge {
                    file: row.get(0)?,
                    caller: row.get(1)?,
                    callee: row.get(2)?,
                    line: row.get(3)?,
                    col: row.get(4)?,
                    edge_type: row.get(5)?,
                    confidence: row.get(6)?,
                })
            })
            .map_err(|e| RetrievalError::SearchError(e.to_string()))?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| RetrievalError::SearchError(e.to_string()))
    }

    /// BFS traversal to find all transitive callers (who calls this, recursively).
    ///
    /// `max_depth` limits traversal depth (default 5). Returns route nodes ordered by depth.
    pub fn routes_callers(&self, symbol: &str, max_depth: u32) -> Result<Vec<RouteNode>> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        visited.insert(symbol.to_string());
        queue.push_back((symbol.to_string(), 0u32));

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            let callers = self.callers_of(&current)?;
            for edge in callers {
                if visited.insert(edge.caller.clone()) {
                    let next_depth = depth + 1;
                    result.push(RouteNode {
                        symbol: edge.caller.clone(),
                        file: Some(edge.file),
                        depth: next_depth,
                    });
                    queue.push_back((edge.caller, next_depth));
                }
            }
        }

        Ok(result)
    }

    /// BFS traversal to find all transitive callees (what does this call, recursively).
    ///
    /// `max_depth` limits traversal depth (default 5). Returns route nodes ordered by depth.
    pub fn routes_callees(&self, symbol: &str, max_depth: u32) -> Result<Vec<RouteNode>> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        let mut result = Vec::new();

        visited.insert(symbol.to_string());
        queue.push_back((symbol.to_string(), 0u32));

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            let callees = self.callees_of(&current)?;
            for edge in callees {
                if visited.insert(edge.callee.clone()) {
                    let next_depth = depth + 1;
                    result.push(RouteNode {
                        symbol: edge.callee.clone(),
                        file: Some(edge.file),
                        depth: next_depth,
                    });
                    queue.push_back((edge.callee, next_depth));
                }
            }
        }

        Ok(result)
    }

    /// Full impact analysis: find all callers AND callees transitively.
    pub fn impact(&self, symbol: &str, max_depth: u32) -> Result<ImpactResult> {
        let callers = self.routes_callers(symbol, max_depth)?;
        let callees = self.routes_callees(symbol, max_depth)?;
        Ok(ImpactResult {
            target: symbol.to_string(),
            callers,
            callees,
        })
    }

    /// Return total number of stored edges.
    pub fn edge_count(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("call graph lock poisoned".into()))?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM calls", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Record that a file was indexed at the given modification timestamp (epoch secs).
    pub fn record_file_meta(&self, file: &str, modified_at: u64, edge_count: usize) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("call graph lock poisoned".into()))?;
        conn.execute(
            "INSERT OR REPLACE INTO file_meta (file, modified_at, edge_count) VALUES (?1, ?2, ?3)",
            rusqlite::params![file, modified_at as i64, edge_count as i64],
        )?;
        Ok(())
    }

    /// Check if a file needs re-indexing based on its modification timestamp.
    ///
    /// Returns `true` if the file is not yet indexed or the stored timestamp
    /// is older than `current_modified_at`.
    pub fn needs_reindex(&self, file: &str, current_modified_at: u64) -> Result<bool> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("call graph lock poisoned".into()))?;
        let result: std::result::Result<i64, _> = conn.query_row(
            "SELECT modified_at FROM file_meta WHERE file = ?1",
            [file],
            |row| row.get(0),
        );
        match result {
            Ok(stored) => Ok((stored as u64) < current_modified_at),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(true),
            Err(e) => Err(RetrievalError::SearchError(e.to_string())),
        }
    }

    /// Remove all edges and metadata for a file (e.g. when the file is deleted).
    pub fn remove_file(&self, file: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("call graph lock poisoned".into()))?;
        conn.execute("DELETE FROM calls WHERE file = ?1", [file])?;
        conn.execute("DELETE FROM file_meta WHERE file = ?1", [file])?;
        Ok(())
    }

    /// Return the number of indexed files.
    pub fn file_count(&self) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|_| RetrievalError::SearchError("call graph lock poisoned".into()))?;
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM file_meta", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_edges() -> Vec<StoredCallEdge> {
        vec![
            StoredCallEdge {
                file: "src/main.rs".into(),
                caller: "main".into(),
                callee: "run".into(),
                line: 5,
                col: 5,
                edge_type: "direct".into(),
                confidence: 1.0,
            },
            StoredCallEdge {
                file: "src/main.rs".into(),
                caller: "main".into(),
                callee: "init".into(),
                line: 6,
                col: 5,
                edge_type: "direct".into(),
                confidence: 1.0,
            },
            StoredCallEdge {
                file: "src/lib.rs".into(),
                caller: "run".into(),
                callee: "process".into(),
                line: 10,
                col: 9,
                edge_type: "direct".into(),
                confidence: 1.0,
            },
            StoredCallEdge {
                file: "src/lib.rs".into(),
                caller: "run".into(),
                callee: "cleanup".into(),
                line: 15,
                col: 9,
                edge_type: "member".into(),
                confidence: 0.9,
            },
            StoredCallEdge {
                file: "src/lib.rs".into(),
                caller: "process".into(),
                callee: "validate".into(),
                line: 20,
                col: 13,
                edge_type: "scoped".into(),
                confidence: 0.85,
            },
        ]
    }

    #[test]
    fn index_and_query_callers() {
        let idx = CallGraphIndex::new(":memory:").unwrap();
        idx.index_calls_batch(&sample_edges()).unwrap();

        let callers = idx.callers_of("run").unwrap();
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].caller, "main");
    }

    #[test]
    fn index_and_query_callees() {
        let idx = CallGraphIndex::new(":memory:").unwrap();
        idx.index_calls_batch(&sample_edges()).unwrap();

        let callees = idx.callees_of("main").unwrap();
        assert_eq!(callees.len(), 2);
        let names: Vec<&str> = callees.iter().map(|e| e.callee.as_str()).collect();
        assert!(names.contains(&"run"));
        assert!(names.contains(&"init"));
    }

    #[test]
    fn transitive_callers_bfs() {
        let idx = CallGraphIndex::new(":memory:").unwrap();
        idx.index_calls_batch(&sample_edges()).unwrap();

        // validate is called by process, which is called by run, which is called by main
        let callers = idx.routes_callers("validate", 5).unwrap();
        let names: Vec<&str> = callers.iter().map(|n| n.symbol.as_str()).collect();
        assert!(names.contains(&"process"), "direct caller");
        assert!(names.contains(&"run"), "transitive caller");
        assert!(names.contains(&"main"), "root caller");
        assert_eq!(
            callers
                .iter()
                .find(|n| n.symbol == "process")
                .unwrap()
                .depth,
            1
        );
        assert_eq!(callers.iter().find(|n| n.symbol == "run").unwrap().depth, 2);
        assert_eq!(
            callers.iter().find(|n| n.symbol == "main").unwrap().depth,
            3
        );
    }

    #[test]
    fn transitive_callees_bfs() {
        let idx = CallGraphIndex::new(":memory:").unwrap();
        idx.index_calls_batch(&sample_edges()).unwrap();

        let callees = idx.routes_callees("main", 5).unwrap();
        let names: Vec<&str> = callees.iter().map(|n| n.symbol.as_str()).collect();
        assert!(names.contains(&"run"));
        assert!(names.contains(&"init"));
        assert!(names.contains(&"process"));
        assert!(names.contains(&"cleanup"));
        assert!(names.contains(&"validate"));
    }

    #[test]
    fn impact_analysis() {
        let idx = CallGraphIndex::new(":memory:").unwrap();
        idx.index_calls_batch(&sample_edges()).unwrap();

        let impact = idx.impact("run", 5).unwrap();
        assert_eq!(impact.target, "run");
        assert!(!impact.callers.is_empty(), "run has callers");
        assert!(!impact.callees.is_empty(), "run has callees");
    }

    #[test]
    fn depth_limit_respected() {
        let idx = CallGraphIndex::new(":memory:").unwrap();
        idx.index_calls_batch(&sample_edges()).unwrap();

        // depth=1 from validate should only find process
        let callers = idx.routes_callers("validate", 1).unwrap();
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].symbol, "process");
    }

    #[test]
    fn upsert_per_file() {
        let idx = CallGraphIndex::new(":memory:").unwrap();
        let edges = vec![StoredCallEdge {
            file: "a.rs".into(),
            caller: "foo".into(),
            callee: "bar".into(),
            line: 1,
            col: 1,
            edge_type: "direct".into(),
            confidence: 1.0,
        }];
        idx.index_file_calls("a.rs", &edges).unwrap();
        assert_eq!(idx.edge_count().unwrap(), 1);

        // Re-index same file with different edges
        let edges2 = vec![StoredCallEdge {
            file: "a.rs".into(),
            caller: "baz".into(),
            callee: "qux".into(),
            line: 2,
            col: 1,
            edge_type: "member".into(),
            confidence: 0.9,
        }];
        idx.index_file_calls("a.rs", &edges2).unwrap();
        assert_eq!(idx.edge_count().unwrap(), 1);

        let callers = idx.callers_of("bar").unwrap();
        assert!(callers.is_empty(), "old edges should be replaced");
    }

    #[test]
    fn cycle_safe() {
        let idx = CallGraphIndex::new(":memory:").unwrap();
        let edges = vec![
            StoredCallEdge {
                file: "a.rs".into(),
                caller: "a".into(),
                callee: "b".into(),
                line: 1,
                col: 1,
                edge_type: "direct".into(),
                confidence: 1.0,
            },
            StoredCallEdge {
                file: "a.rs".into(),
                caller: "b".into(),
                callee: "a".into(),
                line: 2,
                col: 1,
                edge_type: "direct".into(),
                confidence: 1.0,
            },
        ];
        idx.index_calls_batch(&edges).unwrap();

        // BFS should not loop forever
        let callers = idx.routes_callers("a", 10).unwrap();
        assert_eq!(callers.len(), 1, "cycle: only b calls a");
        assert_eq!(callers[0].symbol, "b");
    }

    #[test]
    fn incremental_reindex_tracking() {
        let idx = CallGraphIndex::new(":memory:").unwrap();

        // File not yet indexed → needs reindex
        assert!(idx.needs_reindex("src/main.rs", 1000).unwrap());

        // Record indexing
        idx.record_file_meta("src/main.rs", 1000, 5).unwrap();

        // Same timestamp → no reindex needed
        assert!(!idx.needs_reindex("src/main.rs", 1000).unwrap());

        // Older timestamp → no reindex needed
        assert!(!idx.needs_reindex("src/main.rs", 999).unwrap());

        // Newer timestamp → needs reindex
        assert!(idx.needs_reindex("src/main.rs", 1001).unwrap());

        // Different file → needs reindex
        assert!(idx.needs_reindex("src/lib.rs", 1000).unwrap());
    }

    #[test]
    fn remove_file_cleans_up() {
        let idx = CallGraphIndex::new(":memory:").unwrap();
        let edges = vec![StoredCallEdge {
            file: "a.rs".into(),
            caller: "foo".into(),
            callee: "bar".into(),
            line: 1,
            col: 1,
            edge_type: "direct".into(),
            confidence: 1.0,
        }];
        idx.index_file_calls("a.rs", &edges).unwrap();
        idx.record_file_meta("a.rs", 1000, 1).unwrap();
        assert_eq!(idx.edge_count().unwrap(), 1);
        assert_eq!(idx.file_count().unwrap(), 1);

        idx.remove_file("a.rs").unwrap();
        assert_eq!(idx.edge_count().unwrap(), 0);
        assert_eq!(idx.file_count().unwrap(), 0);
        assert!(idx.needs_reindex("a.rs", 1000).unwrap());
    }
}
