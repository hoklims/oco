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
8. When the user message includes a "Prior Art Research" section, you MUST include the recommended research steps before any implementation step. Research steps should use subagent execution mode with read_only roles and web_search capability.

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

    // Inject prior-art research recommendation for non-trivial tasks
    if context.needs_planning() {
        let prior_art = crate::prior_art::analyze_prior_art(request, context);
        if prior_art.any() {
            msg.push_str("\n\n## Prior Art Research\n\n");
            msg.push_str(&prior_art.rationale);
            msg.push_str(
                "\n\nIMPORTANT: Add research steps BEFORE implementation steps in the plan:\n",
            );
            if prior_art.should_search_oss {
                msg.push_str(&format!(
                    "- Add a 'research-oss' step (subagent, read_only, web_search) to find existing open-source solutions. Search hints: {}\n",
                    prior_art.oss_search_hints.join(", ")
                ));
                if !prior_art.registries.is_empty() {
                    msg.push_str(&format!(
                        "  Check registries: {}\n",
                        prior_art.registries.join(", ")
                    ));
                }
            }
            if prior_art.should_search_papers {
                msg.push_str(&format!(
                    "- Add a 'research-papers' step (subagent, read_only, web_search) to find recent research papers. Search hints: {}\n",
                    prior_art.paper_search_hints.join(", ")
                ));
            }
            msg.push_str("- Add a 'synthesize-research' step that depends on the research steps, summarizing findings and recommending whether to use an existing solution or build from scratch.\n");
            msg.push_str("- Implementation steps must depend on 'synthesize-research'.\n");
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
///
/// The prompt includes the full failure context (step output + verification
/// output) and explicitly asks the planner to change approach, not just retry.
pub fn replan_message(
    request: &str,
    failed_step_name: &str,
    error_context: &str,
    completed_steps: &[String],
    failed_step_output: Option<&str>,
) -> String {
    let completed = if completed_steps.is_empty() {
        "None".to_string()
    } else {
        completed_steps.join(", ")
    };

    let step_output_section = if let Some(output) = failed_step_output {
        let truncated = if output.len() > 2000 {
            &output[..2000]
        } else {
            output
        };
        format!("\n\nStep output before failure (truncated to 2k):\n{truncated}")
    } else {
        String::new()
    };

    format!(
        r#"The original task was: {request}

Step "{failed_step_name}" FAILED with error:
{error_context}{step_output_section}

Already completed steps: {completed}

Generate a NEW plan that:
1. Does NOT repeat completed steps — their outputs are already available.
2. Takes a DIFFERENT approach to fix the failure — do not retry the exact same strategy.
3. Explicitly states what changed in the approach compared to the failed step.
4. Completes the remaining work toward the original goal.

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
            None,
        );
        assert!(msg.contains("implement-middleware"));
        assert!(msg.contains("401 Unauthorized"));
        assert!(msg.contains("investigate"));
        assert!(msg.contains("DIFFERENT approach"));
    }

    #[test]
    fn replan_message_includes_step_output() {
        let msg = replan_message(
            "fix auth",
            "patch-handler",
            "typecheck failed: expected String, got i32",
            &["investigate".into()],
            Some("Added handler returning session.user_id as i32"),
        );
        assert!(msg.contains("Step output before failure"));
        assert!(msg.contains("session.user_id as i32"));
    }

    #[test]
    fn user_message_includes_prior_art_for_new_feature() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        let msg = user_message("add JWT authentication", &ctx);

        assert!(msg.contains("Prior Art Research"));
        assert!(msg.contains("research-oss"));
        assert!(msg.contains("synthesize-research"));
    }

    #[test]
    fn user_message_no_prior_art_for_bug() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::Bug);
        let msg = user_message("fix null pointer in user handler", &ctx);

        assert!(!msg.contains("Prior Art Research"));
    }

    #[test]
    fn user_message_no_prior_art_for_trivial() {
        let ctx = PlanningContext::minimal(TaskComplexity::Trivial, TaskCategory::NewFeature);
        let msg = user_message("add logging", &ctx);

        assert!(!msg.contains("Prior Art Research"));
    }

    #[test]
    fn user_message_includes_paper_search_for_algorithm() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        let msg = user_message("implement a compression algorithm", &ctx);

        assert!(msg.contains("Prior Art Research"));
        assert!(msg.contains("research-papers"));
        assert!(msg.contains("research-oss"));
    }

    #[test]
    fn user_message_includes_registries_for_rust() {
        let mut ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        ctx.repo_profile.stack = "rust".into();
        let msg = user_message("add HTTP client", &ctx);

        assert!(msg.contains("crates.io"));
    }

    #[test]
    fn system_prompt_contains_prior_art_rule() {
        let ctx = PlanningContext::minimal(TaskComplexity::Medium, TaskCategory::NewFeature);
        let prompt = system_prompt(&ctx);

        assert!(prompt.contains("Prior Art Research"));
    }
}
