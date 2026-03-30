//! LLM-powered planner — generates structured execution plans via LLM calls.

use async_trait::async_trait;
use tracing::debug;
use uuid::Uuid;

use oco_shared_types::{
    AgentRole, ExecutionPlan, PlanStep, PlanStrategy, StepExecution, StepStatus, TeamCommunication,
    TeamConfig, TeamMember,
};

use crate::Planner;
use crate::context::PlanningContext;
use crate::error::PlannerError;
use crate::prompt;

/// LLM-powered planner for Medium+ tasks.
///
/// Calls an LLM with a structured prompt and parses the JSON response
/// into an `ExecutionPlan`. The plan structure emerges from the task —
/// no templates.
pub struct LlmPlanner {
    /// Function that calls the LLM. Abstracted for testability.
    /// Takes (system_prompt, user_message, max_tokens) → (response_text, tokens_used).
    llm_call: Box<dyn LlmCallFn>,
}

/// Trait object for LLM calls, allowing injection of real or stub providers.
#[async_trait]
pub trait LlmCallFn: Send + Sync {
    async fn call(
        &self,
        system_prompt: &str,
        user_message: &str,
        max_tokens: u32,
    ) -> Result<(String, u64), PlannerError>;
}

/// Stub LLM call for testing — returns a predefined plan JSON.
pub struct StubLlmCall {
    pub response: String,
}

#[async_trait]
impl LlmCallFn for StubLlmCall {
    async fn call(
        &self,
        _system: &str,
        _user: &str,
        _max_tokens: u32,
    ) -> Result<(String, u64), PlannerError> {
        Ok((self.response.clone(), 100))
    }
}

impl LlmPlanner {
    /// Create a planner with a custom LLM call function.
    pub fn new(llm_call: Box<dyn LlmCallFn>) -> Self {
        Self { llm_call }
    }

    /// Create a planner with a stub response (for testing).
    pub fn stub(response: impl Into<String>) -> Self {
        Self::new(Box::new(StubLlmCall {
            response: response.into(),
        }))
    }

    /// Parse LLM response into a list of PlanSteps.
    fn parse_steps(response: &str) -> Result<Vec<PlanStep>, PlannerError> {
        // Extract JSON from response (may be wrapped in markdown code blocks)
        let json_str = extract_json(response)?;

        let raw_steps: Vec<RawStep> = serde_json::from_str(&json_str).map_err(|e| {
            PlannerError::ParseError(format!("invalid JSON array: {e}\n\nRaw: {json_str}"))
        })?;

        if raw_steps.is_empty() {
            return Err(PlannerError::ParseError(
                "LLM returned empty step list".into(),
            ));
        }

        // First pass: create steps with temporary IDs, build name→id map.
        // Deduplicate names (LLM may produce duplicates): append suffix.
        let mut steps: Vec<PlanStep> = Vec::with_capacity(raw_steps.len());
        let mut name_to_id: std::collections::HashMap<String, Uuid> =
            std::collections::HashMap::new();
        let mut name_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();

        for raw in &raw_steps {
            let count = name_counts.entry(raw.name.clone()).or_insert(0);
            let unique_name = if *count == 0 {
                raw.name.clone()
            } else {
                format!("{}-{}", raw.name, count)
            };
            *count += 1;

            let step = PlanStep::new(&unique_name, &raw.description);
            name_to_id.insert(unique_name, step.id);
            // Also map original name to first occurrence for dep resolution
            name_to_id.entry(raw.name.clone()).or_insert(step.id);
            steps.push(step);
        }

        // Build an id-by-index lookup (avoids borrow conflicts in second pass)
        let id_by_index: Vec<Uuid> = steps.iter().map(|s| s.id).collect();

        // Second pass: fill in details and resolve dependency names to UUIDs
        for (i, raw) in raw_steps.iter().enumerate() {
            // Resolve dependencies before mutably borrowing steps[i]
            let mut deps = Vec::new();
            for dep in &raw.depends_on {
                if let Some(id) = name_to_id.get(dep) {
                    deps.push(*id);
                } else if let Ok(idx) = dep.parse::<usize>()
                    && let Some(&id) = id_by_index.get(idx)
                {
                    deps.push(id);
                }
                // Silently skip unresolvable deps (LLM may hallucinate)
            }

            let step = &mut steps[i];
            step.agent_role = raw.agent_role.clone().unwrap_or_default();
            step.execution = raw
                .execution
                .as_ref()
                .map(parse_execution)
                .unwrap_or(StepExecution::Inline);
            step.depends_on = deps;
            step.verify_after = raw.verify_after.unwrap_or(false);
            step.estimated_tokens = raw.estimated_tokens.unwrap_or(2000);
        }

        Ok(steps)
    }

