use oco_shared_types::{TaskCategory, TaskComplexity};

/// Deterministic task classifier using keyword heuristics.
///
/// Analyzes the user request and workspace signals to estimate
/// task complexity without any LLM call.
pub struct TaskClassifier;

/// Keywords that signal trivial tasks (direct lookup / single fact).
const TRIVIAL_KEYWORDS: &[&str] = &[
    "what is", "explain", "describe", "define", "show me", "list", "print", "display", "version",
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
        keywords.iter().filter(|kw| text.contains(**kw)).count() as u32
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

    /// Classify the category (domain/type) of a task using keyword heuristics.
    ///
    /// Categories are checked in priority order: more specific categories
    /// (Security, Frontend) are checked before broader ones (NewFeature, General).
    pub fn classify_category(request: &str) -> TaskCategory {
        let lower = request.to_lowercase();

        // Priority-ordered keyword lists. Earlier matches win.
        const SECURITY_KEYWORDS: &[&str] = &[
            "security",
            "vulnerab",
            "auth",
            "permission",
            "xss",
            "injection",
            "csrf",
            "encrypt",
            "credential",
        ];
        const FRONTEND_KEYWORDS: &[&str] = &[
            "ui",
            "ux",
            "component",
            "css",
            "style",
            "layout",
            "responsive",
            "design",
            "frontend",
            "button",
            "modal",
            "form",
            "page",
            "dashboard",
            "tailwind",
            "react",
            "svelte",
            "vue",
        ];
        const BUG_KEYWORDS: &[&str] = &[
            "bug",
            "fix",
            "crash",
            "error",
            "broken",
            "issue",
            "debug",
            "regression",
        ];
        const REFACTOR_KEYWORDS: &[&str] = &[
            "refactor",
            "rename",
            "restructure",
            "extract",
            "cleanup",
            "simplify",
            "deduplicate",
        ];
        const TESTING_KEYWORDS: &[&str] = &[
            "test", "coverage", "spec", "assert", "mock", "fixture", "tdd",
        ];
        const REVIEW_KEYWORDS: &[&str] = &["review", "audit", "inspect", "check"];
        const DEVOPS_KEYWORDS: &[&str] = &[
            "deploy",
            "ci",
            "cd",
            "docker",
            "kubernetes",
            "pipeline",
            "infra",
            "terraform",
        ];
        const EXPLANATION_KEYWORDS: &[&str] = &[
            "explain",
            "what",
            "how",
            "why",
            "understand",
            "document",
        ];
        const NEW_FEATURE_KEYWORDS: &[&str] = &[
            "add",
            "create",
            "implement",
            "build",
            "new feature",
            "scaffold",
        ];

        // Score each category; highest score wins. Ties broken by priority order.
        let scores: Vec<(TaskCategory, u32)> = vec![
            (TaskCategory::Security, Self::keyword_score(&lower, SECURITY_KEYWORDS)),
            (TaskCategory::Frontend, Self::keyword_score(&lower, FRONTEND_KEYWORDS)),
            (TaskCategory::Bug, Self::keyword_score(&lower, BUG_KEYWORDS)),
            (TaskCategory::Refactor, Self::keyword_score(&lower, REFACTOR_KEYWORDS)),
            (TaskCategory::Testing, Self::keyword_score(&lower, TESTING_KEYWORDS)),
            (TaskCategory::Review, Self::keyword_score(&lower, REVIEW_KEYWORDS)),
            (TaskCategory::DevOps, Self::keyword_score(&lower, DEVOPS_KEYWORDS)),
            (TaskCategory::Explanation, Self::keyword_score(&lower, EXPLANATION_KEYWORDS)),
            (TaskCategory::NewFeature, Self::keyword_score(&lower, NEW_FEATURE_KEYWORDS)),
        ];

        // Return the category with the highest score.
        // On tie, the category earlier in the list wins (higher priority).
        scores
            .into_iter()
            .enumerate()
            .filter(|(_, (_, score))| *score > 0)
            .max_by(|(idx_a, (_, score_a)), (idx_b, (_, score_b))| {
                score_a.cmp(score_b).then(idx_b.cmp(idx_a))
            })
            .map(|(_, (cat, _))| cat)
            .unwrap_or(TaskCategory::General)
    }

    /// Count likely file paths in the text (heuristic: contains `/` or `\` with an extension).
    fn count_file_paths(text: &str) -> u32 {
        text.split_whitespace()
            .filter(|word| {
                (word.contains('/') || word.contains('\\')) && word.contains('.') && word.len() > 3
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

    // --- TaskCategory classification tests ---

    #[test]
    fn category_bug() {
        assert_eq!(
            TaskClassifier::classify_category("fix the login bug"),
            TaskCategory::Bug
        );
    }

    #[test]
    fn category_refactor() {
        assert_eq!(
            TaskClassifier::classify_category("refactor the database layer"),
            TaskCategory::Refactor
        );
    }

    #[test]
    fn category_frontend_dashboard() {
        let cat = TaskClassifier::classify_category("add a new dashboard page");
        assert!(
            cat == TaskCategory::Frontend || cat == TaskCategory::NewFeature,
            "expected Frontend or NewFeature, got {:?}",
            cat
        );
    }

    #[test]
    fn category_security_overrides_review() {
        // "review" + "vulnerabilities" + "authentication" → Security wins
        assert_eq!(
            TaskClassifier::classify_category("review the authentication code for vulnerabilities"),
            TaskCategory::Security
        );
    }

    #[test]
    fn category_explanation() {
        assert_eq!(
            TaskClassifier::classify_category("explain how the context engine works"),
            TaskCategory::Explanation
        );
    }

    #[test]
    fn category_testing() {
        assert_eq!(
            TaskClassifier::classify_category("create unit tests for the parser"),
            TaskCategory::Testing
        );
    }

    #[test]
    fn category_devops() {
        assert_eq!(
            TaskClassifier::classify_category("deploy to production"),
            TaskCategory::DevOps
        );
    }

    #[test]
    fn category_general_fallback() {
        assert_eq!(
            TaskClassifier::classify_category("do something"),
            TaskCategory::General
        );
    }
}
