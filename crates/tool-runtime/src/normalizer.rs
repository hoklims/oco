use oco_shared_types::{Observation, ObservationKind, ObservationSource, ToolResult};

/// Converts raw [`ToolResult`]s into structured [`Observation`]s suitable for
/// inclusion in the orchestrator context window.
pub struct ObservationNormalizer;

impl ObservationNormalizer {
    /// Normalize a `ToolResult` into an `Observation`.
    ///
    /// Error results produce an `ObservationKind::Error`.
    /// Successful results with a JSON object/array produce `Structured`;
    /// otherwise the output is serialized as `Text`.
    pub fn normalize(result: &ToolResult) -> Observation {
        let source = ObservationSource::ToolExecution {
            tool_name: result.tool_name.clone(),
        };

        if !result.success {
            let message = result
                .error
                .clone()
                .unwrap_or_else(|| "unknown error".to_string());
            let token_estimate = estimate_tokens(&message);
            return Observation::new(
                source,
                ObservationKind::Error {
                    message,
                    recoverable: true,
                },
                token_estimate,
            );
        }

        let kind = classify_output(&result.output);
        let token_estimate = estimate_tokens_for_value(&result.output);

        Observation::new(source, kind, token_estimate)
    }
}

/// Rough token estimate: ~4 characters per token.
fn estimate_tokens(text: &str) -> u32 {
    (text.len() as u32).div_ceil(4)
}

/// Estimate tokens from an arbitrary JSON value by serializing it.
fn estimate_tokens_for_value(value: &serde_json::Value) -> u32 {
    // For strings, use the string length directly to avoid extra quoting overhead.
    if let Some(s) = value.as_str() {
        return estimate_tokens(s);
    }
    let serialized = serde_json::to_string(value).unwrap_or_default();
    estimate_tokens(&serialized)
}

/// Classify a successful tool output into the appropriate `ObservationKind`.
fn classify_output(output: &serde_json::Value) -> ObservationKind {
    // If the output contains a "content" field with file_path / start_line, treat
    // as a code snippet.
    if let Some(obj) = output.as_object()
        && let (Some(file_path), Some(start_line), Some(content)) = (
            obj.get("file_path").and_then(|v| v.as_str()),
            obj.get("start_line").and_then(|v| v.as_u64()),
            obj.get("content").and_then(|v| v.as_str()),
        )
    {
        let end_line = obj
            .get("end_line")
            .and_then(|v| v.as_u64())
            .unwrap_or(start_line);
        let language = obj
            .get("language")
            .and_then(|v| v.as_str())
            .map(String::from);
        return ObservationKind::CodeSnippet {
            file_path: file_path.to_string(),
            start_line: start_line as u32,
            end_line: end_line as u32,
            content: content.to_string(),
            language,
        };
    }

    // Objects and arrays become Structured observations.
    if output.is_object() || output.is_array() {
        return ObservationKind::Structured {
            data: output.clone(),
        };
    }

    // Everything else (strings, numbers, booleans, null) → Text.
    let text = match output.as_str() {
        Some(s) => s.to_string(),
        None => serde_json::to_string(output).unwrap_or_default(),
    };

    ObservationKind::Text {
        content: text,
        metadata: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn success_result(output: serde_json::Value) -> ToolResult {
        ToolResult {
            tool_name: "test_tool".to_string(),
            success: true,
            output,
            error: None,
            duration_ms: 42,
        }
    }

    fn error_result(msg: &str) -> ToolResult {
        ToolResult {
            tool_name: "test_tool".to_string(),
            success: false,
            output: serde_json::Value::Null,
            error: Some(msg.to_string()),
            duration_ms: 10,
        }
    }

    #[test]
    fn error_result_normalizes_to_error_kind() {
        let obs = ObservationNormalizer::normalize(&error_result("boom"));
        match &obs.kind {
            ObservationKind::Error {
                message,
                recoverable,
            } => {
                assert_eq!(message, "boom");
                assert!(*recoverable);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn json_object_normalizes_to_structured() {
        let obs =
            ObservationNormalizer::normalize(&success_result(serde_json::json!({"key": "val"})));
        assert!(matches!(obs.kind, ObservationKind::Structured { .. }));
    }

    #[test]
    fn code_snippet_detected() {
        let output = serde_json::json!({
            "file_path": "src/main.rs",
            "start_line": 1,
            "end_line": 10,
            "content": "fn main() {}",
            "language": "rust",
        });
        let obs = ObservationNormalizer::normalize(&success_result(output));
        assert!(matches!(obs.kind, ObservationKind::CodeSnippet { .. }));
    }

    #[test]
    fn string_normalizes_to_text() {
        let obs =
            ObservationNormalizer::normalize(&success_result(serde_json::Value::String("hello".into())));
        assert!(matches!(obs.kind, ObservationKind::Text { .. }));
    }

    #[test]
    fn token_estimate_is_roughly_correct() {
        // 100 chars → ~25 tokens
        let text = "a".repeat(100);
        assert_eq!(estimate_tokens(&text), 25);
    }
}
