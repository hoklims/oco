use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::error::Result;
use crate::fts::FtsIndex;
use crate::vector::VectorBackend;

/// A retrieval result produced by the hybrid retriever, combining lexical and
/// vector search scores via Reciprocal Rank Fusion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub id: String,
    pub path: String,
    pub content: String,
    pub score: f64,
    /// `"fts"`, `"vector"`, or `"hybrid"` depending on which sources matched.
    pub source: String,
}

/// Combines an [`FtsIndex`] (lexical / FTS5) with a [`VectorBackend`]
/// (semantic) using Reciprocal Rank Fusion (RRF).
pub struct HybridRetriever<V: VectorBackend> {
    fts: FtsIndex,
    vector: V,
}

/// Constant `k` for RRF: `score = 1 / (k + rank)`.
///
/// A value of 60 is the standard default from the original RRF paper.
const RRF_K: f64 = 60.0;

impl<V: VectorBackend> HybridRetriever<V> {
    /// Build a new hybrid retriever from an FTS index and a vector backend.
    pub fn new(fts: FtsIndex, vector: V) -> Self {
        Self { fts, vector }
    }

    /// Retrieve documents by combining FTS5 lexical search and vector
    /// similarity search via weighted Reciprocal Rank Fusion.
    ///
    /// * `query` — the user's natural-language query (used as-is for FTS5).
    /// * `query_embedding` — the embedding of `query` (pre-computed by caller).
    /// * `fts_weight` — relative weight for the FTS5 component.
    /// * `vector_weight` — relative weight for the vector component.
    /// * `limit` — maximum number of results to return.
    #[instrument(skip(self, query_embedding), fields(query, fts_weight, vector_weight, limit))]
    pub async fn retrieve(
        &self,
        query: &str,
        query_embedding: Vec<f32>,
        fts_weight: f64,
        vector_weight: f64,
        limit: u32,
    ) -> Result<Vec<RetrievalResult>> {
        // Over-fetch from each source so RRF has enough candidates.
        let fetch_limit = limit * 3;

        // FTS5 search (synchronous).
        let fts_results = self.fts.search(query, fetch_limit)?;

        // Vector search (async).
        let vec_results = self.vector.search(query_embedding, fetch_limit).await?;

        // --- Reciprocal Rank Fusion ---
        //
        // For each ranked list, the RRF score for a document at position r is:
        //   weight * 1 / (k + r)
        // We accumulate across both lists per document id.

        let mut scores: HashMap<String, RrfEntry> = HashMap::new();

        // FTS results (already sorted by rank descending, i.e. best first).
        for (rank, fts) in fts_results.iter().enumerate() {
            let rrf_score = fts_weight * (1.0 / (RRF_K + rank as f64 + 1.0));
            let entry = scores.entry(fts.id.clone()).or_insert_with(|| RrfEntry {
                path: fts.path.clone(),
                content: fts.snippet.clone(),
                score: 0.0,
                has_fts: false,
                has_vector: false,
            });
            entry.score += rrf_score;
            entry.has_fts = true;
        }

        // Vector results (already sorted by similarity descending).
        for (rank, vr) in vec_results.iter().enumerate() {
            let rrf_score = vector_weight * (1.0 / (RRF_K + rank as f64 + 1.0));
            let entry = scores.entry(vr.id.clone()).or_insert_with(|| {
                let path = vr
                    .metadata
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let content = vr
                    .metadata
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                RrfEntry {
                    path,
                    content,
                    score: 0.0,
                    has_fts: false,
                    has_vector: false,
                }
            });
            entry.score += rrf_score;
            entry.has_vector = true;
        }

        // Sort by fused score descending and take top `limit`.
        let mut results: Vec<(String, RrfEntry)> = scores.into_iter().collect();
        results.sort_by(|a, b| b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit as usize);

        let out: Vec<RetrievalResult> = results
            .into_iter()
            .map(|(id, entry)| {
                let source = match (entry.has_fts, entry.has_vector) {
                    (true, true) => "hybrid".to_owned(),
                    (true, false) => "fts".to_owned(),
                    (false, true) => "vector".to_owned(),
                    _ => "unknown".to_owned(),
                };
                RetrievalResult {
                    id,
                    path: entry.path,
                    content: entry.content,
                    score: entry.score,
                    source,
                }
            })
            .collect();

        debug!("hybrid retrieval returned {} results", out.len());
        Ok(out)
    }

    /// Convenience: retrieve using only FTS (no embedding required).
    pub fn retrieve_fts_only(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<RetrievalResult>> {
        let fts_results = self.fts.search(query, limit)?;
        Ok(fts_results
            .into_iter()
            .map(|fts| RetrievalResult {
                id: fts.id,
                path: fts.path,
                content: fts.snippet,
                score: fts.rank,
                source: "fts".to_owned(),
            })
            .collect())
    }
}

/// Internal bookkeeping struct for RRF score accumulation.
struct RrfEntry {
    path: String,
    content: String,
    score: f64,
    has_fts: bool,
    has_vector: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fts::FtsIndex;
    use crate::vector::InMemoryVectorBackend;

    fn setup() -> HybridRetriever<InMemoryVectorBackend> {
        let fts = FtsIndex::new(":memory:").unwrap();
        let vec_backend = InMemoryVectorBackend::new();
        HybridRetriever::new(fts, vec_backend)
    }

    #[tokio::test]
    async fn hybrid_retrieval_combines_sources() {
        let retriever = setup();

        // Index into FTS.
        retriever
            .fts
            .index_document("1", "src/main.rs", "async runtime tokio executor")
            .unwrap();
        retriever
            .fts
            .index_document("2", "src/lib.rs", "public library interface module")
            .unwrap();

        // Index into vector store.
        retriever
            .vector
            .upsert(
                "1",
                vec![1.0, 0.0, 0.0],
                serde_json::json!({"path": "src/main.rs", "content": "async runtime"}),
            )
            .await
            .unwrap();
        retriever
            .vector
            .upsert(
                "3",
                vec![0.9, 0.1, 0.0],
                serde_json::json!({"path": "src/utils.rs", "content": "utility helpers"}),
            )
            .await
            .unwrap();

        let results = retriever
            .retrieve("tokio runtime", vec![1.0, 0.0, 0.0], 1.0, 1.0, 10)
            .await
            .unwrap();

        // doc "1" should appear from both FTS and vector → source = "hybrid".
        let doc1 = results.iter().find(|r| r.id == "1").unwrap();
        assert_eq!(doc1.source, "hybrid");

        // doc "3" only from vector.
        let doc3 = results.iter().find(|r| r.id == "3").unwrap();
        assert_eq!(doc3.source, "vector");
    }

    #[test]
    fn fts_only_retrieval() {
        let retriever = setup();
        retriever
            .fts
            .index_document("1", "a.rs", "hello world program")
            .unwrap();

        let results = retriever.retrieve_fts_only("hello", 5).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].source, "fts");
    }
}
