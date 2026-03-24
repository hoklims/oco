use anyhow::Result;
use async_trait::async_trait;
use oco_shared_types::ContextItem;
use tracing::debug;

use crate::estimator::TokenEstimator;

/// Trait for strategies that compress context items to fit a target token budget.
#[async_trait]
pub trait ContextCompressor: Send + Sync {
    /// Compress / shrink `items` so their total estimated tokens fit within `target_tokens`.
    async fn compress(
        &self,
        items: &[ContextItem],
        target_tokens: u32,
    ) -> Result<Vec<ContextItem>>;
}

// ---------------------------------------------------------------------------
// TruncationCompressor
// ---------------------------------------------------------------------------

/// Simple compression strategy: truncates long items to fit the budget.
///
/// For content that looks like code it preserves the first and last N lines,
/// inserting a `// ... truncated ...` marker in between.
pub struct TruncationCompressor {
    /// Number of lines to keep at the start and end of truncated code blocks.
    preserve_lines: usize,
}

impl TruncationCompressor {
    pub fn new(preserve_lines: usize) -> Self {
        Self { preserve_lines }
    }

    /// Truncate a single item's content so it fits within `max_tokens`.
    fn truncate_content(&self, content: &str, max_tokens: u32) -> String {
        let current = TokenEstimator::estimate_tokens(content);
        if current <= max_tokens {
            return content.to_string();
        }

        let lines: Vec<&str> = content.lines().collect();

        // If there are enough lines, keep head + tail.
        if lines.len() > self.preserve_lines * 2 + 1 {
            let head = &lines[..self.preserve_lines];
            let tail = &lines[lines.len() - self.preserve_lines..];
            let truncated = format!(
                "{}\n// ... truncated ({} lines omitted) ...\n{}",
                head.join("\n"),
                lines.len() - self.preserve_lines * 2,
                tail.join("\n"),
            );

            // If the truncated version is still too large, fall back to pure byte truncation.
            if TokenEstimator::estimate_tokens(&truncated) <= max_tokens {
                return truncated;
            }
        }

        // Fallback: keep as many bytes as the budget allows (~4 bytes/token).
        let target_bytes = (max_tokens as usize) * 4;
        let mut end = target_bytes.min(content.len());
        // Avoid splitting a multi-byte char.
        while end < content.len() && !content.is_char_boundary(end) {
            end -= 1;
        }
        let mut result = content[..end].to_string();
        result.push_str("\n// ... truncated ...");
        result
    }
}

impl Default for TruncationCompressor {
    fn default() -> Self {
        Self::new(10)
    }
}

#[async_trait]
impl ContextCompressor for TruncationCompressor {
    async fn compress(
        &self,
        items: &[ContextItem],
        target_tokens: u32,
    ) -> Result<Vec<ContextItem>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }

        let current_total: u32 = items.iter().map(TokenEstimator::estimate_item).sum();

        if current_total <= target_tokens {
            return Ok(items.to_vec());
        }

        // Compute a per-item shrink ratio so items shrink proportionally.
        let ratio = target_tokens as f64 / current_total as f64;

        let mut result = Vec::with_capacity(items.len());
        for item in items {
            let item_tokens = TokenEstimator::estimate_item(item);
            let item_target = ((item_tokens as f64) * ratio).floor() as u32;
            let item_target = item_target.max(1);

            let new_content = self.truncate_content(&item.content, item_target);
            let new_estimate = TokenEstimator::estimate_tokens(&new_content);

            debug!(
                key = %item.key,
                original_tokens = item_tokens,
                target_tokens = item_target,
                actual_tokens = new_estimate,
                "truncated item"
            );

            result.push(ContextItem {
                content: new_content,
                token_estimate: new_estimate,
                ..item.clone()
            });
        }

        Ok(result)
    }
}

// ---------------------------------------------------------------------------
// SummaryCompressor
// ---------------------------------------------------------------------------

/// Placeholder compressor that *would* call an LLM to summarise context items.
///
/// For now it falls back to [`TruncationCompressor`].
pub struct SummaryCompressor {
    fallback: TruncationCompressor,
}

impl SummaryCompressor {
    pub fn new() -> Self {
        Self {
            fallback: TruncationCompressor::default(),
        }
    }
}

impl Default for SummaryCompressor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ContextCompressor for SummaryCompressor {
    async fn compress(
        &self,
        items: &[ContextItem],
        target_tokens: u32,
    ) -> Result<Vec<ContextItem>> {
        // TODO: integrate with LLM summarisation endpoint.
        debug!("SummaryCompressor: falling back to truncation");
        self.fallback.compress(items, target_tokens).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::{ContextPriority, ContextSource};

    fn make_item(key: &str, content: &str) -> ContextItem {
        ContextItem {
            key: key.to_string(),
            label: key.to_string(),
            content: content.to_string(),
            token_estimate: 0,
            priority: ContextPriority::Medium,
            source: ContextSource::UserRequest,
            pinned: false,
            relevance: 0.5,
            added_at: chrono::Utc::now(),
            added_at_step: 0,
        }
    }

    #[tokio::test]
    async fn truncation_fits_budget() {
        let compressor = TruncationCompressor::new(3);
        let long_content = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
        let items = vec![make_item("big", &long_content)];

        let result = compressor.compress(&items, 20).await.unwrap();
        assert_eq!(result.len(), 1);
        // The truncated result should be significantly smaller than the original.
        let original_tokens = TokenEstimator::estimate_tokens(&long_content);
        assert!(
            result[0].token_estimate < original_tokens,
            "truncated={} should be < original={original_tokens}",
            result[0].token_estimate
        );
    }

    #[tokio::test]
    async fn already_within_budget() {
        let compressor = TruncationCompressor::default();
        let items = vec![make_item("small", "hello world")];
        let result = compressor.compress(&items, 1000).await.unwrap();
        assert_eq!(result[0].content, "hello world");
    }
}
