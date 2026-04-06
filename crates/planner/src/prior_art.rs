//! Prior Art analysis — avoid reinventing the wheel.
//!
//! Before generating a plan for Medium+ tasks, check whether existing
//! open-source solutions or research papers could accelerate the work.
//! The output feeds into the planner prompt as a recommendation to add
//! research steps before implementation.
//!
//! Deterministic — no LLM call. Uses category, complexity, keywords,
//! and repo profile heuristics (same pattern as `risk_analysis.rs`).

use oco_shared_types::{TaskCategory, TaskComplexity};
use serde::{Deserialize, Serialize};

use crate::context::PlanningContext;

/// Recommendation on whether to search for prior art before implementing.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriorArtRecommendation {
    /// Whether to search for existing open-source projects/libraries.
    pub should_search_oss: bool,
    /// Whether to search for recent research papers.
    pub should_search_papers: bool,
    /// Search hints for open-source discovery (e.g., "JWT library rust").
    pub oss_search_hints: Vec<String>,
    /// Search hints for academic/research papers (e.g., "token-based authentication").
    pub paper_search_hints: Vec<String>,
    /// Package registries to check, derived from repo stack.
    pub registries: Vec<String>,
    /// Human-readable rationale for the recommendation.
    pub rationale: String,
}

impl PriorArtRecommendation {
    /// Whether any research is recommended.
    pub fn any(&self) -> bool {
        self.should_search_oss || self.should_search_papers
    }
}

