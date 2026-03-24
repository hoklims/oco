use oco_shared_types::{Observation, ObservationKind, TaskComplexity};

/// Heuristic estimator for knowledge boundary confidence.
///
/// Produces a score from 0.0 (very uncertain) to 1.0 (very confident)
/// based on task characteristics and orchestration state.
pub struct KnowledgeBoundaryEstimator;

impl KnowledgeBoundaryEstimator {
    /// Estimate confidence that the model can handle this task
    /// within its knowledge boundary.
    ///
    /// Factors:
    /// - **Task complexity**: simpler tasks get higher base confidence
    /// - **Specific file paths**: presence of concrete paths suggests grounded context
    /// - **Retrieval done**: having retrieved context boosts confidence
    /// - **Error count**: recent errors reduce confidence
    /// - **Observation quality**: relevant, high-scoring observations boost confidence
    pub fn estimate(
        task_complexity: TaskComplexity,
        request: &str,
        observations: &[Observation],
        has_retrieved: bool,
        workspace_signals: &[String],
    ) -> f64 {
        let base = Self::base_confidence(task_complexity);
        let path_boost = Self::file_path_boost(request, workspace_signals);
        let retrieval_boost = Self::retrieval_boost(has_retrieved, task_complexity);
        let error_penalty = Self::error_penalty(observations);
        let observation_boost = Self::observation_quality_boost(observations);
        let specificity_boost = Self::request_specificity_boost(request);

        let raw = base + path_boost + retrieval_boost + observation_boost + specificity_boost
            - error_penalty;

        // Clamp to [0.0, 1.0]
        raw.clamp(0.0, 1.0)
    }

    /// Base confidence from task complexity alone.
    fn base_confidence(complexity: TaskComplexity) -> f64 {
        match complexity {
            TaskComplexity::Trivial => 0.85,
            TaskComplexity::Low => 0.70,
            TaskComplexity::Medium => 0.50,
            TaskComplexity::High => 0.30,
            TaskComplexity::Critical => 0.15,
        }
    }

    /// Boost if the request or signals contain specific file paths.
    /// Specific paths mean we have grounded information to work with.
    fn file_path_boost(request: &str, signals: &[String]) -> f64 {
        let combined = format!("{} {}", request, signals.join(" "));

        let path_count = combined
            .split_whitespace()
            .filter(|word| {
                (word.contains('/') || word.contains('\\')) && word.contains('.') && word.len() > 3
            })
            .count();

        match path_count {
            0 => 0.0,
            1 => 0.05,
            2..=3 => 0.10,
            _ => 0.15,
        }
    }

    /// Boost from having performed retrieval.
    fn retrieval_boost(has_retrieved: bool, complexity: TaskComplexity) -> f64 {
        if !has_retrieved {
            return 0.0;
        }
        // Retrieval matters more for complex tasks
        match complexity {
            TaskComplexity::Trivial => 0.02,
            TaskComplexity::Low => 0.05,
            TaskComplexity::Medium => 0.15,
            TaskComplexity::High => 0.25,
            TaskComplexity::Critical => 0.30,
        }
    }

    /// Penalty from recent errors in observations.
    fn error_penalty(observations: &[Observation]) -> f64 {
        let recent = observations.iter().rev().take(10);
        let error_count = recent
            .filter(|o| matches!(o.kind, ObservationKind::Error { .. }))
            .count();

        match error_count {
            0 => 0.0,
            1 => 0.10,
            2 => 0.20,
            3 => 0.30,
            _ => 0.40,
        }
    }

    /// Boost from high-quality observations (high relevance scores, code snippets).
    fn observation_quality_boost(observations: &[Observation]) -> f64 {
        if observations.is_empty() {
            return 0.0;
        }

        let mut boost = 0.0;

        // Count observations with high relevance
        let high_relevance_count = observations
            .iter()
            .filter(|o| o.relevance_score.unwrap_or(0.0) > 0.7)
            .count();

        boost += (high_relevance_count as f64 * 0.03).min(0.15);

        // Code snippets are grounding: they provide concrete context
        let code_snippet_count = observations
            .iter()
            .filter(|o| matches!(o.kind, ObservationKind::CodeSnippet { .. }))
            .count();

        boost += (code_snippet_count as f64 * 0.02).min(0.10);

        // Successful verification results boost confidence significantly
        let passed_verifications = observations
            .iter()
            .filter(|o| {
                matches!(
                    o.kind,
                    ObservationKind::VerificationResult { passed: true, .. }
                )
            })
            .count();

        boost += (passed_verifications as f64 * 0.05).min(0.15);

        boost.min(0.25)
    }

