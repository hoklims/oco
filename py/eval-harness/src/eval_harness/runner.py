"""Runner that bridges Python eval scenarios to the Rust CLI."""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

from .scenario import EvalResult, EvalScenario


def scenarios_to_jsonl(scenarios: list[EvalScenario]) -> str:
    """Convert Python scenarios to JSONL format compatible with `oco eval`."""
    lines = []
    for s in scenarios:
        # Map to Rust ReplayScenario format.
        entry = {
            "name": s.id,
            "description": s.description,
            "user_request": s.user_request,
            "workspace": s.workspace_path,
            "expected_actions": [a.action_type for a in s.expected_actions],
            "tags": [s.category.value],
            "config_overrides": {
                "max_steps": s.max_steps,
                "max_duration_secs": s.timeout_secs,
            },
        }
        lines.append(json.dumps(entry))
    return "\n".join(lines)


def run_scenarios(
    scenarios: list[EvalScenario],
    *,
    provider: str = "stub",
    oco_bin: str = "oco",
) -> list[EvalResult]:
    """Run scenarios via the Rust `oco eval` CLI and return parsed results."""
    jsonl = scenarios_to_jsonl(scenarios)

    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".jsonl", delete=False
    ) as f:
        f.write(jsonl)
        scenario_path = f.name

    output_path = Path(tempfile.mktemp(suffix=".json"))

    try:
        result = subprocess.run(
            [
                oco_bin,
                "eval",
                scenario_path,
                "--provider",
                provider,
                "--output",
                str(output_path),
            ],
            capture_output=True,
            text=True,
            timeout=max(s.timeout_secs for s in scenarios) * len(scenarios) + 30,
        )

        if result.returncode != 0:
            return [
                EvalResult(
                    scenario_id=s.id,
                    passed=False,
                    actual_steps=0,
                    actual_actions=[],
                    outcome=f"CLI error: {result.stderr.strip()}",
                    duration_ms=0,
                    notes=[f"exit_code={result.returncode}"],
                )
                for s in scenarios
            ]

        if not output_path.exists():
            return [
                EvalResult(
                    scenario_id=s.id,
                    passed=False,
                    actual_steps=0,
                    actual_actions=[],
                    outcome="No output file generated",
                    duration_ms=0,
                )
                for s in scenarios
            ]

        metrics = json.loads(output_path.read_text())
        return _parse_metrics(scenarios, metrics)

    except subprocess.TimeoutExpired:
        return [
            EvalResult(
                scenario_id=s.id,
                passed=False,
                actual_steps=0,
                actual_actions=[],
                outcome="Timeout",
                duration_ms=0,
                notes=["timeout"],
            )
            for s in scenarios
        ]
    finally:
        Path(scenario_path).unlink(missing_ok=True)
        output_path.unlink(missing_ok=True)


def _parse_metrics(
    scenarios: list[EvalScenario],
    metrics: list[dict],
) -> list[EvalResult]:
    """Map Rust EvaluationMetrics back to Python EvalResult."""
    results = []
    metrics_by_name = {m["scenario_name"]: m for m in metrics}

    for s in scenarios:
        m = metrics_by_name.get(s.id)
        if m is None:
            results.append(
                EvalResult(
                    scenario_id=s.id,
                    passed=False,
                    actual_steps=0,
                    actual_actions=[],
                    outcome="Scenario not found in results",
                    duration_ms=0,
                )
            )
            continue

        results.append(
            EvalResult(
                scenario_id=s.id,
                passed=m.get("success", False),
                actual_steps=m.get("step_count", 0),
                actual_actions=[],  # Metrics don't include action list
                outcome="pass" if m.get("success") else "fail",
                duration_ms=m.get("duration_ms", 0),
                notes=[
                    f"tokens={m.get('total_tokens', 0)}",
                    f"tokens_per_step={m.get('token_per_step', 0):.0f}",
                    f"error_rate={m.get('error_rate', 0):.2f}",
                ],
            )
        )
    return results