/// Analyze whether a task would benefit from prior art research.
/// Deterministic — no LLM call.
pub fn analyze_prior_art(request: &str, context: &PlanningContext) -> PriorArtRecommendation {
    let mut rec = PriorArtRecommendation::default();

    // Skip for trivial/low complexity — not worth the research overhead
    if matches!(
        context.task_complexity,
        TaskComplexity::Trivial | TaskComplexity::Low
    ) {
        rec.rationale = "Task is low complexity — research overhead not justified.".into();
        return rec;
    }

    // Skip for categories that don't benefit from prior art
    if matches!(
        context.task_category,
        TaskCategory::Bug
            | TaskCategory::Explanation
            | TaskCategory::Review
            | TaskCategory::Testing
    ) {
        rec.rationale =
            "Task category (bug/explanation/review/testing) rarely benefits from prior art search."
                .into();
        return rec;
    }

    let request_lower = request.to_lowercase();
    let tokens: Vec<&str> = request_lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect();
    let has_token = |keywords: &[&str]| tokens.iter().any(|t| keywords.contains(t));

    // --- Category-specific triggers ---
    match context.task_category {
        TaskCategory::NewFeature => {
            rec.should_search_oss = true;
            rec.rationale =
                "New feature — check if existing libraries/tools already solve this.".into();
        }
        TaskCategory::Security => {
            rec.should_search_oss = true;
            rec.should_search_papers = true;
            rec.rationale =
                "Security work — check established tools and recent vulnerability research.".into();
        }
        TaskCategory::DevOps => {
            rec.should_search_oss = true;
            rec.rationale =
                "DevOps task — check existing infrastructure tools and patterns.".into();
        }
        TaskCategory::Refactor => {
            // Refactors only warrant OSS search if they introduce new patterns
            if has_token(&["pattern", "architecture", "framework", "replace"]) {
                rec.should_search_oss = true;
                rec.rationale =
                    "Refactor introduces new patterns — check existing solutions.".into();
            }
        }
        TaskCategory::Frontend => {
            rec.should_search_oss = true;
            rec.rationale =
                "Frontend work — check existing UI libraries and component systems.".into();
        }
        _ => {}
    }

    // --- Keyword-based paper triggers ---
    if has_token(&[
        "algorithm",
        "optimize",
        "optimization",
        "compress",
        "compression",
        "encrypt",
        "encryption",
        "hash",
        "hashing",
        "ml",
        "machine",
        "learning",
        "ai",
        "neural",
        "embedding",
        "vector",
        "similarity",
        "ranking",
        "rerank",
        "tokenize",
        "tokenizer",
        "parse",
        "parser",
        "protocol",
    ]) {
        rec.should_search_papers = true;
        if rec.rationale.is_empty() {
            rec.rationale =
                "Task involves algorithmic/research-heavy work — check recent papers.".into();
        }
    }

    // --- Keyword-based OSS triggers ---
    if has_token(&[
        "library",
        "framework",
        "sdk",
        "client",
        "driver",
        "plugin",
        "integration",
        "api",
        "connector",
        "adapter",
        "wrapper",
    ]) {
        rec.should_search_oss = true;
        if rec.rationale.is_empty() {
            rec.rationale =
                "Task likely has existing library solutions — check package registries.".into();
        }
    }

    // --- Build search hints from request tokens ---
    if rec.should_search_oss {
        let stack = &context.repo_profile.stack;
        // Extract meaningful tokens (skip stop words) for search hints
        let stop_words = [
            "the",
            "a",
            "an",
            "to",
            "for",
            "in",
            "on",
            "with",
            "and",
            "or",
            "add",
            "create",
            "implement",
            "build",
            "make",
            "new",
            "use",
            "using",
            "support",
            "that",
            "this",
            "it",
            "is",
            "be",
        ];
        let meaningful: Vec<&str> = tokens
            .iter()
            .filter(|t| t.len() > 2 && !stop_words.contains(t))
            .copied()
            .take(5)
            .collect();
        if !meaningful.is_empty() {
            let hint = if stack.is_empty() || stack == "unknown" {
                meaningful.join(" ")
            } else {
                format!("{} {}", meaningful.join(" "), stack)
            };
            rec.oss_search_hints.push(hint);
        }
    }

    if rec.should_search_papers {
        // For papers, use a more academic phrasing
        let stop_words = [
            "the",
            "a",
            "an",
            "to",
            "for",
            "in",
            "on",
            "with",
            "and",
            "or",
            "add",
            "create",
            "implement",
            "build",
            "make",
            "new",
            "use",
            "using",
            "support",
            "that",
            "this",
            "it",
            "is",
            "be",
        ];
        let meaningful: Vec<&str> = tokens
            .iter()
            .filter(|t| t.len() > 2 && !stop_words.contains(t))
            .copied()
            .take(6)
            .collect();
        if !meaningful.is_empty() {
            rec.paper_search_hints.push(meaningful.join(" "));
        }
    }

    // --- Detect registries from repo stack ---
    if rec.should_search_oss {
        let stack = &context.repo_profile.stack;
        match stack.as_str() {
            "rust" => rec.registries.push("crates.io".into()),
            "node" => rec.registries.push("npm".into()),
            "python" => rec.registries.push("pypi".into()),
            "go" => rec.registries.push("pkg.go.dev".into()),
            "mixed" => {
                // For mixed repos, include all major registries
                rec.registries.push("crates.io".into());
                rec.registries.push("npm".into());
                rec.registries.push("pypi".into());
            }
            _ => {} // unknown stack — no specific registry
        }
    }

    rec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_feature_medium_triggers_oss_search() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        let rec = analyze_prior_art("add JWT authentication", &ctx);

        assert!(rec.should_search_oss);
        assert!(rec.any());
        assert!(rec.rationale.contains("existing"));
    }

    #[test]
    fn bug_skips_research() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Bug);
        let rec = analyze_prior_art("fix null pointer in auth handler", &ctx);

        assert!(!rec.should_search_oss);
        assert!(!rec.should_search_papers);
        assert!(!rec.any());
    }

    #[test]
    fn trivial_always_skips() {
        let ctx = PlanningContext::minimal(TaskComplexity::Trivial, TaskCategory::NewFeature);
        let rec = analyze_prior_art("add a big complex feature", &ctx);

        assert!(!rec.should_search_oss);
        assert!(!rec.should_search_papers);
    }

    #[test]
    fn low_complexity_skips() {
        let ctx = PlanningContext::minimal(TaskComplexity::Low, TaskCategory::NewFeature);
        let rec = analyze_prior_art("add logging middleware", &ctx);

        assert!(!rec.should_search_oss);
        assert!(!rec.should_search_papers);
    }

    #[test]
    fn explanation_skips_research() {
        let ctx = PlanningContext::minimal(TaskComplexity::High, TaskCategory::Explanation);
        let rec = analyze_prior_art("explain the authentication flow", &ctx);

        assert!(!rec.any());
    }

    #[test]
    fn review_skips_research() {
        let ctx = PlanningContext::minimal(TaskComplexity::High, TaskCategory::Review);
        let rec = analyze_prior_art("review the database module", &ctx);

        assert!(!rec.any());
    }

    #[test]
    fn testing_skips_research() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Testing);
        let rec = analyze_prior_art("add tests for user service", &ctx);

        assert!(!rec.any());
    }

    #[test]
    fn algorithm_keyword_triggers_papers() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        let rec = analyze_prior_art("implement a better compression algorithm", &ctx);

        assert!(rec.should_search_papers);
        assert!(rec.should_search_oss);
        assert!(!rec.paper_search_hints.is_empty());
    }

    #[test]
    fn encryption_keyword_triggers_papers() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::General);
        let rec = analyze_prior_art("add encryption for user data", &ctx);

        assert!(rec.should_search_papers);
    }

    #[test]
    fn library_keyword_triggers_oss() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::General);
        let rec = analyze_prior_art("integrate a PDF library", &ctx);

        assert!(rec.should_search_oss);
    }

    #[test]
    fn sdk_keyword_triggers_oss() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::General);
        let rec = analyze_prior_art("build an sdk client for the payment API", &ctx);

        assert!(rec.should_search_oss);
    }

    #[test]
    fn security_triggers_both() {
        let ctx = PlanningContext::minimal(TaskComplexity::High, TaskCategory::Security);
        let rec = analyze_prior_art("harden the authentication system", &ctx);

        assert!(rec.should_search_oss);
        assert!(rec.should_search_papers);
    }

    #[test]
    fn devops_triggers_oss() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::DevOps);
        let rec = analyze_prior_art("set up CI/CD pipeline", &ctx);

        assert!(rec.should_search_oss);
        assert!(!rec.should_search_papers);
    }

    #[test]
    fn frontend_triggers_oss() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Frontend);
        let rec = analyze_prior_art("build a dashboard UI", &ctx);

        assert!(rec.should_search_oss);
    }

    #[test]
    fn rust_stack_detects_crates_io() {
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.repo_profile.stack = "rust".into();
        let rec = analyze_prior_art("add HTTP client", &ctx);

        assert!(rec.registries.contains(&"crates.io".to_string()));
    }

    #[test]
    fn node_stack_detects_npm() {
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.repo_profile.stack = "node".into();
        let rec = analyze_prior_art("add WebSocket support", &ctx);

        assert!(rec.registries.contains(&"npm".to_string()));
    }

    #[test]
    fn python_stack_detects_pypi() {
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.repo_profile.stack = "python".into();
        let rec = analyze_prior_art("add data validation", &ctx);

        assert!(rec.registries.contains(&"pypi".to_string()));
    }

    #[test]
    fn go_stack_detects_pkg_go_dev() {
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.repo_profile.stack = "go".into();
        let rec = analyze_prior_art("add gRPC service", &ctx);

        assert!(rec.registries.contains(&"pkg.go.dev".to_string()));
    }

    #[test]
    fn mixed_stack_includes_multiple_registries() {
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.repo_profile.stack = "mixed".into();
        let rec = analyze_prior_art("add monitoring", &ctx);

        assert!(rec.registries.len() >= 2);
    }

    #[test]
    fn unknown_stack_has_no_registries() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        // default stack is empty/unknown
        let rec = analyze_prior_art("add feature", &ctx);

        assert!(rec.registries.is_empty());
    }

    #[test]
    fn oss_hints_include_stack() {
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.repo_profile.stack = "rust".into();
        let rec = analyze_prior_art("add JWT authentication", &ctx);

        assert!(!rec.oss_search_hints.is_empty());
        assert!(rec.oss_search_hints[0].contains("rust"));
    }

    #[test]
    fn paper_hints_generated_for_research_tasks() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        let rec = analyze_prior_art("implement vector similarity search algorithm", &ctx);

        assert!(rec.should_search_papers);
        assert!(!rec.paper_search_hints.is_empty());
        assert!(rec.paper_search_hints[0].contains("vector"));
    }

    #[test]
    fn refactor_without_pattern_keyword_skips_oss() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Refactor);
        let rec = analyze_prior_art("rename variables in auth module", &ctx);

        assert!(!rec.should_search_oss);
    }

    #[test]
    fn refactor_with_pattern_keyword_triggers_oss() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Refactor);
        let rec = analyze_prior_art("replace auth with a new framework", &ctx);

        assert!(rec.should_search_oss);
    }

    #[test]
    fn ml_keyword_triggers_papers() {
        let ctx = PlanningContext::minimal(TaskComplexity::High, TaskCategory::NewFeature);
        let rec = analyze_prior_art("add ml-based code classification", &ctx);

        assert!(rec.should_search_papers);
    }

    #[test]
    fn any_returns_false_when_no_research() {
        let ctx = PlanningContext::minimal(TaskComplexity::Trivial, TaskCategory::Bug);
        let rec = analyze_prior_art("fix typo", &ctx);

        assert!(!rec.any());
    }
}