    /// Boost from request specificity.
    /// More specific requests (containing identifiers, line numbers, exact terms)
    /// suggest the user knows what they want, improving effective confidence.
    fn request_specificity_boost(request: &str) -> f64 {
        let mut score: f64 = 0.0;

        // Contains line numbers (e.g. "line 42", "L42", ":42")
        let has_line_numbers = request
            .split_whitespace()
            .any(|w| w.starts_with("line") || w.starts_with('L') || w.starts_with(':'))
            && request.chars().any(|c| c.is_ascii_digit());

        if has_line_numbers {
            score += 0.05;
        }

        // Contains function/type names (CamelCase or snake_case identifiers)
        let has_identifiers = request.split_whitespace().any(|w| {
            w.len() > 3
                && !w.chars().all(|c| c.is_lowercase() || c.is_whitespace())
                && (w.contains('_')
                    || w.chars()
                        .zip(w.chars().skip(1))
                        .any(|(a, b)| a.is_lowercase() && b.is_uppercase()))
        });

        if has_identifiers {
            score += 0.05;
        }

        // Contains backtick-quoted code
        if request.contains('`') {
            score += 0.03;
        }

        score.min(0.10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivial_task_high_confidence() {
        let confidence = KnowledgeBoundaryEstimator::estimate(
            TaskComplexity::Trivial,
            "what is a mutex?",
            &[],
            false,
            &[],
        );
        assert!(
            confidence >= 0.80,
            "trivial task should have high base confidence, got {}",
            confidence
        );
    }

    #[test]
    fn critical_task_low_confidence() {
        let confidence = KnowledgeBoundaryEstimator::estimate(
            TaskComplexity::Critical,
            "redesign the entire authentication system",
            &[],
            false,
            &[],
        );
        assert!(
            confidence <= 0.30,
            "critical task without retrieval should have low confidence, got {}",
            confidence
        );
    }

    #[test]
    fn retrieval_boosts_confidence() {
        let without = KnowledgeBoundaryEstimator::estimate(
            TaskComplexity::High,
            "refactor the parser module",
            &[],
            false,
            &[],
        );
        let with = KnowledgeBoundaryEstimator::estimate(
            TaskComplexity::High,
            "refactor the parser module",
            &[],
            true,
            &[],
        );
        assert!(
            with > without,
            "retrieval should boost confidence: {} vs {}",
            with,
            without
        );
    }

    #[test]
    fn file_paths_boost_confidence() {
        let without = KnowledgeBoundaryEstimator::estimate(
            TaskComplexity::Medium,
            "fix the bug",
            &[],
            false,
            &[],
        );
        let with = KnowledgeBoundaryEstimator::estimate(
            TaskComplexity::Medium,
            "fix the bug in src/parser/mod.rs",
            &[],
            false,
            &["src/lib.rs changed".to_string()],
        );
        assert!(
            with > without,
            "file paths should boost confidence: {} vs {}",
            with,
            without
        );
    }

    #[test]
    fn errors_reduce_confidence() {
        use oco_shared_types::ObservationSource;

        let error_obs = Observation::new(
            ObservationSource::ToolExecution {
                tool_name: "shell".to_string(),
            },
            ObservationKind::Error {
                message: "compilation failed".to_string(),
                recoverable: true,
            },
            50,
        );

        let without = KnowledgeBoundaryEstimator::estimate(
            TaskComplexity::Medium,
            "fix the tests",
            &[],
            true,
            &[],
        );
        let with_errors = KnowledgeBoundaryEstimator::estimate(
            TaskComplexity::Medium,
            "fix the tests",
            &[error_obs.clone(), error_obs.clone(), error_obs],
            true,
            &[],
        );
        assert!(
            with_errors < without,
            "errors should reduce confidence: {} vs {}",
            with_errors,
            without
        );
    }

    #[test]
    fn confidence_clamped() {
        // Even with all boosts, shouldn't exceed 1.0
        let confidence = KnowledgeBoundaryEstimator::estimate(
            TaskComplexity::Trivial,
            "explain `MyStruct` in src/lib.rs line 42",
            &[],
            true,
            &["src/lib.rs exists".to_string()],
        );
        assert!(confidence <= 1.0);
        assert!(confidence >= 0.0);
    }
}