    /// Generate a TeamConfig if there are parallelizable steps.
    fn generate_team(plan: &ExecutionPlan, context: &PlanningContext) -> Option<TeamConfig> {
        if !context.needs_team() {
            return None;
        }

        let groups = plan.parallel_groups();
        let has_parallel = groups.iter().any(|g| g.len() > 1);

        if !has_parallel {
            return None;
        }

        // Determine communication mode based on step interdependencies
        let comm = if has_interdependent_parallel_steps(plan) {
            TeamCommunication::Mesh // Agent Teams — need coordination
        } else {
            TeamCommunication::HubSpoke // Subagents — independent work
        };

        // Create team members from unique agent roles
        let mut members: Vec<TeamMember> = Vec::new();
        let mut seen_roles: std::collections::HashSet<String> = std::collections::HashSet::new();

        for step in &plan.steps {
            if seen_roles.insert(step.agent_role.name.clone()) {
                let assigned: Vec<Uuid> = plan
                    .steps
                    .iter()
                    .filter(|s| s.agent_role.name == step.agent_role.name)
                    .map(|s| s.id)
                    .collect();

                members.push(TeamMember {
                    agent_id: None,
                    role: step.agent_role.clone(),
                    assigned_steps: assigned,
                });
            }
        }

        Some(TeamConfig {
            name: format!("team-{}", &plan.id.to_string()[..8]),
            members,
            communication: comm,
        })
    }
}

/// A scored plan candidate from competitive planning.
#[derive(Debug, Clone)]
pub struct PlanCandidate {
    /// The generated plan.
    pub plan: ExecutionPlan,
    /// Planning strategy used ("balanced", "speed", "safety").
    pub strategy: String,
    /// Score (higher = better). Deterministic, no LLM call.
    pub score: f64,
    /// Tokens used for generation.
    pub tokens_used: u64,
    /// Whether this candidate was selected as winner.
    pub winner: bool,
}

impl LlmPlanner {
    /// Generate 2 competing plan candidates in parallel and return the best.
    ///
    /// Runs two LLM calls concurrently with different strategy biases
    /// (Speed vs Safety), scores them deterministically, and selects the winner.
    /// Returns all candidates (for visualization) and the winning plan.
    pub async fn plan_competitive(
        &self,
        request: &str,
        context: &PlanningContext,
    ) -> Result<(ExecutionPlan, Vec<PlanCandidate>), PlannerError> {
        let sys = prompt::system_prompt(context);
        let max_tokens = context.planning_token_budget();

        let user_speed = prompt::user_message_with_bias(request, context, prompt::PlanBias::Speed);
        let user_safety = prompt::user_message_with_bias(request, context, prompt::PlanBias::Safety);

        debug!("launching 2 competitive plan candidates (speed vs safety)");

        // Run both in parallel
        let (result_speed, result_safety) = tokio::join!(
            self.generate_one(&sys, &user_speed, max_tokens, context),
            self.generate_one(&sys, &user_safety, max_tokens, context),
        );

        let mut candidates: Vec<PlanCandidate> = Vec::new();

        if let Ok((plan, tokens)) = result_speed {
            let score = score_plan(&plan, context);
            candidates.push(PlanCandidate {
                plan,
                strategy: "speed".into(),
                score,
                tokens_used: tokens,
                winner: false,
            });
        }

        if let Ok((plan, tokens)) = result_safety {
            let score = score_plan(&plan, context);
            candidates.push(PlanCandidate {
                plan,
                strategy: "safety".into(),
                score,
                tokens_used: tokens,
                winner: false,
            });
        }

        if candidates.is_empty() {
            // Both failed — fall back to single plan
            return Err(PlannerError::LlmError(
                "all competitive plan candidates failed".into(),
            ));
        }

        // Select winner (highest score)
        let winner_idx = candidates
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.score.partial_cmp(&b.score).unwrap())
            .map(|(i, _)| i)
            .unwrap();

        candidates[winner_idx].winner = true;
        let winning_plan = candidates[winner_idx].plan.clone();

        debug!(
            winner = candidates[winner_idx].strategy,
            score = candidates[winner_idx].score,
            candidates = candidates.len(),
            "competitive planning complete"
        );

