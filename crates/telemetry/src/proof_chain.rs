//! HMAC-SHA256 tamper-evident proof chain for decision audit trails.
//!
//! Each `ProofEnvelope` links to the previous via hash chaining over the
//! **full envelope hash** (not just content_hash). The HMAC signature covers
//! all immutable fields: sequence, timestamp, content_hash, previous_envelope_hash,
//! tool_call_ids, and label. Modifying any field invalidates the signature.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// A single envelope in the proof chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofEnvelope {
    /// Sequence number (0-based).
    pub sequence: u64,
    /// SHA-256 hash of the canonical content.
    pub content_hash: String,
    /// Hash of the **previous envelope** (all fields). Genesis = 64 zeros.
    pub previous_envelope_hash: String,
    /// ISO 8601 timestamp (set before signing, included in signature).
    pub timestamp: String,
    /// Optional: IDs of tool calls included in this step.
    #[serde(default)]
    pub tool_call_ids: Vec<String>,
    /// Optional: human-readable label.
    #[serde(default)]
    pub label: Option<String>,
    /// HMAC-SHA256 signature over the canonical representation of all fields above.
    pub signature: String,
}

/// The proof chain — append-only, hash-linked.
pub struct ProofChain {
    /// HMAC key for signing envelopes.
    key: Vec<u8>,
    /// Ordered list of envelopes.
    envelopes: Vec<ProofEnvelope>,
}

/// Genesis hash: 64 zeros (sentinel value for the first envelope).
const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

impl ProofChain {
    /// Create a new proof chain with the given HMAC signing key.
    pub fn new(key: &[u8]) -> Self {
        Self {
            key: key.to_vec(),
            envelopes: Vec::new(),
        }
    }

    /// Append a new entry to the chain.
    ///
    /// `content` is the canonical JSON (or any deterministic string)
    /// representing the decision/action at this step.
    pub fn append(
        &mut self,
        content: &str,
        tool_call_ids: Vec<String>,
        label: Option<String>,
    ) -> &ProofEnvelope {
        let previous_envelope_hash = self
            .envelopes
            .last()
            .map(Self::envelope_hash)
            .unwrap_or_else(|| GENESIS_HASH.to_string());

        let content_hash = sha256_hex(content.as_bytes());
        // Timestamp is set BEFORE signing so it's covered by the HMAC.
        let timestamp = chrono::Utc::now().to_rfc3339();

        let signature = self.sign_envelope(
            self.envelopes.len() as u64,
            &content_hash,
            &previous_envelope_hash,
            &timestamp,
            &tool_call_ids,
            label.as_deref(),
        );

        let envelope = ProofEnvelope {
            sequence: self.envelopes.len() as u64,
            content_hash,
            previous_envelope_hash,
            timestamp,
            tool_call_ids,
            label,
            signature,
        };

        self.envelopes.push(envelope);
        self.envelopes.last().expect("just pushed")
    }

    /// Verify the entire chain: check HMAC signatures and hash linkage.
    ///
    /// Returns `Ok(())` if valid, `Err` with the first broken envelope index.
    pub fn verify(&self) -> Result<(), ProofChainError> {
        for (i, envelope) in self.envelopes.iter().enumerate() {
            // Check sequence
            if envelope.sequence != i as u64 {
                return Err(ProofChainError::SequenceMismatch {
                    expected: i as u64,
                    actual: envelope.sequence,
                });
            }

            // Check previous envelope hash linkage
            let expected_prev = if i == 0 {
                GENESIS_HASH.to_string()
            } else {
                Self::envelope_hash(&self.envelopes[i - 1])
            };
            if envelope.previous_envelope_hash != expected_prev {
                return Err(ProofChainError::BrokenLink { index: i });
            }

            // Check HMAC signature covers all fields
            let expected_sig = self.sign_envelope(
                envelope.sequence,
                &envelope.content_hash,
                &envelope.previous_envelope_hash,
                &envelope.timestamp,
                &envelope.tool_call_ids,
                envelope.label.as_deref(),
            );
            if envelope.signature != expected_sig {
                return Err(ProofChainError::InvalidSignature { index: i });
            }
        }

        Ok(())
    }

    /// Get all envelopes.
    pub fn envelopes(&self) -> &[ProofEnvelope] {
        &self.envelopes
    }

    /// Number of entries in the chain.
    pub fn len(&self) -> usize {
        self.envelopes.len()
    }

    /// Check if the chain is empty.
    pub fn is_empty(&self) -> bool {
        self.envelopes.is_empty()
    }

