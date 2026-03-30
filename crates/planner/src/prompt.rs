//! Prompt generation for the LLM planner.
//!
//! Builds the system prompt and user message that instruct the LLM to produce
//! a structured JSON `ExecutionPlan`.

use crate::context::PlanningContext;

/// Build the system prompt for plan generation.
pub fn system_prompt(context: &PlanningContext) -> String {
    let risk_str = format!("{:?}", context.repo_profile.risk_level);
    let complexity_str = format!("{:?}", context.task_complexity);
    let category_str = format!("{:?}", context.task_category);
    format!(
        r#"You are a software engineering task planner. Your job is to decompose a user request into an execution plan — a DAG of steps.

## Rules

1. Each step has: name, description, agent_role, execution mode, dependencies, and verify_after flag.
2. Steps can depend on other steps (DAG — no cycles).
3. Steps that can run in parallel SHOULD have the same dependencies (not each other).
4. Set verify_after: true for any step that modifies code.
5. Use read_only roles for investigation/review steps.
6. Keep plans shallow: 2-5 steps for Medium, 3-8 for High, 5-12 for Critical.
7. Each step must use only capabilities available in the registry below.

## Execution Modes

- "inline": run in the main orchestration loop (default, for simple steps)
- "subagent": spawn an isolated agent with filtered context (for focused work)
- "teammate": spawn as a team member with mesh communication (for interdependent parallel work)
- "mcp_tool": delegate to an MCP tool directly

## Available Capabilities

### Tools
{tools}

### MCP Tools
{mcp}

### Agents
{agents}

### Skills
{skills}

### LLM Models
{llms}

## Repo Profile

- Language: {language}
- Risk level: {risk}
- Test framework: {test_framework}

## Task Info

- Complexity: {complexity:?}
- Category: {category:?}

## Output Format

Respond with ONLY a JSON array of steps. No markdown, no explanation.

```json
[
  {{
    "name": "step-name",
    "description": "What this step does",
    "agent_role": {{
      "name": "role-name",
      "required_capabilities": ["cap1", "cap2"],
      "preferred_model": "sonnet",
      "read_only": false
    }},
    "execution": {{ "mode": "inline" }},
    "depends_on": [],
    "verify_after": true,
    "estimated_tokens": 2000
  }}
]
```

For subagent mode: {{"mode": "subagent", "model": "haiku"}}
For teammate mode: {{"mode": "teammate", "team_name": "feature-crew"}}
For mcp_tool mode: {{"mode": "mcp_tool", "server": "yoyo", "tool": "search"}}

Subagent/teammate steps may include a "sub_plan" field with nested steps (max depth 2):
{{"name": "delegate-analysis", "execution": {{"mode": "subagent"}}, "sub_plan": [{{"name": "sub-task", "description": "...", "depends_on": [], "estimated_tokens": 1000}}]}}
"#,
        tools = format_items(&context.capabilities.tools),
        mcp = format_items(&context.capabilities.mcp),
        agents = format_items(&context.capabilities.agents),
        skills = format_items(&context.capabilities.skills),
        llms = format_items(&context.capabilities.llms),
        language = context.repo_profile.stack,
        risk = risk_str,
        test_framework = context
            .repo_profile
            .test_command
            .as_deref()
            .unwrap_or("unknown"),
        complexity = complexity_str,
        category = category_str,
    )
}

/// Planning strategy bias — used by competitive planning to generate diverse candidates.
#[derive(Debug, Clone, Copy)]
pub enum PlanBias {
    /// Balanced (default) — no bias.
    Balanced,
    /// Favor fewer steps, less verification — faster but riskier.
    Speed,
    /// Favor more verification gates, conservative approach — slower but safer.
    Safety,
}

/// Build the user message for plan generation.
/// Includes failure-first risk analysis (#67 wiring) for Medium+ tasks.
pub fn user_message(request: &str, context: &PlanningContext) -> String {
    user_message_with_bias(request, context, PlanBias::Balanced)
}