        Ok((winning_plan, candidates))
    }

    /// Generate a single plan (internal helper for competitive planning).
    async fn generate_one(
        &self,
        sys: &str,
        user: &str,
        max_tokens: u32,
        context: &PlanningContext,
    ) -> Result<(ExecutionPlan, u64), PlannerError> {
        let (response, tokens_used) = self.llm_call.call(sys, user, max_tokens).await?;

        let steps = match Self::parse_steps(&response) {
            Ok(s) => s,
            Err(first_err) => {
                let correction = format!(
                    "Your previous response could not be parsed.\nError: {first_err}\n\n\
                     Output ONLY a valid JSON array of steps."
                );
                let (retry_response, _) = self.llm_call.call(sys, &correction, max_tokens).await?;
                Self::parse_steps(&retry_response)?
            }
        };

        let model = "llm".to_string();
        let mut plan = ExecutionPlan::new(steps, PlanStrategy::Generated { model, tokens_used });
        plan.validate().map_err(|e| {
            PlannerError::ValidationError(format!("invalid DAG: {e}"))
        })?;
        plan.check_step_limit(context.task_complexity)
            .map_err(|e| PlannerError::ValidationError(format!("step limit: {e}")))?;
        plan.team = Self::generate_team(&plan, context);

        Ok((plan, tokens_used))
    }
}

/// Score a plan deterministically (no LLM call).
/// Higher = better. Considers: step count, verify coverage, parallelism, estimated cost.
fn score_plan(plan: &ExecutionPlan, context: &PlanningContext) -> f64 {
    let step_count = plan.steps.len() as f64;
    let verify_count = plan.steps.iter().filter(|s| s.verify_after).count() as f64;
    let parallel_groups = plan.parallel_groups().len() as f64;
    let critical_path = plan.critical_path_length() as f64;
    let total_tokens: u64 = plan.steps.iter().map(|s| s.estimated_tokens as u64).sum();
    let budget = context.budget.max_total_tokens as f64;

    // Verify coverage ratio (0-1): higher = more verification = safer
    let verify_ratio = if step_count > 0.0 { verify_count / step_count } else { 0.0 };

    // Parallelism ratio: fewer groups relative to steps = more parallelism
    let parallel_ratio = if step_count > 1.0 { 1.0 - (parallel_groups / step_count) } else { 0.0 };

    // Cost efficiency: lower token estimate relative to budget = better
    let cost_ratio = 1.0 - (total_tokens as f64 / budget).min(1.0);

    // Depth efficiency: shallower critical path = faster
    let depth_ratio = 1.0 - (critical_path / step_count.max(1.0));

    // Weighted score
    let score = verify_ratio * 0.35       // safety matters most
        + cost_ratio * 0.25              // token efficiency
        + parallel_ratio * 0.20          // parallelism
        + depth_ratio * 0.15             // shallow plans execute faster
        + (1.0 / step_count.max(1.0)) * 0.05;  // slight preference for simplicity

    score.clamp(0.0, 1.0)
}

#[async_trait]
impl Planner for LlmPlanner {
    async fn plan(
        &self,
        request: &str,
        context: &PlanningContext,
    ) -> Result<ExecutionPlan, PlannerError> {
        let sys = prompt::system_prompt(context);
        let user = prompt::user_message(request, context);
        let max_tokens = context.planning_token_budget();

        debug!(
            complexity = ?context.task_complexity,
            category = ?context.task_category,
            budget = max_tokens,
            "generating execution plan via LLM"
        );

        let (response, tokens_used) = self.llm_call.call(&sys, &user, max_tokens).await?;

        debug!(
            tokens_used,
            response_len = response.len(),
            "LLM plan response received"
        );

        // Fix #22: retry with correction prompt on parse failure (max 2 attempts)
        let steps = match Self::parse_steps(&response) {
            Ok(s) => s,
            Err(first_err) => {
                debug!(error = %first_err, "first parse attempt failed, retrying with correction prompt");
                let correction = format!(
                    "Your previous response could not be parsed as a valid JSON array of steps.\n\
                     Error: {first_err}\n\n\
                     Please output ONLY a valid JSON array (no markdown, no explanation) matching this schema:\n\
                     [{{\"name\": \"...\", \"description\": \"...\", \"depends_on\": [\"...\"], ...}}]"
                );
                let (retry_response, _) = self.llm_call.call(&sys, &correction, max_tokens).await?;
                Self::parse_steps(&retry_response).map_err(|retry_err| {
                    PlannerError::ParseError(format!(
                        "parse failed after retry.\nFirst: {first_err}\nRetry: {retry_err}"
                    ))
                })?
            }
        };

        let model = "llm".to_string(); // In production, comes from provider
        let mut plan = ExecutionPlan::new(steps, PlanStrategy::Generated { model, tokens_used });

        // Validate the DAG
        plan.validate().map_err(|e| {
            PlannerError::ValidationError(format!("generated plan has invalid DAG: {e}"))
        })?;

        // Enforce hard step count limit per complexity tier
        plan.check_step_limit(context.task_complexity)
            .map_err(|e| PlannerError::ValidationError(format!("plan exceeds step limit: {e}")))?;

        // Generate team config if warranted
        plan.team = Self::generate_team(&plan, context);

        Ok(plan)
    }