    /// Export the chain as JSONL (one envelope per line).
    pub fn export_jsonl(&self) -> String {
        self.envelopes
            .iter()
            .filter_map(|e| serde_json::to_string(e).ok())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Compute a SHA-256 hash of all non-signature fields of an envelope.
    /// This is the value used for `previous_envelope_hash` linkage.
    fn envelope_hash(envelope: &ProofEnvelope) -> String {
        let canonical = serde_json::json!({
            "sequence": envelope.sequence,
            "content_hash": envelope.content_hash,
            "previous_envelope_hash": envelope.previous_envelope_hash,
            "timestamp": envelope.timestamp,
            "tool_call_ids": envelope.tool_call_ids,
            "label": envelope.label,
        });
        sha256_hex(canonical.to_string().as_bytes())
    }

    /// Compute HMAC-SHA256 over all immutable fields of an envelope.
    fn sign_envelope(
        &self,
        sequence: u64,
        content_hash: &str,
        previous_envelope_hash: &str,
        timestamp: &str,
        tool_call_ids: &[String],
        label: Option<&str>,
    ) -> String {
        let canonical = serde_json::json!({
            "sequence": sequence,
            "content_hash": content_hash,
            "previous_envelope_hash": previous_envelope_hash,
            "timestamp": timestamp,
            "tool_call_ids": tool_call_ids,
            "label": label,
        });
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC accepts any key length");
        mac.update(canonical.to_string().as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }
}

/// SHA-256 hash as hex string.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let hash = Sha256::digest(data);
    hex::encode(hash)
}

/// Errors during proof chain verification.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ProofChainError {
    #[error("sequence mismatch at envelope: expected {expected}, got {actual}")]
    SequenceMismatch { expected: u64, actual: u64 },
    #[error("broken hash link at envelope index {index}")]
    BrokenLink { index: usize },
    #[error("invalid HMAC signature at envelope index {index}")]
    InvalidSignature { index: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_chain_verifies() {
        let chain = ProofChain::new(b"test-key");
        assert!(chain.verify().is_ok());
        assert!(chain.is_empty());
    }

    #[test]
    fn single_entry_chain() {
        let mut chain = ProofChain::new(b"test-key");
        let envelope = chain.append(r#"{"action":"retrieve"}"#, vec![], None);
        assert_eq!(envelope.sequence, 0);
        assert_eq!(envelope.previous_envelope_hash, GENESIS_HASH);
        assert!(chain.verify().is_ok());
    }

    #[test]
    fn multi_entry_chain_links_correctly() {
        let mut chain = ProofChain::new(b"test-key");
        chain.append("step-0", vec![], Some("retrieve".into()));
        chain.append("step-1", vec!["tool_1".into()], Some("tool_call".into()));
        chain.append("step-2", vec![], Some("respond".into()));

        assert_eq!(chain.len(), 3);
        assert!(chain.verify().is_ok());

        // Each envelope links to the full hash of the previous envelope
        let hash_0 = ProofChain::envelope_hash(&chain.envelopes[0]);
        let hash_1 = ProofChain::envelope_hash(&chain.envelopes[1]);
        assert_eq!(chain.envelopes[1].previous_envelope_hash, hash_0);
        assert_eq!(chain.envelopes[2].previous_envelope_hash, hash_1);
    }

    #[test]
    fn tampered_content_detected() {
        let mut chain = ProofChain::new(b"test-key");
        chain.append("step-0", vec![], None);
        chain.append("step-1", vec![], None);

        // Tamper with the first envelope's content hash
        chain.envelopes[0].content_hash = "deadbeef".repeat(8);

        let result = chain.verify();
        assert!(result.is_err());
    }

    #[test]
    fn tampered_signature_detected() {
        let mut chain = ProofChain::new(b"test-key");
        chain.append("step-0", vec![], None);

        chain.envelopes[0].signature = "bad_signature".to_string();

        assert!(matches!(
            chain.verify(),
            Err(ProofChainError::InvalidSignature { index: 0 })
        ));
    }

    #[test]
    fn tampered_timestamp_detected() {
        let mut chain = ProofChain::new(b"test-key");
        chain.append("step-0", vec![], None);

        // Modify the timestamp — should break the signature
        chain.envelopes[0].timestamp = "2020-01-01T00:00:00Z".to_string();

        assert!(matches!(
            chain.verify(),
            Err(ProofChainError::InvalidSignature { index: 0 })
        ));
    }

    #[test]
    fn tampered_label_detected() {
        let mut chain = ProofChain::new(b"test-key");
        chain.append("step-0", vec![], Some("original".into()));

        chain.envelopes[0].label = Some("tampered".into());

        assert!(matches!(
            chain.verify(),
            Err(ProofChainError::InvalidSignature { index: 0 })
        ));
    }

    #[test]
    fn tampered_tool_call_ids_detected() {
        let mut chain = ProofChain::new(b"test-key");
        chain.append("step-0", vec!["tool_1".into()], None);

        chain.envelopes[0].tool_call_ids = vec!["fake_tool".into()];

        assert!(matches!(
            chain.verify(),
            Err(ProofChainError::InvalidSignature { index: 0 })
        ));
    }

    #[test]
    fn different_keys_produce_different_signatures() {
        let mut chain_a = ProofChain::new(b"key-a");
        let mut chain_b = ProofChain::new(b"key-b");

        chain_a.append("same content", vec![], None);
        chain_b.append("same content", vec![], None);

        assert_ne!(
            chain_a.envelopes[0].signature,
            chain_b.envelopes[0].signature
        );
        // Same content hash though
        assert_eq!(
            chain_a.envelopes[0].content_hash,
            chain_b.envelopes[0].content_hash
        );
    }

    #[test]
    fn export_jsonl_produces_valid_lines() {
        let mut chain = ProofChain::new(b"test-key");
        chain.append("step-0", vec![], None);
        chain.append("step-1", vec![], None);

        let jsonl = chain.export_jsonl();
        let lines: Vec<&str> = jsonl.lines().collect();
        assert_eq!(lines.len(), 2);

        for line in lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.get("content_hash").is_some());
            assert!(parsed.get("previous_envelope_hash").is_some());
            assert!(parsed.get("signature").is_some());
        }
    }

    #[test]
    fn broken_link_detected() {
        let mut chain = ProofChain::new(b"test-key");
        chain.append("step-0", vec![], None);
        chain.append("step-1", vec![], None);

        chain.envelopes[1].previous_envelope_hash = "wrong_hash".to_string();

        assert!(matches!(
            chain.verify(),
            Err(ProofChainError::BrokenLink { index: 1 })
        ));
    }
}
