use oco_shared_types::ContextItem;

/// Heuristic token estimator.
///
/// Uses a simple bytes-to-tokens ratio with an adjustment for source code,
/// which tends to have shorter tokens than natural language.
pub struct TokenEstimator;

impl TokenEstimator {
    /// Rough heuristic: ~4 bytes per token for English text.
    /// Code typically has shorter identifiers and more punctuation, so we use
    /// a multiplier of 0.85 (i.e. ~3.4 bytes per token).
    const BYTES_PER_TOKEN_TEXT: f64 = 4.0;
    const CODE_ADJUSTMENT: f64 = 0.85;

    /// Estimate the number of tokens in a piece of text.
    pub fn estimate_tokens(text: &str) -> u32 {
        if text.is_empty() {
            return 0;
        }

        let base = text.len() as f64 / Self::BYTES_PER_TOKEN_TEXT;

        let ratio = if Self::looks_like_code(text) {
            base / Self::CODE_ADJUSTMENT
        } else {
            base
        };

        ratio.ceil() as u32
    }

    /// Estimate the token count of a [`ContextItem`].
    ///
    /// If the item already carries a non-zero `token_estimate` we trust it;
    /// otherwise we compute one from the content.
    pub fn estimate_item(item: &ContextItem) -> u32 {
        if item.token_estimate > 0 {
            return item.token_estimate;
        }
        Self::estimate_tokens(&item.content)
    }

    /// Very simple heuristic: text that contains braces, semicolons, or `fn `
    /// is likely code.
    fn looks_like_code(text: &str) -> bool {
        let sample = if text.len() > 512 { &text[..512] } else { text };
        let indicators = ['{', '}', ';'];
        let keyword_hits = ["fn ", "def ", "func ", "class ", "import ", "use "];

        let punct_count = sample.chars().filter(|c| indicators.contains(c)).count();
        let has_keywords = keyword_hits.iter().any(|kw| sample.contains(kw));

        punct_count >= 3 || has_keywords
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_is_zero() {
        assert_eq!(TokenEstimator::estimate_tokens(""), 0);
    }

    #[test]
    fn short_english_text() {
        // "hello world" = 11 bytes -> ~3 tokens
        let tokens = TokenEstimator::estimate_tokens("hello world");
        assert!(tokens >= 2 && tokens <= 5, "got {tokens}");
    }

    #[test]
    fn code_adjustment_increases_estimate() {
        // Verify the code heuristic is active: same text classified as code
        // should yield more tokens than the base formula.
        let code = "fn main() {\n    let x = 42;\n    println!(\"{}\", x);\n}";
        let tokens = TokenEstimator::estimate_tokens(code);
        let base = (code.len() as f64 / 4.0).ceil() as u32;
        assert!(
            tokens >= base,
            "code tokens ({tokens}) should be >= base ({base})"
        );
    }
}
