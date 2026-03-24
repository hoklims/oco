use std::collections::HashMap;

use oco_shared_types::{ContextItem, ContextSource};
use tracing::debug;

/// Deduplicates and merges overlapping context items.
pub struct ContextDeduplicator;

impl ContextDeduplicator {
    /// Remove exact duplicates by key and merge overlapping code snippets
    /// from the same file.
    pub fn deduplicate(items: Vec<ContextItem>) -> Vec<ContextItem> {
        // Phase 1: exact dedup by key.
        let mut seen = HashMap::<String, usize>::new();
        let mut deduped: Vec<ContextItem> = Vec::with_capacity(items.len());

        for item in items {
            if let Some(&idx) = seen.get(&item.key) {
                // Keep the one with higher relevance.
                if item.relevance > deduped[idx].relevance {
                    deduped[idx] = item;
                }
            } else {
                seen.insert(item.key.clone(), deduped.len());
                deduped.push(item);
            }
        }

        let before = deduped.len();

        // Phase 2: merge overlapping file snippets.
        let merged = Self::merge_file_snippets(deduped);

        let after = merged.len();
        if before != after {
            debug!(before, after, "merged overlapping file snippets");
        }

        merged
    }

    /// Group items by file path, detect overlapping line ranges, and merge them.
    fn merge_file_snippets(items: Vec<ContextItem>) -> Vec<ContextItem> {
        // Separate file-content items from the rest.
        let mut file_items: HashMap<String, Vec<ContextItem>> = HashMap::new();
        let mut other_items: Vec<ContextItem> = Vec::new();

        for item in items {
            if let ContextSource::FileContent { ref path } = item.source {
                file_items
                    .entry(path.clone())
                    .or_default()
                    .push(item);
            } else {
                other_items.push(item);
            }
        }

        // For each file, try to merge overlapping snippets.
        for snippets in file_items.values_mut() {
            if snippets.len() <= 1 {
                other_items.append(snippets);
                continue;
            }

            let merged = Self::merge_snippets_for_file(std::mem::take(snippets));
            other_items.extend(merged);
        }

        other_items
    }

    /// Given multiple snippets from the same file, merge those whose line ranges overlap.
    fn merge_snippets_for_file(mut snippets: Vec<ContextItem>) -> Vec<ContextItem> {
        // Parse line ranges from content (lines are numbered or we use content length).
        // We treat each snippet's content as a block of lines and detect overlaps
        // by comparing the actual text lines.

        if snippets.len() <= 1 {
            return snippets;
        }

        // Sort by content length descending so the largest snippet comes first.
        snippets.sort_by_key(|s| std::cmp::Reverse(s.content.len()));

        let mut result: Vec<ContextItem> = Vec::new();

        for snippet in snippets {
            let snippet_lines: Vec<&str> = snippet.content.lines().collect();
            let mut merged = false;

            for existing in &mut result {
                let existing_lines: Vec<&str> = existing.content.lines().collect();

                if let Some(merged_content) =
                    Self::try_merge_lines(&existing_lines, &snippet_lines)
                {
                    existing.content = merged_content;
                    existing.token_estimate = 0; // will be re-estimated
                    // Take the higher relevance and priority.
                    if snippet.relevance > existing.relevance {
                        existing.relevance = snippet.relevance;
                    }
                    if snippet.priority > existing.priority {
                        existing.priority = snippet.priority;
                    }
                    existing.label = format!("{} (merged)", existing.label);
                    merged = true;
                    break;
                }
            }

            if !merged {
                result.push(snippet);
            }
        }

        result
    }

    /// Try to merge two sets of lines if they overlap.
    /// Returns `Some(merged)` if at least `MIN_OVERLAP` consecutive lines match.
    fn try_merge_lines(a_lines: &[&str], b_lines: &[&str]) -> Option<String> {
        const MIN_OVERLAP: usize = 2;

        if a_lines.is_empty() || b_lines.is_empty() {
            return None;
        }

        // Check if b's start overlaps with a's tail.
        // Find the first line of b in a.
        for start in 0..a_lines.len() {
            if a_lines[start] == b_lines[0] {
                // Check how many consecutive lines match.
                let mut match_len = 0;
                while start + match_len < a_lines.len()
                    && match_len < b_lines.len()
                    && a_lines[start + match_len] == b_lines[match_len]
                {
                    match_len += 1;
                }

                if match_len >= MIN_OVERLAP {
                    // Merge: a[..start] + b (which extends from the overlap point).
                    let mut merged = a_lines[..start].to_vec();
                    merged.extend_from_slice(b_lines);
                    return Some(merged.join("\n"));
                }
            }
        }

        // Check the reverse: a's start overlaps with b's tail.
        for start in 0..b_lines.len() {
            if b_lines[start] == a_lines[0] {
                let mut match_len = 0;
                while start + match_len < b_lines.len()
                    && match_len < a_lines.len()
                    && b_lines[start + match_len] == a_lines[match_len]
                {
                    match_len += 1;
                }

                if match_len >= MIN_OVERLAP {
                    let mut merged = b_lines[..start].to_vec();
                    merged.extend_from_slice(a_lines);
                    return Some(merged.join("\n"));
                }
            }
        }

        // Check if one is a complete subset of the other.
        let a_text = a_lines.join("\n");
        let b_text = b_lines.join("\n");
        if a_text.contains(&b_text) {
            return Some(a_text);
        }
        if b_text.contains(&a_text) {
            return Some(b_text);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::{ContextPriority, ContextSource};

    fn file_item(key: &str, path: &str, content: &str, relevance: f64) -> ContextItem {
        ContextItem {
            key: key.to_string(),
            label: key.to_string(),
            content: content.to_string(),
            token_estimate: 0,
            priority: ContextPriority::Medium,
            source: ContextSource::FileContent {
                path: path.to_string(),
            },
            pinned: false,
            relevance,
            added_at: chrono::Utc::now(),
            added_at_step: 0,
        }
    }

    fn plain_item(key: &str, relevance: f64) -> ContextItem {
        ContextItem {
            key: key.to_string(),
            label: key.to_string(),
            content: "some content".to_string(),
            token_estimate: 0,
            priority: ContextPriority::Medium,
            source: ContextSource::UserRequest,
            pinned: false,
            relevance,
            added_at: chrono::Utc::now(),
            added_at_step: 0,
        }
    }

    #[test]
    fn removes_exact_duplicates_keeping_higher_relevance() {
        let items = vec![plain_item("a", 0.3), plain_item("a", 0.9)];
        let result = ContextDeduplicator::deduplicate(items);
        assert_eq!(result.len(), 1);
        assert!((result[0].relevance - 0.9).abs() < f64::EPSILON);
    }

    #[test]
    fn merges_overlapping_snippets() {
        let a = file_item("a", "src/lib.rs", "line1\nline2\nline3\nline4", 0.5);
        let b = file_item("b", "src/lib.rs", "line3\nline4\nline5\nline6", 0.5);
        let result = ContextDeduplicator::deduplicate(vec![a, b]);
        assert_eq!(result.len(), 1);
        assert!(result[0].content.contains("line1"));
        assert!(result[0].content.contains("line6"));
    }

    #[test]
    fn does_not_merge_non_overlapping() {
        let a = file_item("a", "src/lib.rs", "aaa\nbbb", 0.5);
        let b = file_item("b", "src/lib.rs", "ccc\nddd", 0.5);
        let result = ContextDeduplicator::deduplicate(vec![a, b]);
        assert_eq!(result.len(), 2);
    }
}
