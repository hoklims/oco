use thiserror::Error;

/// Errors produced by the retrieval engine.
#[derive(Debug, Error)]
pub enum RetrievalError {
    /// SQLite / FTS5 database error.
    #[error("database error: {0}")]
    DatabaseError(String),

    /// Failure while building or updating an index.
    #[error("index error: {0}")]
    IndexError(String),

    /// Failure during a search query.
    #[error("search error: {0}")]
    SearchError(String),

    /// Error from the vector backend.
    #[error("vector backend error: {0}")]
    VectorBackendError(String),
}

impl From<rusqlite::Error> for RetrievalError {
    fn from(err: rusqlite::Error) -> Self {
        Self::DatabaseError(err.to_string())
    }
}

/// Crate-level result alias.
pub type Result<T> = std::result::Result<T, RetrievalError>;
