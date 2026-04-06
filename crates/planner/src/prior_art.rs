//! Prior Art analysis — avoid reinventing the wheel.
//!
//! Before generating a plan for Medium+ tasks, check whether existing
//! open-source solutions or research papers could accelerate the work.
//! The output feeds into the planner prompt as a recommendation to add
//! research steps before implementation.
//!
//! Deterministic — no LLM call. Uses category, complexity, keywords,
//! repo profile, and capability registry heuristics (same pattern as
//! `risk_analysis.rs`).
//!
//! **Key design decisions (addressing audit findings):**
//! - **Capability gating**: research is only recommended if the registry
//!   actually has `web_search` or `search` capability.
//! - **Additive scoring**: Bug/Testing categories don't hard-skip — they
//!   use keyword scoring so "fix protocol parser bug" can still trigger research.
//! - **Acronym safelist**: short tokens like "ui", "ci", "ml" are preserved
//!   in search hints instead of being dropped by a `len() > 2` filter.
//! - **Step budget awareness**: Medium tasks get "1 research step", High/Critical
//!   get "1-2 research steps" — avoids conflicting with the planner's step limits.

use oco_shared_types::{TaskCategory, TaskComplexity};
use serde::{Deserialize, Serialize};

use crate::context::PlanningContext;

/// Short technical acronyms that should be preserved in search hints
/// despite being ≤2 characters.
const ACRONYM_SAFELIST: &[&str] = &[
    "ui", "ci", "cd", "ml", "ai", "db", "jwt", "sso", "api", "sdk", "cli", "sql", "css", "ux",
    "io", "rx", "ws", "fs", "os", "ip", "vm",
];

/// Stop words filtered out of search hints.
const STOP_WORDS: &[&str] = &[
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
    "fix",
    "update",
    "change",
    "set",
    "get",
    "from",
    "into",
    "of",
];

/// Advice on whether to include prior-art research steps in the plan.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PriorArtAdvice {
    /// Whether the task would benefit from research before implementation.
    pub needs_research: bool,
    /// Search hints for discovery (e.g., "JWT authentication rust").
    pub search_hints: Vec<String>,
    /// Ready-to-inject prompt section ("" if no research needed).
    /// Contains the full "## Prior Art Research" block for the planner prompt.
    pub prompt_section: String,
    /// Whether the capability registry actually has search/web capability.
    /// If false, research is never recommended regardless of heuristics.
    pub research_capable: bool,
}

