use std::collections::HashMap;
use std::sync::RwLock;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::error::{RetrievalError, Result};

/// A single vector search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorResult {
    pub id: String,
    pub score: f64,
    pub metadata: serde_json::Value,
}

/// Async trait for pluggable vector backends (in-memory, Qdrant, etc.).
#[async_trait]
pub trait VectorBackend: Send + Sync {
    /// Insert or update an embedding with associated metadata.
    async fn upsert(
        &self,
        id: &str,
        embedding: Vec<f32>,
        metadata: serde_json::Value,
    ) -> Result<()>;

    /// Find the `top_k` nearest neighbours to `query_embedding`.
    async fn search(
        &self,
        query_embedding: Vec<f32>,
        top_k: u32,
    ) -> Result<Vec<VectorResult>>;

    /// Delete the vector with the given `id`.
    async fn delete(&self, id: &str) -> Result<()>;
}

// ---------------------------------------------------------------------------
// In-memory backend (brute-force cosine similarity)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct StoredVector {
    embedding: Vec<f32>,
    metadata: serde_json::Value,
}

/// A simple, dependency-free vector store that keeps everything in memory.
///
/// Uses brute-force cosine similarity — suitable for small-to-medium
/// collections where an external vector DB is overkill.
pub struct InMemoryVectorBackend {
    store: RwLock<HashMap<String, StoredVector>>,
}

impl InMemoryVectorBackend {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryVectorBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VectorBackend for InMemoryVectorBackend {
    #[instrument(skip(self, embedding, metadata), fields(id))]
    async fn upsert(
        &self,
        id: &str,
        embedding: Vec<f32>,
        metadata: serde_json::Value,
    ) -> Result<()> {
        let mut store = self
            .store
            .write()
            .map_err(|e| RetrievalError::VectorBackendError(e.to_string()))?;
        store.insert(
            id.to_owned(),
            StoredVector {
                embedding,
                metadata,
            },
        );
        debug!("upserted vector {id}");
        Ok(())
    }

    #[instrument(skip(self, query_embedding), fields(top_k))]
    async fn search(
        &self,
        query_embedding: Vec<f32>,
        top_k: u32,
    ) -> Result<Vec<VectorResult>> {
        let store = self
            .store
            .read()
            .map_err(|e| RetrievalError::VectorBackendError(e.to_string()))?;

        let mut scored: Vec<(String, f64, serde_json::Value)> = store
            .iter()
            .map(|(id, sv)| {
                let score = cosine_similarity(&query_embedding, &sv.embedding);
                (id.clone(), score, sv.metadata.clone())
            })
            .collect();

        // Sort descending by score.
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(top_k as usize);

        let results: Vec<VectorResult> = scored
            .into_iter()
            .map(|(id, score, metadata)| VectorResult {
                id,
                score,
                metadata,
            })
            .collect();

        debug!("vector search returned {} results", results.len());
        Ok(results)
    }

    #[instrument(skip(self), fields(id))]
    async fn delete(&self, id: &str) -> Result<()> {
        let mut store = self
            .store
            .write()
            .map_err(|e| RetrievalError::VectorBackendError(e.to_string()))?;
        store.remove(id);
        debug!("deleted vector {id}");
        Ok(())
    }
}

/// Cosine similarity between two vectors.
///
/// Returns 0.0 when either vector has zero magnitude.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let (mut dot, mut mag_a, mut mag_b) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        mag_a += x * x;
        mag_b += y * y;
    }
    let denom = mag_a.sqrt() * mag_b.sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_and_search() {
        let backend = InMemoryVectorBackend::new();

        backend
            .upsert("a", vec![1.0, 0.0, 0.0], serde_json::json!({"path": "a.rs"}))
            .await
            .unwrap();
        backend
            .upsert("b", vec![0.0, 1.0, 0.0], serde_json::json!({"path": "b.rs"}))
            .await
            .unwrap();
        backend
            .upsert("c", vec![0.9, 0.1, 0.0], serde_json::json!({"path": "c.rs"}))
            .await
            .unwrap();

        let results = backend.search(vec![1.0, 0.0, 0.0], 2).await.unwrap();
        assert_eq!(results.len(), 2);
        // "a" should be the closest match (exact).
        assert_eq!(results[0].id, "a");
        assert!((results[0].score - 1.0).abs() < 1e-6);
    }

    #[tokio::test]
    async fn delete_removes_entry() {
        let backend = InMemoryVectorBackend::new();
        backend
            .upsert("x", vec![1.0, 0.0], serde_json::json!({}))
            .await
            .unwrap();
        backend.delete("x").await.unwrap();

        let results = backend.search(vec![1.0, 0.0], 10).await.unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn cosine_identical_vectors() {
        let v = vec![1.0, 2.0, 3.0];
        let score = cosine_similarity(&v, &v);
        assert!((score - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let score = cosine_similarity(&a, &b);
        assert!(score.abs() < 1e-9);
    }
}