    async fn replan(
        &self,
        original: &ExecutionPlan,
        failed_step: &PlanStep,
        error_context: &str,
        context: &PlanningContext,
    ) -> Result<ExecutionPlan, PlannerError> {
        let completed_names: Vec<String> = original
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .map(|s| s.name.clone())
            .collect();

        let sys = prompt::system_prompt(context);
        let user = prompt::replan_message(
            &context.working_memory_summary,
            &failed_step.name,
            error_context,
            &completed_names,
        );
        let max_tokens = context.planning_token_budget();

        let (response, _tokens_used) = self.llm_call.call(&sys, &user, max_tokens).await?;

        let new_steps = Self::parse_steps(&response)?;

        // Fix #21: transaction-safe merge that handles all step states
        let mut merged_steps: Vec<PlanStep> =
            Vec::with_capacity(original.steps.len() + new_steps.len());

        for step in &original.steps {
            match &step.status {
                // Keep completed steps as-is
                StepStatus::Completed => {
                    merged_steps.push(step.clone());
                }
                // Keep in-progress steps as-is — they're being executed right now
                StepStatus::InProgress => {
                    merged_steps.push(step.clone());
                }
                // Mark failed, pending, and blocked steps as Replanned
                StepStatus::Failed { .. } | StepStatus::Pending | StepStatus::Blocked => {
                    let mut replaced = step.clone();
                    replaced.status = StepStatus::Replanned;
                    merged_steps.push(replaced);
                }
                // Already replanned steps stay replanned
                StepStatus::Replanned => {
                    merged_steps.push(step.clone());
                }
            }
        }

        merged_steps.extend(new_steps);

        let plan = ExecutionPlan::new(
            merged_steps,
            PlanStrategy::Replanned {
                original_plan_id: original.id,
                failed_step_id: failed_step.id,
            },
        );

        // Team regeneration
        let mut plan = plan;
        plan.team = Self::generate_team(&plan, context);

        Ok(plan)
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Raw step as parsed from LLM JSON output.
#[derive(Debug, serde::Deserialize)]
struct RawStep {
    name: String,
    description: String,
    #[serde(default)]
    agent_role: Option<AgentRole>,
    #[serde(default)]
    execution: Option<serde_json::Value>,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    verify_after: Option<bool>,
    #[serde(default)]
    estimated_tokens: Option<u32>,
}

/// Extract JSON from a response that may be wrapped in markdown code blocks (fix #22).
///
/// Layered extraction strategy:
/// 1. Direct JSON array
/// 2. Last ```json ... ``` fence (prefer last — LLM often wraps corrections)
/// 3. Last ``` ... ``` fence containing a JSON array
/// 4. Bracket matching: outermost [ ... ]
fn extract_json(response: &str) -> Result<String, PlannerError> {
    let trimmed = response.trim();

    // 1. Direct JSON array
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return Ok(trimmed.to_string());
    }

    // 2. Find the LAST ```json ... ``` block (handles multiple fences)
    let mut last_json_fence: Option<String> = None;
    let mut search = trimmed;
    while let Some(start) = search.find("```json") {
        let after = &search[start + 7..];
        if let Some(end) = after.find("```") {
            last_json_fence = Some(after[..end].trim().to_string());
            search = &after[end + 3..];
        } else {
            // Unclosed fence — try to extract from start to end of string
            let candidate = after.trim();
            if candidate.starts_with('[') {
                last_json_fence = Some(candidate.to_string());
            }
            break;
        }
    }
    if let Some(ref json) = last_json_fence
        && json.starts_with('[')
    {
        return Ok(json.clone());
    }

    // 3. Find the LAST ``` ... ``` block containing a JSON array
    let mut search = trimmed;
    while let Some(start) = search.find("```") {
        let after = &search[start + 3..];
        if let Some(end) = after.find("```") {
            let inner = after[..end].trim();
            // Skip the "json" language tag if present
            let inner = inner
                .strip_prefix("json")
                .map(|s| s.trim())
                .unwrap_or(inner);
            if inner.starts_with('[') {
                last_json_fence = Some(inner.to_string());
            }
            search = &after[end + 3..];
        } else {
            break;
        }
    }
    if let Some(json) = last_json_fence
        && json.starts_with('[')
    {
        return Ok(json);
    }

    // 4. Bracket matching: find first [ and last ]
    if let (Some(start), Some(end)) = (trimmed.find('['), trimmed.rfind(']'))
        && start < end
    {
        return Ok(trimmed[start..=end].to_string());
    }

    Err(PlannerError::ParseError(format!(
        "could not extract JSON array from response: {}",
        &trimmed[..trimmed.len().min(200)]
    )))
}

/// Parse the "execution" field from LLM JSON into a StepExecution.
fn parse_execution(value: &serde_json::Value) -> StepExecution {
    let mode = value
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("inline");

    match mode {
        "subagent" => StepExecution::Subagent {
            model: value.get("model").and_then(|v| v.as_str()).map(Into::into),
        },
        "teammate" => StepExecution::Teammate {
            team_name: value
                .get("team_name")
                .and_then(|v| v.as_str())
                .unwrap_or("default-team")
                .into(),
        },
        "mcp_tool" => StepExecution::McpTool {
            server: value
                .get("server")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .into(),
            tool: value
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .into(),
        },
        _ => StepExecution::Inline,
    }
}

/// Check if parallel steps have cross-references (need mesh communication).
fn has_interdependent_parallel_steps(plan: &ExecutionPlan) -> bool {
    let groups = plan.parallel_groups();
    for group in &groups {
        if group.len() <= 1 {
            continue;
        }
        // Check if any step in this group references files/capabilities
        // that another step in the same group also modifies.
        // Heuristic: if any parallel step has verify_after AND is not read_only,
        // they may conflict.
        let writers: Vec<&PlanStep> = group
            .iter()
            .filter_map(|id| plan.get_step(*id))
            .filter(|s| !s.agent_role.read_only && s.verify_after)
            .collect();
        if writers.len() > 1 {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::{TaskCategory, TaskComplexity};

    fn medium_ctx() -> PlanningContext {
        PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature)
    }

    fn high_ctx() -> PlanningContext {
        PlanningContext::minimal(TaskComplexity::High, TaskCategory::NewFeature)
    }

    // -- JSON extraction --

    #[test]
    fn extract_json_direct() {
        let input = r#"[{"name": "test"}]"#;
        assert!(extract_json(input).unwrap().starts_with('['));
    }

    #[test]
    fn extract_json_from_markdown() {
        let input = "Here's the plan:\n```json\n[{\"name\": \"test\"}]\n```\n";
        let result = extract_json(input).unwrap();
        assert!(result.starts_with('['));
    }

    #[test]
    fn extract_json_with_preamble() {
        let input = "Sure, here's your plan: [{\"name\": \"test\"}] hope this helps!";
        let result = extract_json(input).unwrap();
        assert!(result.starts_with('['));
    }

    #[test]
    fn extract_json_no_array_fails() {
        let input = "I can't generate a plan for this.";
        assert!(extract_json(input).is_err());
    }

    #[test]
    fn extract_json_two_fences_uses_last() {
        let input = "Draft:\n```json\n[{\"name\": \"wrong\"}]\n```\nActually:\n```json\n[{\"name\": \"correct\"}]\n```\n";
        let result = extract_json(input).unwrap();
        assert!(result.contains("correct"));
        assert!(!result.contains("wrong"));
    }

    #[test]
    fn extract_json_unclosed_fence() {
        let input = "```json\n[{\"name\": \"truncated\"}]";
        let result = extract_json(input).unwrap();
        assert!(result.contains("truncated"));
    }

    #[test]
    fn extract_json_bare_fence_with_array() {
        let input = "```\n[{\"name\": \"bare\"}]\n```";
        let result = extract_json(input).unwrap();
        assert!(result.contains("bare"));
    }

    // -- parse_execution --

    #[test]
    fn parse_execution_inline() {
        let val = serde_json::json!({"mode": "inline"});
        assert_eq!(parse_execution(&val), StepExecution::Inline);
    }

    #[test]
    fn parse_execution_subagent() {
        let val = serde_json::json!({"mode": "subagent", "model": "haiku"});
        assert!(matches!(
            parse_execution(&val),
            StepExecution::Subagent { model: Some(m) } if m == "haiku"
        ));
    }

    #[test]
    fn parse_execution_teammate() {
        let val = serde_json::json!({"mode": "teammate", "team_name": "auth-crew"});
        assert!(matches!(
            parse_execution(&val),
            StepExecution::Teammate { team_name } if team_name == "auth-crew"
        ));
    }

    #[test]
    fn parse_execution_mcp() {
        let val = serde_json::json!({"mode": "mcp_tool", "server": "yoyo", "tool": "search"});
        assert!(matches!(
            parse_execution(&val),
            StepExecution::McpTool { server, tool } if server == "yoyo" && tool == "search"
        ));
    }

    // -- LlmPlanner with stub --

    #[tokio::test]
    async fn llm_planner_generates_valid_plan() {
        let response = serde_json::json!([
            {
                "name": "investigate",
                "description": "Search for relevant code",
                "agent_role": {"name": "explorer", "required_capabilities": ["code_search"], "read_only": true},
                "execution": {"mode": "subagent", "model": "haiku"},
                "depends_on": [],
                "verify_after": false,
                "estimated_tokens": 1000
            },
            {
                "name": "implement",
                "description": "Write the feature code",
                "agent_role": {"name": "coder", "required_capabilities": ["file_edit"]},
                "execution": {"mode": "inline"},
                "depends_on": ["investigate"],
                "verify_after": true,
                "estimated_tokens": 3000
            },
            {
                "name": "test",
                "description": "Write and run tests",
                "agent_role": {"name": "tester", "required_capabilities": ["shell_exec"]},
                "execution": {"mode": "inline"},
                "depends_on": ["implement"],
                "verify_after": true,
                "estimated_tokens": 2000
            }
        ]);

        let planner = LlmPlanner::stub(response.to_string());
        let ctx = medium_ctx();
        let plan = planner.plan("add JWT auth", &ctx).await.unwrap();

        assert_eq!(plan.steps.len(), 3);
        assert!(plan.validate().is_ok());
        assert_eq!(plan.steps[0].name, "investigate");
        assert!(plan.steps[0].agent_role.read_only);
        assert!(matches!(
            plan.steps[0].execution,
            StepExecution::Subagent { .. }
        ));
        assert_eq!(plan.steps[1].depends_on.len(), 1);
        assert!(plan.steps[2].verify_after);
        assert_eq!(plan.critical_path_length(), 3);

        // No team for Medium complexity
        assert!(plan.team.is_none());
    }

    #[tokio::test]
    async fn llm_planner_parallel_steps() {
        let response = serde_json::json!([
            {
                "name": "investigate",
                "description": "Analyze codebase",
                "depends_on": []
            },
            {
                "name": "implement-api",
                "description": "Build API endpoints",
                "agent_role": {"name": "backend", "required_capabilities": ["file_edit"]},
                "depends_on": ["investigate"],
                "verify_after": true
            },
            {
                "name": "implement-tests",
                "description": "Write test suite",
                "agent_role": {"name": "tester", "required_capabilities": ["file_edit"]},
                "depends_on": ["investigate"],
                "verify_after": true
            },
            {
                "name": "verify",
                "description": "Run full verification",
                "depends_on": ["implement-api", "implement-tests"],
                "verify_after": true
            }
        ]);

        let planner = LlmPlanner::stub(response.to_string());
        let ctx = high_ctx();
        let plan = planner.plan("add JWT auth", &ctx).await.unwrap();

        assert_eq!(plan.steps.len(), 4);
        assert!(plan.validate().is_ok());

        let groups = plan.parallel_groups();
        assert_eq!(groups.len(), 3); // [investigate], [api, tests], [verify]
        assert_eq!(groups[1].len(), 2); // parallel group

        // High complexity with parallel writers → team with Mesh
        assert!(plan.team.is_some());
        let team = plan.team.unwrap();
        assert_eq!(team.communication, TeamCommunication::Mesh);
    }

    #[tokio::test]
    async fn llm_planner_markdown_wrapped_response() {
        let response = "Here's the plan:\n```json\n[{\"name\": \"do-it\", \"description\": \"Execute the task\", \"depends_on\": []}]\n```\nLet me know if you need changes.";

        let planner = LlmPlanner::stub(response);
        let plan = planner.plan("test", &medium_ctx()).await.unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].name, "do-it");
    }