/// Analyze whether a task would benefit from prior art research.
/// Deterministic — no LLM call.
pub fn analyze_prior_art(request: &str, context: &PlanningContext) -> PriorArtAdvice {
    let mut advice = PriorArtAdvice::default();

    // --- Capability gating (P0) ---
    // Only gate on tools/MCP with exact capability match — agents/skills
    // named "researcher" don't count as executable search capability.
    let search_cap = context
        .capabilities
        .find_tool_capability(&["web_search", "search"]);
    advice.research_capable = search_cap.is_some();

    let search_cap = match search_cap {
        Some(cap) => cap,
        None => return advice,
    };

    // --- Complexity gate ---
    // Trivial/Low tasks: research overhead not justified.
    if matches!(
        context.task_complexity,
        TaskComplexity::Trivial | TaskComplexity::Low
    ) {
        return advice;
    }

    let request_lower = request.to_lowercase();
    let tokens: Vec<&str> = request_lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| !t.is_empty())
        .collect();
    let has_token = |keywords: &[&str]| tokens.iter().any(|t| keywords.contains(t));

    // --- Category-specific scoring ---
    let mut score: i32 = 0;
    let mut rationale = String::new();
    let mut should_search_papers = false;
    let mut registries: Vec<String> = Vec::new();

    match context.task_category {
        // Hard skip: these categories never need external research.
        TaskCategory::Explanation | TaskCategory::Review => {
            return advice;
        }

        // Additive scoring: Bug/Testing only trigger research on specific keywords.
        TaskCategory::Bug => {
            if has_token(&[
                "algorithm",
                "protocol",
                "parser",
                "codec",
                "encoding",
                "format",
            ]) {
                score += 2;
                rationale =
                    "Bug involves algorithmic/protocol work — check known solutions.".into();
            }
            if has_token(&["regression", "performance", "benchmark"]) {
                score += 1;
            }
        }
        TaskCategory::Testing => {
            if has_token(&[
                "fuzz",
                "fuzzing",
                "property",
                "mutation",
                "coverage",
                "framework",
            ]) {
                score += 2;
                rationale =
                    "Testing task involves advanced techniques — check existing frameworks.".into();
                should_search_papers = has_token(&["fuzz", "fuzzing", "property", "mutation"]);
            }
        }

        // These categories default to research.
        TaskCategory::NewFeature => {
            score += 2;
            rationale =
                "New feature — check if existing libraries/tools already solve this.".into();
        }
        TaskCategory::Security => {
            score += 2;
            should_search_papers = true;
            rationale =
                "Security work — check established tools and recent vulnerability research.".into();
        }
        TaskCategory::DevOps => {
            score += 2;
            rationale = "DevOps task — check existing infrastructure tools and patterns.".into();
        }
        TaskCategory::Frontend => {
            score += 2;
            rationale = "Frontend work — check existing UI libraries and component systems.".into();
        }
        TaskCategory::Refactor => {
            if has_token(&["pattern", "architecture", "framework", "replace"]) {
                score += 2;
                rationale = "Refactor introduces new patterns — check existing solutions.".into();
            }
        }
        TaskCategory::General => {
            // General tasks: keyword-driven only.
        }
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
        should_search_papers = true;
        if score == 0 {
            score += 1;
        }
        if rationale.is_empty() {
            rationale =
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
        if score == 0 {
            score += 1;
        }
        if rationale.is_empty() {
            rationale =
                "Task likely has existing library solutions — check package registries.".into();
        }
    }

    // --- Decision: does this task need research? ---
    if score <= 0 {
        return advice;
    }

    advice.needs_research = true;

    // --- Build search hints (with acronym safelist) ---
    let meaningful: Vec<&str> = tokens
        .iter()
        .filter(|t| !STOP_WORDS.contains(t) && (t.len() > 2 || ACRONYM_SAFELIST.contains(t)))
        .copied()
        .take(5)
        .collect();

    let stack = &context.repo_profile.stack;
    if !meaningful.is_empty() {
        let hint = if stack.is_empty() || stack == "unknown" {
            meaningful.join(" ")
        } else {
            format!("{} {}", meaningful.join(" "), stack)
        };
        advice.search_hints.push(hint);
    }

    if should_search_papers {
        let paper_tokens: Vec<&str> = tokens
            .iter()
            .filter(|t| !STOP_WORDS.contains(t) && (t.len() > 2 || ACRONYM_SAFELIST.contains(t)))
            .copied()
            .take(6)
            .collect();
        if !paper_tokens.is_empty() {
            advice.search_hints.push(paper_tokens.join(" "));
        }
    }

    // --- Empty hints guard ---
    // If search_hints is still empty after filtering, fall back to first meaningful tokens.
    if advice.search_hints.is_empty() {
        let fallback: Vec<&str> = tokens
            .iter()
            .filter(|t| t.len() > 3 && !STOP_WORDS.contains(t))
            .copied()
            .take(3)
            .collect();
        if !fallback.is_empty() {
            advice.search_hints.push(fallback.join(" "));
        }
    }

    // If still empty, don't generate a prompt section.
    if advice.search_hints.is_empty() {
        advice.needs_research = false;
        return advice;
    }

    // --- Detect registries from repo stack ---
    match stack.as_str() {
        "rust" => registries.push("crates.io".into()),
        "node" => registries.push("npm".into()),
        "python" => registries.push("pypi".into()),
        "go" => registries.push("pkg.go.dev".into()),
        "mixed" => {
            registries.push("crates.io".into());
            registries.push("npm".into());
            registries.push("pypi".into());
        }
        _ => {}
    }

    // --- Build prompt_section (step budget aware) ---
    // Use the actual detected capability name, not a hardcoded string.
    let hints_str = advice.search_hints.join("; ");
    let registry_line = if registries.is_empty() {
        String::new()
    } else {
        format!(" Check registries: {}.", registries.join(", "))
    };

    let section = match context.task_complexity {
        // Medium (2-5 total steps): single combined research step to stay within budget.
        TaskComplexity::Medium => {
            let paper_note = if should_search_papers {
                " Also check recent research papers."
            } else {
                ""
            };
            format!(
                "\n\n## Prior Art Research\n\n\
                 {rationale}\n\n\
                 Include 1 research step before implementation.\n\
                 - Add a 'research' step (subagent, read_only, {search_cap}) that searches for existing solutions and summarizes findings.\
                 {paper_note} Search hints: {hints_str}.{registry_line}\n\
                 - Implementation steps must depend on 'research'.\n"
            )
        }
        // High/Critical (3-12 total steps): multi-step research allowed.
        _ => {
            let mut s = format!(
                "\n\n## Prior Art Research\n\n\
                 {rationale}\n\n\
                 Include 1-2 research steps before implementation.\n\
                 - Add a 'research-oss' step (subagent, read_only, {search_cap}) to find existing solutions. Search hints: {hints_str}.{registry_line}\n"
            );
            if should_search_papers {
                s.push_str(&format!(
                    "- Add a 'research-papers' step (subagent, read_only, {search_cap}) to find recent research papers.\n"
                ));
            }
            s.push_str(
                "- Add a 'synthesize-research' step that depends on the research steps, summarizing findings and recommending whether to use an existing solution or build from scratch.\n\
                 - Implementation steps must depend on 'synthesize-research'.\n"
            );
            s
        }
    };

    advice.prompt_section = section;

    advice
}

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::SummaryItem;

    /// Helper: create a PlanningContext with search capability enabled.
    fn ctx_with_search(complexity: TaskComplexity, category: TaskCategory) -> PlanningContext {
        let mut ctx = PlanningContext::minimal(complexity, category);
        ctx.capabilities.tools.push(SummaryItem {
            id: "web_search".into(),
            name: "Web Search".into(),
            capabilities: vec!["web_search".into()],
        });
        ctx
    }

    // --- Capability gating ---

    #[test]
    fn no_search_capability_skips_research() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add JWT authentication", &ctx);

        assert!(!advice.needs_research);
        assert!(!advice.research_capable);
        assert!(advice.prompt_section.is_empty());
    }

    #[test]
    fn with_search_capability_enables_research() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add JWT authentication", &ctx);

        assert!(advice.research_capable);
        assert!(advice.needs_research);
    }

    // --- Category: Explanation / Review always skip ---

    #[test]
    fn explanation_always_skips_research() {
        let ctx = ctx_with_search(TaskComplexity::High, TaskCategory::Explanation);
        let advice = analyze_prior_art(
            "explain the authentication flow with algorithm details",
            &ctx,
        );

        assert!(!advice.needs_research);
    }

    #[test]
    fn review_always_skips_research() {
        let ctx = ctx_with_search(TaskComplexity::High, TaskCategory::Review);
        let advice = analyze_prior_art("review the database module", &ctx);

        assert!(!advice.needs_research);
    }

    // --- Category: Bug — additive scoring ---

    #[test]
    fn bug_without_keywords_skips_research() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::Bug);
        let advice = analyze_prior_art("fix null pointer in user handler", &ctx);

        assert!(!advice.needs_research);
    }

    #[test]
    fn bug_with_algorithm_keyword_triggers_research() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::Bug);
        let advice = analyze_prior_art("fix protocol parser bug", &ctx);

        assert!(advice.needs_research);
        assert!(!advice.prompt_section.is_empty());
    }

    // --- Category: Testing — additive scoring ---

    #[test]
    fn testing_without_keywords_skips_research() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::Testing);
        let advice = analyze_prior_art("add unit test for login", &ctx);

        assert!(!advice.needs_research);
    }

    #[test]
    fn testing_with_fuzz_keyword_triggers_research() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::Testing);
        let advice = analyze_prior_art("add fuzz tests for parser", &ctx);

        assert!(advice.needs_research);
        assert!(!advice.prompt_section.is_empty());
    }

    // --- Complexity gates ---

    #[test]
    fn trivial_always_skips() {
        let ctx = ctx_with_search(TaskComplexity::Trivial, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add a big complex feature", &ctx);

        assert!(!advice.needs_research);
    }

    #[test]
    fn low_complexity_skips() {
        let ctx = ctx_with_search(TaskComplexity::Low, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add logging middleware", &ctx);

        assert!(!advice.needs_research);
    }

    // --- NewFeature / keyword triggers ---

    #[test]
    fn new_feature_with_oss_keyword() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        let advice = analyze_prior_art("integrate Redis caching", &ctx);

        assert!(advice.needs_research);
        assert!(!advice.search_hints.is_empty());
    }

    #[test]
    fn algorithm_keyword_triggers_papers_medium() {
        // Medium: single combined step mentions papers
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        let advice = analyze_prior_art("implement a better compression algorithm", &ctx);

        assert!(advice.needs_research);
        assert!(advice.prompt_section.contains("research papers"));
    }

    #[test]
    fn algorithm_keyword_triggers_papers_high() {
        // High: multi-step pattern with separate research-papers step
        let ctx = ctx_with_search(TaskComplexity::High, TaskCategory::NewFeature);
        let advice = analyze_prior_art("implement a better compression algorithm", &ctx);

        assert!(advice.needs_research);
        assert!(advice.prompt_section.contains("research-papers"));
        assert!(advice.prompt_section.contains("research-oss"));
    }

    #[test]
    fn security_triggers_both() {
        let ctx = ctx_with_search(TaskComplexity::High, TaskCategory::Security);
        let advice = analyze_prior_art("harden the authentication system", &ctx);

        assert!(advice.needs_research);
        assert!(advice.prompt_section.contains("research-oss"));
        assert!(advice.prompt_section.contains("research-papers"));
    }

    // --- Acronym safelist ---

    #[test]
    fn acronyms_preserved_in_hints() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add JWT SSO for CI", &ctx);

        assert!(advice.needs_research);
        let all_hints = advice.search_hints.join(" ");
        assert!(
            all_hints.contains("jwt"),
            "hints should contain 'jwt': {all_hints}"
        );
        assert!(
            all_hints.contains("sso"),
            "hints should contain 'sso': {all_hints}"
        );
        assert!(
            all_hints.contains("ci"),
            "hints should contain 'ci': {all_hints}"
        );
    }

    // --- Empty hints guard ---

    #[test]
    fn search_hints_never_empty_when_research_needed() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add authentication", &ctx);

        if advice.needs_research {
            assert!(!advice.search_hints.is_empty());
        }
    }

    #[test]
    fn empty_hints_fallback_to_request_tokens() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        // Use a request with only short common words that get filtered
        let advice = analyze_prior_art("add very important feature with details", &ctx);

        // Should still produce hints via fallback
        if advice.needs_research {
            assert!(!advice.search_hints.is_empty());
        }
    }

    // --- Step budget in prompt_section ---

    #[test]
    fn medium_prompt_section_mentions_single_step() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add JWT authentication", &ctx);

        assert!(advice.prompt_section.contains("Include 1 research step"));
        assert!(!advice.prompt_section.contains("1-2"));
    }

    #[test]
    fn high_prompt_section_allows_two_steps() {
        let ctx = ctx_with_search(TaskComplexity::High, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add OAuth2 authentication system", &ctx);

        assert!(advice.prompt_section.contains("1-2 research steps"));
    }

    // --- Registries ---

    #[test]
    fn rust_stack_detects_crates_io() {
        let mut ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.repo_profile.stack = "rust".into();
        let advice = analyze_prior_art("add HTTP client", &ctx);

        assert!(advice.prompt_section.contains("crates.io"));
    }

    #[test]
    fn stack_specific_hints_included() {
        let mut ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.repo_profile.stack = "rust".into();
        let advice = analyze_prior_art("add async runtime", &ctx);

        assert!(advice.needs_research);
        let all_hints = advice.search_hints.join(" ");
        assert!(
            all_hints.contains("rust"),
            "hints should include stack: {all_hints}"
        );
    }

    // --- Prompt section structure ---

    #[test]
    fn medium_prompt_section_contains_required_elements() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add JWT authentication", &ctx);

        assert!(advice.prompt_section.contains("## Prior Art Research"));
        // Medium: single 'research' step, no separate research-oss/synthesize
        assert!(advice.prompt_section.contains("'research'"));
        assert!(advice.prompt_section.contains("depend on 'research'"));
    }

    #[test]
    fn high_prompt_section_contains_required_elements() {
        let ctx = ctx_with_search(TaskComplexity::High, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add JWT authentication", &ctx);

        assert!(advice.prompt_section.contains("## Prior Art Research"));
        // High: multi-step pattern
        assert!(advice.prompt_section.contains("research-oss"));
        assert!(advice.prompt_section.contains("synthesize-research"));
    }

    // --- Edge cases ---

    #[test]
    fn devops_triggers_oss() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::DevOps);
        let advice = analyze_prior_art("set up CI/CD pipeline", &ctx);

        assert!(advice.needs_research);
    }

    #[test]
    fn frontend_triggers_oss() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::Frontend);
        let advice = analyze_prior_art("build a dashboard UI", &ctx);

        assert!(advice.needs_research);
    }

    #[test]
    fn refactor_without_pattern_keyword_skips() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::Refactor);
        let advice = analyze_prior_art("rename variables in auth module", &ctx);

        assert!(!advice.needs_research);
    }

    #[test]
    fn refactor_with_pattern_keyword_triggers() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::Refactor);
        let advice = analyze_prior_art("replace auth with a new framework", &ctx);

        assert!(advice.needs_research);
    }

    #[test]
    fn general_with_library_keyword_triggers() {
        let ctx = ctx_with_search(TaskComplexity::Medium, TaskCategory::General);
        let advice = analyze_prior_art("integrate a PDF library", &ctx);

        assert!(advice.needs_research);
    }

    #[test]
    fn ml_keyword_triggers_papers() {
        let ctx = ctx_with_search(TaskComplexity::High, TaskCategory::NewFeature);
        let advice = analyze_prior_art("add ml-based code classification", &ctx);

        assert!(advice.needs_research);
        assert!(advice.prompt_section.contains("research-papers"));
    }

    // --- Capability gating precision ---

    #[test]
    fn agent_with_search_capability_does_not_gate() {
        // An agent named "researcher" with "search" capability should NOT
        // satisfy the gating — only tools/MCP count.
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.capabilities.agents.push(SummaryItem {
            id: "researcher".into(),
            name: "Deep Research Agent".into(),
            capabilities: vec!["search".into()],
        });
        let advice = analyze_prior_art("add JWT authentication", &ctx);

        assert!(!advice.research_capable);
        assert!(!advice.needs_research);
    }

    #[test]
    fn mcp_search_capability_gates_correctly() {
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.capabilities.mcp.push(SummaryItem {
            id: "mcp:perplexity".into(),
            name: "Perplexity".into(),
            capabilities: vec!["search".into()],
        });
        let advice = analyze_prior_art("add JWT authentication", &ctx);

        assert!(advice.research_capable);
        assert!(advice.needs_research);
    }

    #[test]
    fn prompt_uses_detected_capability_not_hardcoded() {
        // When only "search" is available (not "web_search"), the prompt
        // should use "search", not hardcode "web_search".
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.capabilities.mcp.push(SummaryItem {
            id: "mcp:search".into(),
            name: "Search".into(),
            capabilities: vec!["search".into()],
        });
        let advice = analyze_prior_art("add JWT authentication", &ctx);

        assert!(advice.needs_research);
        assert!(
            advice.prompt_section.contains("search"),
            "prompt should reference the detected capability"
        );
        // Should NOT contain "web_search" since only "search" is available
        assert!(
            !advice.prompt_section.contains("web_search"),
            "prompt should not hardcode web_search when only search is available: {}",
            advice.prompt_section
        );
    }
}
