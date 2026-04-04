//! Retrieval engine for the Open Context Orchestrator.
//!
//! Provides four complementary retrieval strategies:
//!
//! - **FTS5** (`fts`): Lexical full-text search via SQLite FTS5.
//! - **Vector** (`vector`): Pluggable vector similarity search with an
//!   in-memory brute-force backend included.
//! - **Hybrid** (`hybrid`): Reciprocal Rank Fusion combining FTS5 and vector
//!   results.
//! - **Call Graph** (`call_graph`): SQLite-backed call edge storage with BFS
//!   traversal for impact analysis and route discovery.

pub mod call_graph;
pub mod error;
pub mod fts;
pub mod hybrid;
pub mod vector;

pub use call_graph::{CallGraphIndex, ImpactResult, RouteNode, StoredCallEdge};
pub use error::{Result, RetrievalError};
pub use fts::{FtsIndex, FtsResult, IncrementalPlan};
pub use hybrid::{HybridRetriever, RetrievalResult};
pub use vector::{InMemoryVectorBackend, VectorBackend, VectorResult};
