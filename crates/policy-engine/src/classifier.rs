use oco_shared_types::TaskComplexity;

/// Deterministic task classifier using keyword heuristics.
///
/// Analyzes the user request and workspace signals to estimate
/// task complexity without any LLM call.
pub struct TaskClassifier;

/// Keywords that signal trivial tasks (direct lookup / single fact).
const TRIVIAL_KEYWORDS: &[&str] = &[
    "what is",
    "explain",
    "describe",
    "define",
    "show me",
    "list",
    "print",
    "display",
    "version",
    "help",
];

/// Keywords that signal low complexity (single-step, single-file).
const LOW_KEYWORDS: &[&str] = &[
    "rename",
    "format",
    "add comment",
    "add import",
    "update version",
    "change name",
    "typo",
    "spelling",
    "fix typo",
    "remove unused",
];

/// Keywords that signal medium complexity (multi-step, may need retrieval).
const MEDIUM_KEYWORDS: &[&str] = &[
    "implement",
    "create",
    "add feature",
    "write tests",
    "add tests",
    "fix bug",
    "update",
    "modify",
    "change",
    "convert",
    "migrate",
    "extract",
    "move",
    "split",
];

/// Keywords that signal high complexity (multi-file, needs deep context).
const HIGH_KEYWORDS: &[&str] = &[
    "refactor",
    "debug",
    "optimize",
    "redesign",
    "rewrite",
    "performance",
    "investigate",
    "diagnose",
    "trace",
    "integrate",
    "multi-file",
    "across files",
    "cross-cutting",
];

/// Keywords that signal critical complexity (architectural, large-scale).
const CRITICAL_KEYWORDS: &[&str] = &[
    "architect",
    "large-scale",
    "entire codebase",
    "system design",
    "breaking change",
    "major refactor",
    "overhaul",
    "redesign architecture",
    "migration strategy",
    "cross-service",
];

impl TaskClassifier {
    /// Classify the complexity of a task based on heuristic keyword analysis.
    ///
    /// The algorithm:
    /// 1. Normalize the request to lowercase.
    /// 2. Score against each complexity tier's keyword list.
    /// 3. Factor in workspace signals (e.g. number of files mentioned, error indicators).
    /// 4. Return the highest matching tier (with tie-breaking toward higher complexity).
    pub fn classify(request: &str, workspace_signals: &[String]) -> TaskComplexity {
        let lower = request.to_lowercase();
        let signal_text: String = workspace_signals
            .iter()
            .map(|s| s.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        let combined = format!("{} {}", lower, signal_text);

        let critical_score = Self::keyword_score(&combined, CRITICAL_KEYWORDS);
        let high_score = Self::keyword_score(&combined, HIGH_KEYWORDS);
        let medium_score = Self::keyword_score(&combined, MEDIUM_KEYWORDS);
        let low_score = Self::keyword_score(&combined, LOW_KEYWORDS);
        let trivial_score = Self::keyword_score(&combined, TRIVIAL_KEYWORDS);

        // Workspace signal adjustments
        let signal_boost = Self::workspace_signal_boost(workspace_signals);

        // File count heuristic: multiple file paths mentioned -> bump complexity
        let file_path_count = Self::count_file_paths(&combined);

        // Compute effective scores with adjustments
        let effective_critical = critical_score + signal_boost.saturating_sub(2);
        let effective_high = high_score + if file_path_count > 3 { 2 } else { 0 };
        let effective_medium = medium_score + if file_path_count > 1 { 1 } else { 0 };

        // Word count heuristic: longer requests tend to be more complex
        let word_count = combined.split_whitespace().count();
        let length_boost = match word_count {
            0..=10 => 0_u32,
            11..=30 => 1,
            31..=80 => 2,
            _ => 3,
        };

        // Select tier: highest effective score wins, biased toward higher complexity
        if effective_critical > 0 || (high_score >= 2 && length_boost >= 3) {
            TaskComplexity::Critical
        } else if effective_high > 0 || (medium_score >= 2 && file_path_count > 2) {
            TaskComplexity::High
        } else if effective_medium > 0 || (low_score > 0 && length_boost >= 2) {
            TaskComplexity::Medium
        } else if low_score > 0 {
            TaskComplexity::Low
        } else if trivial_score > 0 {
            TaskComplexity::Trivial
        } else {
            // Default: medium for unrecognized requests (assume they need some work)
            TaskComplexity::Medium
        }
    }

    /// Count how many keywords from the given list appear in the text.
    fn keyword_score(text: &str, keywords: &[&str]) -> u32 {
        keywords
            .iter()
            .filter(|kw| text.contains(**kw))
            .count() as u32
    }

    /// Analyze workspace signals for complexity indicators.
    ///
    /// Signals like "error", "failing", "multiple packages" boost complexity.
    fn workspace_signal_boost(signals: &[String]) -> u32 {
        let mut boost = 0u32;
        for signal in signals {
            let s = signal.to_lowercase();
            if s.contains("error") || s.contains("failing") || s.contains("broken") {
                boost += 1;
            }
            if s.contains("monorepo") || s.contains("workspace") || s.contains("multi-package") {
                boost += 1;
            }
            if s.contains("large") || s.contains("legacy") || s.contains("complex") {
                boost += 1;
            }
        }
        boost
    }

    /// Count likely file paths in the text (heuristic: contains `/` or `\` with an extension).
    fn count_file_paths(text: &str) -> u32 {
        text.split_whitespace()
            .filter(|word| {
                (word.contains('/') || word.contains('\\'))
                    && word.contains('.')
                    && word.len() > 3
            })
            .count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivial_explain() {
        let result = TaskClassifier::classify("explain what a mutex is", &[]);
        assert_eq!(result, TaskComplexity::Trivial);
    }

    #[test]
    fn low_rename() {
        let result = TaskClassifier::classify("rename the variable foo to bar", &[]);
        assert_eq!(result, TaskComplexity::Low);
    }

    #[test]
    fn medium_implement() {
        let result = TaskClassifier::classify("implement a new endpoint for users", &[]);
        assert_eq!(result, TaskComplexity::Medium);
    }

    #[test]
    fn high_refactor() {
        let result = TaskClassifier::classify("refactor the authentication module", &[]);
        assert_eq!(result, TaskComplexity::High);
    }

    #[test]
    fn critical_overhaul() {
        let result = TaskClassifier::classify("overhaul the entire codebase architecture", &[]);
        assert_eq!(result, TaskComplexity::Critical);
    }

    #[test]
    fn workspace_signals_boost() {
        let result = TaskClassifier::classify(
            "fix the tests",
            &[
                "error in build".to_string(),
                "monorepo with 12 packages".to_string(),
                "legacy codebase".to_string(),
            ],
        );
        // Signals should push this above medium
        assert!(result >= TaskComplexity::High);
    }

    #[test]
    fn unknown_defaults_to_medium() {
        let result = TaskClassifier::classify("do the thing", &[]);
        assert_eq!(result, TaskComplexity::Medium);
    }
}
