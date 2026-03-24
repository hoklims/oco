//! v2: Replay/evaluation scenario runner.
//!
//! Loads scenarios from JSONL, executes them against the orchestration loop,
//! and produces comparison-ready metrics.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use oco_shared_types::{
    EvaluationMetrics, OrchestratorAction, ReplayScenario, ScenarioConfig, ScenarioResult,
};
use tracing::info;

use crate::config::OrchestratorConfig;
use crate::llm::LlmProvider;
use crate::loop_runner::OrchestrationLoop;

/// Run a single scenario and return the result.
pub async fn run_scenario(
    scenario: &ReplayScenario,
    llm: Arc<dyn LlmProvider>,
    base_config: &OrchestratorConfig,
) -> Result<ScenarioResult> {
    let start = Instant::now();
    info!(scenario = %scenario.name, "Running evaluation scenario");

    // Apply config overrides.
    let mut config = base_config.clone();
    apply_overrides(&mut config, &scenario.config_overrides);

    let mut orchestrator = OrchestrationLoop::new(config, llm);

    let workspace = if scenario.workspace.is_empty() {
        None
    } else {
        Some(scenario.workspace.clone())
    };

    let state = orchestrator
        .run(scenario.user_request.clone(), workspace)
        .await?;

    let duration_ms = start.elapsed().as_millis() as u64;

    let actions: Vec<String> = state
        .action_history
        .iter()
        .map(|a| match a {
            OrchestratorAction::Respond { .. } => "respond".into(),
            OrchestratorAction::Retrieve { .. } => "retrieve".into(),
            OrchestratorAction::ToolCall { tool_name, .. } => {
                format!("tool_call:{tool_name}")
            }
            OrchestratorAction::Verify { strategy, .. } => {
                format!("verify:{strategy:?}")
            }
            OrchestratorAction::UpdateMemory { operation } => {
                format!("memory:{operation:?}")
            }
            OrchestratorAction::Stop { reason } => format!("stop:{reason:?}"),
        })
        .collect();

    let errors: Vec<String> = state
        .observations
        .iter()
        .filter_map(|o| {
            if let oco_shared_types::ObservationKind::Error { message, .. } = &o.kind {
                Some(message.clone())
            } else {
                None
            }
        })
        .collect();

    let verification_passed = state.verification.runs.last().map(|r| r.passed);

    // Check if expected action sequence was matched.
    let expected_match = if scenario.expected_actions.is_empty() {
        true
    } else {
        // Simple prefix match: expected actions should appear in order.
        let mut expected_iter = scenario.expected_actions.iter();
        let mut current_expected = expected_iter.next();
        for action in &actions {
            if let Some(expected) = current_expected
                && action.starts_with(expected.as_str())
            {
                current_expected = expected_iter.next();
            }
        }
        current_expected.is_none()
    };

    let task_complete = state.action_history.iter().any(|a| {
        matches!(
            a,
            OrchestratorAction::Stop {
                reason: oco_shared_types::StopReason::TaskComplete
            }
        )
    });

    // Check that at least one non-empty Respond was generated.
    let response_generated = state.action_history.iter().any(|a| {
        matches!(a, OrchestratorAction::Respond { content } if !content.trim().is_empty())
    });

    // Success requires both task completion AND a meaningful response.
    let success = task_complete && response_generated;

    Ok(ScenarioResult {
        scenario_name: scenario.name.clone(),
        success,
        step_count: state.session.step_count,
        total_tokens: state.session.budget.tokens_used,
        duration_ms,
        verification_passed,
        actions,
        errors,
        expected_match,
        response_generated,
    })
}

/// Load scenarios from a JSONL file.
pub fn load_scenarios(path: &Path) -> Result<Vec<ReplayScenario>> {
    let content = std::fs::read_to_string(path)?;
    let mut scenarios = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let scenario: ReplayScenario = serde_json::from_str(line)?;
        scenarios.push(scenario);
    }
    Ok(scenarios)
}

/// Run all scenarios and produce metrics.
pub async fn run_all(
    scenarios: &[ReplayScenario],
    llm: Arc<dyn LlmProvider>,
    config: &OrchestratorConfig,
) -> Vec<ScenarioResult> {
    let mut results = Vec::new();
    for scenario in scenarios {
        match run_scenario(scenario, llm.clone(), config).await {
            Ok(result) => {
                info!(
                    scenario = %result.scenario_name,
                    success = result.success,
                    steps = result.step_count,
                    tokens = result.total_tokens,
                    "Scenario completed"
                );
                results.push(result);
            }
            Err(e) => {
                tracing::error!(
                    scenario = %scenario.name,
                    error = %e,
                    "Scenario failed"
                );
                results.push(ScenarioResult {
                    scenario_name: scenario.name.clone(),
                    success: false,
                    step_count: 0,
                    total_tokens: 0,
                    duration_ms: 0,
                    verification_passed: None,
                    actions: vec![],
                    errors: vec![e.to_string()],
                    expected_match: false,
                    response_generated: false,
                });
            }
        }
    }
    results
}

/// Aggregate results into comparison metrics.
pub fn aggregate_metrics(results: &[ScenarioResult]) -> Vec<EvaluationMetrics> {
    results.iter().map(|r| r.metrics()).collect()
}

fn apply_overrides(config: &mut OrchestratorConfig, overrides: &ScenarioConfig) {
    if let Some(ref provider) = overrides.llm_provider {
        config.llm.provider = provider.clone();
    }
    if let Some(max_steps) = overrides.max_steps {
        config.default_budget.max_tool_calls = max_steps;
    }
    if let Some(max_tokens) = overrides.max_total_tokens {
        config.default_budget.max_total_tokens = max_tokens;
    }
    if let Some(max_retrievals) = overrides.max_retrievals {
        config.default_budget.max_retrievals = max_retrievals;
    }
    if let Some(max_duration_secs) = overrides.max_duration_secs {
        config.default_budget.max_duration_secs = max_duration_secs;
    }
    if let Some(max_verify_cycles) = overrides.max_verify_cycles {
        config.default_budget.max_verify_cycles = max_verify_cycles;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_scenarios_from_jsonl() {
        let dir = std::env::temp_dir().join("oco_eval_test");
        let _ = std::fs::create_dir_all(&dir);
        let scenario_file = dir.join("test.jsonl");
        std::fs::write(
            &scenario_file,
            r#"{"name":"test1","description":"desc","user_request":"fix bug","workspace":".","expected_actions":[],"tags":["basic"]}"#,
        )
        .unwrap();

        let scenarios = load_scenarios(&scenario_file).unwrap();
        assert_eq!(scenarios.len(), 1);
        assert_eq!(scenarios[0].name, "test1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scenario_result_to_metrics() {
        let result = ScenarioResult {
            scenario_name: "test".into(),
            success: true,
            step_count: 4,
            total_tokens: 8000,
            duration_ms: 2000,
            verification_passed: Some(true),
            actions: vec!["retrieve".into(), "respond".into()],
            errors: vec![],
            expected_match: true,
            response_generated: true,
        };
        let metrics = result.metrics();
        assert_eq!(metrics.token_per_step, 2000.0);
        assert!(metrics.success);
    }
}
