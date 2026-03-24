//! Best-effort heuristic scanner for embedded secrets in command strings.
//!
//! **This is NOT a security boundary.** It catches common patterns (API keys,
//! tokens, private key headers, connection strings) in plaintext form only.
//! It can be bypassed via string concatenation, encoding, shell interpolation,
//! here-docs, or other obfuscation. Use as a safety net, not a guarantee.
//!
//! For actual secret management, use environment variables or a vault.

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Result of scanning a string for secrets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretScanResult {
    /// Whether any secrets were detected.
    pub has_secrets: bool,
    /// Individual matches with redacted previews.
    pub matches: Vec<SecretMatch>,
    /// The input string with all secrets redacted.
    pub redacted: String,
}

/// A single detected secret.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretMatch {
    /// Which pattern matched (human-readable label).
    pub pattern_name: String,
    /// Redacted preview: first 4 + last 4 chars visible.
    pub redacted_preview: String,
    /// Byte offset in the original string.
    pub offset: usize,
}

/// Compiled secret patterns — initialized once, reused across all scans.
static SECRET_PATTERNS: OnceLock<Vec<SecretPattern>> = OnceLock::new();

struct SecretPattern {
    name: &'static str,
    regex: Regex,
}

fn init_patterns() -> Vec<SecretPattern> {
    let defs: &[(&str, &str)] = &[
        // API keys with common prefixes
        ("AWS Access Key", r"\bAKIA[0-9A-Z]{16}\b"),
        (
            "AWS Secret Key",
            r"(?i)\baws[_\-]?secret[_\-]?access[_\-]?key\s*[=:]\s*[A-Za-z0-9/+=]{40}\b",
        ),
        (
            "GitHub Token",
            r"\b(ghp|gho|ghu|ghs|ghr)_[A-Za-z0-9_]{36,255}\b",
        ),
        ("GitLab Token", r"\bglpat-[A-Za-z0-9\-_]{20,}\b"),
        ("Slack Token", r"\bxox[bpors]-[A-Za-z0-9\-]{10,}\b"),
        // Generic API key patterns (high-entropy hex/base64 after key= or token=)
        (
            "Generic API Key",
            r#"(?i)\b(?:api[_\-]?key|api[_\-]?secret|access[_\-]?token)\s*[=:]\s*['"]?([A-Za-z0-9/+=\-_]{20,})['"]?"#,
        ),
        // Private keys
        (
            "Private Key",
            r"-----BEGIN (?:RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
        ),
        // JWT tokens
        (
            "JWT Token",
            r"\beyJ[A-Za-z0-9\-_]+\.eyJ[A-Za-z0-9\-_]+\.[A-Za-z0-9\-_.+/=]+\b",
        ),
        // Anthropic / OpenAI keys
        ("Anthropic API Key", r"\bsk-ant-[A-Za-z0-9\-_]{20,}\b"),
        // OpenAI keys: require sk-proj- or sk-org- prefix to avoid false positives
        (
            "OpenAI API Key",
            r"\bsk-(?:proj|org|svcacct)-[A-Za-z0-9\-_]{20,}\b",
        ),
        // Connection strings
        (
            "Connection String",
            r#"(?i)(?:mongodb|postgres|mysql|redis|amqp)://[^\s'"]+@[^\s'"]+"#,
        ),
    ];

    defs.iter()
        .map(|(name, pattern)| {
            let regex = Regex::new(pattern).expect("failed to compile secret detection regex");
            SecretPattern { name, regex }
        })
        .collect()
}

/// Scan a string for embedded secrets.
///
/// Returns a `SecretScanResult` with redacted output and match details.
/// Thread-safe: patterns are compiled once via `OnceLock`.
pub fn scan_secrets(input: &str) -> SecretScanResult {
    let patterns = SECRET_PATTERNS.get_or_init(init_patterns);
    let mut matches = Vec::new();
    let mut redacted = input.to_string();

    for pattern in patterns {
        for m in pattern.regex.find_iter(input) {
            let matched = m.as_str();
            let preview = redact_preview(matched);
            matches.push(SecretMatch {
                pattern_name: pattern.name.to_string(),
                redacted_preview: preview,
                offset: m.start(),
            });
            redacted = redacted.replace(matched, &redact_full(matched));
        }
    }

    SecretScanResult {
        has_secrets: !matches.is_empty(),
        matches,
        redacted,
    }
}

/// Redact a secret: show first 4 and last 4 chars, mask the middle.
fn redact_preview(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "*".repeat(chars.len());
    }
    let prefix: String = chars[..4].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}{}{suffix}", "*".repeat(chars.len() - 8))
}

/// Full redaction for replacement in the redacted output.
fn redact_full(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "[REDACTED]".to_string();
    }
    let prefix: String = chars[..4].iter().collect();
    let suffix: String = chars[chars.len() - 4..].iter().collect();
    format!("{prefix}[…REDACTED…]{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_aws_access_key() {
        let input = "export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE";
        let result = scan_secrets(input);
        assert!(result.has_secrets);
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].pattern_name, "AWS Access Key");
        assert!(result.redacted.contains("[…REDACTED…]"));
    }

    #[test]
    fn detects_github_token() {
        let input = "token: ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmn";
        let result = scan_secrets(input);
        assert!(result.has_secrets);
        assert_eq!(result.matches[0].pattern_name, "GitHub Token");
    }

    #[test]
    fn detects_private_key_header() {
        let input = "-----BEGIN RSA PRIVATE KEY-----\nMIIEow...";
        let result = scan_secrets(input);
        assert!(result.has_secrets);
        assert_eq!(result.matches[0].pattern_name, "Private Key");
    }

    #[test]
    fn detects_anthropic_key() {
        let input = "ANTHROPIC_API_KEY=sk-ant-api03-abcdefghijklmnopqrstuvwx";
        let result = scan_secrets(input);
        assert!(result.has_secrets);
        assert_eq!(result.matches[0].pattern_name, "Anthropic API Key");
    }

    #[test]
    fn clean_input_no_secrets() {
        let input = "cargo build --release && echo 'hello world'";
        let result = scan_secrets(input);
        assert!(!result.has_secrets);
        assert!(result.matches.is_empty());
        assert_eq!(result.redacted, input);
    }

    #[test]
    fn detects_connection_string() {
        let input = "DATABASE_URL=postgres://user:password@localhost:5432/mydb";
        let result = scan_secrets(input);
        assert!(result.has_secrets);
        assert_eq!(result.matches[0].pattern_name, "Connection String");
    }

    #[test]
    fn redaction_preserves_prefix_suffix() {
        let preview = redact_preview("AKIAIOSFODNN7EXAMPLE");
        assert!(preview.starts_with("AKIA"));
        assert!(preview.ends_with("MPLE"));
        assert!(preview.contains("*"));
    }

    #[test]
    fn multiple_secrets_in_one_string() {
        let input = "KEY=AKIAIOSFODNN7EXAMPLE TOKEN=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmn";
        let result = scan_secrets(input);
        assert!(result.has_secrets);
        assert!(result.matches.len() >= 2);
    }
}