    #[tokio::test]
    async fn llm_planner_empty_response_fails() {
        let planner = LlmPlanner::stub("[]");
        let result = planner.plan("test", &medium_ctx()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn llm_planner_invalid_json_retries() {
        // Fix #22: first call returns garbage, retry also returns garbage → error
        let planner = LlmPlanner::stub("this is not json at all");
        let result = planner.plan("test", &medium_ctx()).await;
        assert!(result.is_err());
        // Error message should mention "retry"
        let err = result.unwrap_err().to_string();
        assert!(err.contains("retry"), "error should mention retry: {err}");
    }

    #[tokio::test]
    async fn llm_planner_retry_succeeds_on_second_attempt() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};

        // Custom LLM call that returns garbage first, then valid JSON
        struct RetryLlm {
            call_count: Arc<AtomicU32>,
        }

        #[async_trait]
        impl LlmCallFn for RetryLlm {
            async fn call(
                &self,
                _system: &str,
                _user: &str,
                _max_tokens: u32,
            ) -> Result<(String, u64), PlannerError> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    // First call: valid task but mangled JSON
                    Ok(("oops not json".into(), 50))
                } else {
                    // Retry: valid JSON
                    Ok((
                        r#"[{"name": "fixed", "description": "Works now", "depends_on": []}]"#
                            .into(),
                        50,
                    ))
                }
            }
        }

        let planner = LlmPlanner::new(Box::new(RetryLlm {
            call_count: Arc::new(AtomicU32::new(0)),
        }));
        let plan = planner.plan("test", &medium_ctx()).await.unwrap();
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].name, "fixed");
    }

    // -- Replan --

    #[tokio::test]
    async fn replan_preserves_completed_steps() {
        // Original plan: 2 steps, first completed, second failed
        let step1 = PlanStep::new("investigate", "Search code");
        let mut step2 = PlanStep::new("implement", "Write code");
        step2.depends_on = vec![step1.id];

        let mut original = ExecutionPlan::new(
            vec![step1, step2],
            PlanStrategy::Generated {
                model: "test".into(),
                tokens_used: 100,
            },
        );
        original.steps[0].status = StepStatus::Completed;
        original.steps[1].status = StepStatus::Failed {
            reason: "test failed".into(),
        };

        let replan_response = serde_json::json!([
            {
                "name": "fix-and-implement",
                "description": "Fix the issue and re-implement",
                "depends_on": [],
                "verify_after": true
            }
        ]);

        let planner = LlmPlanner::stub(replan_response.to_string());
        let failed = original.steps[1].clone();
        let plan = planner
            .replan(&original, &failed, "assertion error", &medium_ctx())
            .await
            .unwrap();

        // Should have: completed step + replanned step + new step
        assert!(
            plan.steps
                .iter()
                .any(|s| s.name == "investigate" && s.status == StepStatus::Completed)
        );
        assert!(plan.steps.iter().any(|s| s.name == "fix-and-implement"));
        assert!(matches!(plan.strategy, PlanStrategy::Replanned { .. }));
    }

    #[tokio::test]
    async fn replan_preserves_in_progress_steps() {
        // Fix #21: in-progress steps must survive replan
        let step1 = PlanStep::new("investigate", "Search code");
        let mut step2 = PlanStep::new("implement-a", "Write API");
        step2.depends_on = vec![step1.id];
        let mut step3 = PlanStep::new("implement-b", "Write tests");
        step3.depends_on = vec![step1.id];

        let mut original = ExecutionPlan::new(
            vec![step1, step2, step3],
            PlanStrategy::Generated {
                model: "test".into(),
                tokens_used: 100,
            },
        );
        original.steps[0].status = StepStatus::Completed;
        original.steps[1].status = StepStatus::InProgress; // currently executing
        original.steps[2].status = StepStatus::Failed {
            reason: "compile error".into(),
        };

        let replan_response = serde_json::json!([
            {
                "name": "fix-tests",
                "description": "Fix and retry tests",
                "depends_on": [],
                "verify_after": true
            }
        ]);

        let planner = LlmPlanner::stub(replan_response.to_string());
        let failed = original.steps[2].clone();
        let plan = planner
            .replan(&original, &failed, "compile error", &medium_ctx())
            .await
            .unwrap();

        // Completed step preserved
        assert!(
            plan.steps
                .iter()
                .any(|s| s.name == "investigate" && s.status == StepStatus::Completed)
        );
        // In-progress step preserved (NOT dropped or replanned)
        assert!(
            plan.steps
                .iter()
                .any(|s| s.name == "implement-a" && s.status == StepStatus::InProgress)
        );
        // Failed step marked as Replanned
        assert!(
            plan.steps
                .iter()
                .any(|s| s.name == "implement-b" && s.status == StepStatus::Replanned)
        );
        // New step added
        assert!(
            plan.steps
                .iter()
                .any(|s| s.name == "fix-tests" && s.status == StepStatus::Pending)
        );
    }

    // -- Team generation --

    #[test]
    fn no_team_for_sequential_plan() {
        let a = PlanStep::new("a", "first");
        let mut b = PlanStep::new("b", "second");
        b.depends_on = vec![a.id];
        let plan = ExecutionPlan::new(vec![a, b], PlanStrategy::Direct);
        let ctx = high_ctx();

        let team = LlmPlanner::generate_team(&plan, &ctx);
        assert!(team.is_none()); // no parallel steps
    }

    #[test]
    fn mesh_team_for_parallel_writers() {
        let root = PlanStep::new("root", "setup");
        let mut w1 = PlanStep::new("writer-1", "write api")
            .with_role(AgentRole::new("coder"))
            .with_verify();
        w1.depends_on = vec![root.id];
        let mut w2 = PlanStep::new("writer-2", "write tests")
            .with_role(AgentRole::new("tester"))
            .with_verify();
        w2.depends_on = vec![root.id];

        let plan = ExecutionPlan::new(vec![root, w1, w2], PlanStrategy::Direct);
        let ctx = high_ctx();

        let team = LlmPlanner::generate_team(&plan, &ctx).unwrap();
        assert_eq!(team.communication, TeamCommunication::Mesh);
        assert!(team.members.len() >= 2);
    }

    // -- Toxic scenarios (fix #24) --

    #[tokio::test]
    async fn duplicate_step_names_disambiguated() {
        let response = serde_json::json!([
            {"name": "implement", "description": "First", "depends_on": []},
            {"name": "implement", "description": "Second", "depends_on": ["implement"]},
            {"name": "implement", "description": "Third", "depends_on": ["implement-1"]}
        ]);

        let planner = LlmPlanner::stub(response.to_string());
        let plan = planner.plan("test", &medium_ctx()).await.unwrap();

        assert_eq!(plan.steps.len(), 3);
        // Names should be deduplicated
        let names: Vec<&str> = plan.steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names[0], "implement");
        assert_eq!(names[1], "implement-1");
        assert_eq!(names[2], "implement-2");
        // Dependencies should resolve correctly
        assert!(plan.validate().is_ok());
    }

    #[tokio::test]
    async fn circular_dependency_detected() {
        // LLM produces steps with circular deps via names
        let response = serde_json::json!([
            {"name": "a", "description": "Step A", "depends_on": ["b"]},
            {"name": "b", "description": "Step B", "depends_on": ["a"]}
        ]);

        let planner = LlmPlanner::stub(response.to_string());
        let result = planner.plan("test", &medium_ctx()).await;
        // Should fail validation (cycle detected)
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("cycle") || err.contains("invalid DAG"),
            "error: {err}"
        );
    }

    #[tokio::test]
    async fn deeply_nested_deps_accepted() {
        // 7 steps in a linear chain — matches Medium complexity max (7 steps)
        let steps: Vec<serde_json::Value> = (0..7)
            .map(|i| {
                let deps = if i == 0 {
                    vec![]
                } else {
                    vec![format!("step-{}", i - 1)]
                };
                serde_json::json!({
                    "name": format!("step-{i}"),
                    "description": format!("Step {i}"),
                    "depends_on": deps
                })
            })
            .collect();

        let planner = LlmPlanner::stub(serde_json::to_string(&steps).unwrap());
        let plan = planner.plan("test", &medium_ctx()).await.unwrap();
        assert_eq!(plan.steps.len(), 7);
        assert_eq!(plan.critical_path_length(), 7);
        assert!(plan.validate().is_ok());
    }

    #[tokio::test]
    async fn step_limit_rejects_over_planned() {
        // 10 steps for Medium complexity (max 7) — should be rejected
        let steps: Vec<serde_json::Value> = (0..10)
            .map(|i| {
                let deps = if i == 0 {
                    vec![]
                } else {
                    vec![format!("step-{}", i - 1)]
                };
                serde_json::json!({
                    "name": format!("step-{i}"),
                    "description": format!("Step {i}"),
                    "depends_on": deps
                })
            })
            .collect();

        let planner = LlmPlanner::stub(serde_json::to_string(&steps).unwrap());
        let result = planner.plan("test", &medium_ctx()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("step limit"), "error: {err}");
    }

    #[tokio::test]
    async fn high_complexity_allows_more_steps() {
        // 10 steps for High complexity (max 10) — should be accepted
        let steps: Vec<serde_json::Value> = (0..10)
            .map(|i| {
                let deps = if i == 0 {
                    vec![]
                } else {
                    vec![format!("step-{}", i - 1)]
                };
                serde_json::json!({
                    "name": format!("step-{i}"),
                    "description": format!("Step {i}"),
                    "depends_on": deps
                })
            })
            .collect();

        let planner = LlmPlanner::stub(serde_json::to_string(&steps).unwrap());
        let plan = planner.plan("test", &high_ctx()).await.unwrap();
        assert_eq!(plan.steps.len(), 10);
    }

    #[test]
    fn hubspoke_for_parallel_readers() {
        let root = PlanStep::new("root", "setup");
        let mut r1 = PlanStep::new("reader-1", "analyze api")
            .with_role(AgentRole::new("analyzer").read_only());
        r1.depends_on = vec![root.id];
        let mut r2 = PlanStep::new("reader-2", "analyze tests")
            .with_role(AgentRole::new("analyzer").read_only());
        r2.depends_on = vec![root.id];

        let plan = ExecutionPlan::new(vec![root, r1, r2], PlanStrategy::Direct);
        let ctx = high_ctx();

        let team = LlmPlanner::generate_team(&plan, &ctx).unwrap();
        assert_eq!(team.communication, TeamCommunication::HubSpoke);
    }
}