/// Build the user message with a specific planning bias.
pub fn user_message_with_bias(request: &str, context: &PlanningContext, bias: PlanBias) -> String {
    let mut msg = format!("Generate an execution plan for this task:\n\n{request}");

    if !context.working_memory_summary.is_empty() {
        msg.push_str(&format!(
            "\n\n## Prior Knowledge\n\n{}",
            context.working_memory_summary
        ));
    }

    // Inject failure-first risk analysis for non-trivial tasks (#67)
    if context.needs_planning() {
        let preview = crate::risk_analysis::analyze_risks(request, context);
        if !preview.risks.is_empty() {
            msg.push_str("\n\n## Failure-First Analysis\n\nHow this task is likely to fail if treated naively:\n");
            for risk in &preview.risks {
                msg.push_str(&format!(
                    "\n- **{}** (likelihood: {:.0}%, severity: {:.0}%): {}\n  Mitigation: {}",
                    risk.id,
                    risk.likelihood * 100.0,
                    risk.severity * 100.0,
                    risk.description,
                    risk.mitigation,
                ));
            }
            if !preview.uncertainties.is_empty() {
                msg.push_str("\n\nUncertainties:");
                for u in &preview.uncertainties {
                    msg.push_str(&format!("\n- {u}"));
                }
            }
            if !preview.suggested_verify_gates.is_empty() {
                msg.push_str(&format!(
                    "\n\nIMPORTANT: Set verify_after: true on implementation steps. Suggested checks: {}",
                    preview.suggested_verify_gates.join(", ")
                ));
            }
        }
    }

    // Append bias hint
    match bias {
        PlanBias::Balanced => {}
        PlanBias::Speed => {
            msg.push_str("\n\n## Strategy: SPEED\nOptimize for minimal steps and fast execution. Use fewer verify gates. Prefer inline execution. Combine related work into single steps where possible. Minimize token cost.");
        }
        PlanBias::Safety => {
            msg.push_str("\n\n## Strategy: SAFETY\nOptimize for reliability. Add verify_after on every implementation step. Break complex steps into smaller, independently verifiable units. Prefer subagent execution for isolation. Add a final integration test step.");
        }
    }

    msg
}

/// Build the user message for replanning after a failure.
pub fn replan_message(
    request: &str,
    failed_step_name: &str,
    error_context: &str,
    completed_steps: &[String],
) -> String {
    let completed = if completed_steps.is_empty() {
        "None".to_string()
    } else {
        completed_steps.join(", ")
    };

    format!(
        r#"The original task was: {request}

Step "{failed_step_name}" FAILED with error:
{error_context}

Already completed steps: {completed}

Generate a NEW plan that:
1. Does NOT repeat completed steps.
2. Fixes the issue that caused the failure.
3. Completes the remaining work.

Respond with ONLY a JSON array of NEW steps."#
    )
}

fn format_items(items: &[oco_shared_types::SummaryItem]) -> String {
    if items.is_empty() {
        return "(none)".to_string();
    }
    items
        .iter()
        .map(|i| {
            if i.capabilities.is_empty() {
                format!("- {}: {}", i.id, i.name)
            } else {
                format!("- {}: {} [{}]", i.id, i.name, i.capabilities.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use oco_shared_types::{TaskCategory, TaskComplexity};

    #[test]
    fn system_prompt_contains_key_sections() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Bug);
        let prompt = system_prompt(&ctx);

        assert!(prompt.contains("Available Capabilities"));
        assert!(prompt.contains("Repo Profile"));
        assert!(prompt.contains("Output Format"));
        assert!(prompt.contains("verify_after"));
        assert!(prompt.contains("Medium"));
        assert!(prompt.contains("Bug"));
    }

    #[test]
    fn user_message_includes_request() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::General);
        let msg = user_message("add JWT auth", &ctx);
        assert!(msg.contains("add JWT auth"));
    }

    #[test]
    fn user_message_includes_memory() {
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::General);
        ctx.working_memory_summary = "Found auth module in src/auth.rs".into();
        let msg = user_message("add JWT auth", &ctx);
        assert!(msg.contains("Prior Knowledge"));
        assert!(msg.contains("auth module"));
    }

    #[test]
    fn replan_message_includes_context() {
        let msg = replan_message(
            "add JWT auth",
            "implement-middleware",
            "test failed: 401 Unauthorized",
            &["investigate".into()],
        );
        assert!(msg.contains("implement-middleware"));
        assert!(msg.contains("401 Unauthorized"));
        assert!(msg.contains("investigate"));
    }
}
