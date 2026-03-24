//! HTTP client for the Python ML worker (embedding + reranking).

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// HTTP bridge to the Python ML worker service.
pub struct MlWorkerClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    texts: &'a [String],
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

#[derive(Serialize)]
struct RerankRequest<'a> {
    query: &'a str,
    documents: &'a [String],
    top_k: u32,
}

#[derive(Deserialize)]
struct RerankResponse {
    results: Vec<RerankHit>,
}

#[derive(Deserialize)]
struct RerankHit {
    index: usize,
    score: f64,
}

impl MlWorkerClient {
    /// Create a new client targeting the given base URL.
    ///
    /// Returns an error if the HTTP client cannot be built (e.g. TLS init failure).
    pub fn new(base_url: &str) -> Result<Self> {
        let base_url = base_url.trim_end_matches('/').to_string();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .context("failed to build ML worker HTTP client")?;
        Ok(Self { base_url, client })
    }

    /// Embed a batch of texts. Returns one embedding vector per input text.
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/embed", self.base_url);
        debug!(url = %url, count = texts.len(), "ML embed request");

        let resp = self
            .client
            .post(&url)
            .json(&EmbedRequest { texts })
            .send()
            .await?
            .error_for_status()?;

        let body: EmbedResponse = resp.json().await?;

        // Validate response cardinality
        if body.embeddings.len() != texts.len() {
            bail!(
                "ML embed response mismatch: expected {} embeddings, got {}",
                texts.len(),
                body.embeddings.len()
            );
        }

        Ok(body.embeddings)
    }

    /// Rerank documents against a query. Returns `(document_index, score)` pairs
    /// sorted by descending relevance.
    pub async fn rerank(
        &self,
        query: &str,
        documents: &[String],
        top_k: u32,
    ) -> Result<Vec<(usize, f64)>> {
        let url = format!("{}/rerank", self.base_url);
        // Don't log raw query — may contain sensitive content
        debug!(url = %url, docs = documents.len(), top_k = top_k, "ML rerank request");

        let resp = self
            .client
            .post(&url)
            .json(&RerankRequest {
                query,
                documents,
                top_k,
            })
            .send()
            .await?
            .error_for_status()?;

        let body: RerankResponse = resp.json().await?;

        // Validate indices are within bounds
        let results: Vec<(usize, f64)> = body
            .results
            .into_iter()
            .filter(|h| h.index < documents.len())
            .map(|h| (h.index, h.score))
            .collect();

        Ok(results)
    }

    /// Check whether the ML worker is alive.
    pub async fn health(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        match self.client.get(&url).send().await {
            Ok(resp) => Ok(resp.status().is_success()),
            Err(e) => {
                warn!(error = %e, "ML worker health check failed");
                Ok(false)
            }
        }
    }
}
