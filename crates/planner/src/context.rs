//! Planning context — everything the planner needs to generate a plan.

use oco_shared_types::{Budget, RegistrySummary, RepoProfile, TaskCategory, TaskComplexity};
use serde::{Deserialize, Serialize};

/// Context provided to the planner for plan generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningContext {
    /// Repo profile: language, stack, risk level, test framework, etc.
    pub repo_profile: RepoProfile,
    /// Summary of available capabilities (tools, MCP, agents, skills, LLMs).
    pub capabilities: RegistrySummary,
    /// Classified task complexity.
    pub task_complexity: TaskComplexity,
    /// Classified task category.
    pub task_category: TaskCategory,
    /// Current working memory (prior knowledge, findings, hypotheses).
    pub working_memory_summary: String,
    /// Remaining budget for the session.
    pub budget: Budget,
}

impl PlanningContext {
    /// Maximum tokens the planner can spend on a SINGLE planning LLM call.
    /// Capped at 5% of total budget per call. With max 3 replans, cumulative
    /// cap is ~20% (plan + 3 replans × 5% = 20%). See fix for GPT-5.4 review #10.
    pub fn planning_token_budget(&self) -> u32 {
        ((self.budget.max_total_tokens / 20).max(500)) as u32
    }

    /// Whether the task needs a multi-step plan (vs direct execution).
    pub fn needs_planning(&self) -> bool {
        matches!(
            self.task_complexity,
            TaskComplexity::Medium | TaskComplexity::High | TaskComplexity::Critical
        )
    }

    /// Whether the task is complex enough to warrant a team.
    pub fn needs_team(&self) -> bool {
        matches!(
            self.task_complexity,
            TaskComplexity::High | TaskComplexity::Critical
        )
    }

    /// Build a minimal context for testing.
    pub fn minimal(complexity: TaskComplexity, category: TaskCategory) -> Self {
        Self {
            repo_profile: RepoProfile::default(),
            capabilities: RegistrySummary {
                tools: Vec::new(),
                mcp: Vec::new(),
                agents: Vec::new(),
                skills: Vec::new(),
                llms: Vec::new(),
            },
            task_complexity: complexity,
            task_category: category,
            working_memory_summary: String::new(),
            budget: Budget::for_complexity(complexity),
        }
    }
}
