# OCO v2 — Evaluation Methodology

## Purpose

Enable comparative testing of OCO configurations to measure the impact of orchestration features.

## Scenario Format

JSONL file, one scenario per line:

```json
{
  "name": "scenario_name",
  "description": "What this tests",
  "user_request": "The prompt to execute",
  "workspace": "./path/to/workspace",
  "expected_actions": ["retrieve", "respond"],
  "config_overrides": {"llm_provider": "stub", "max_steps": 10},
  "tags": ["category"]
}
```

## Metrics Collected

- **success**: did the orchestration complete with TaskComplete?
- **step_count**: total steps taken
- **total_tokens**: LLM token consumption
- **duration_ms**: wall-clock time
- **verification_passed**: did verification pass?
- **token_per_step**: efficiency metric
- **error_rate**: errors / total steps
- **expected_match**: did actions match expected sequence?

## Comparison Modes

1. **OCO on/off** — compare with vs without orchestration
2. **Verify strict/relaxed** — compare verification enforcement levels
3. **Profile variations** — compare different repo profile settings
4. **Budget variations** — compare with different token/step limits

## CLI Usage

```bash
oco eval scenarios.jsonl --output results.json
oco eval scenarios.jsonl --provider stub
oco eval scenarios.jsonl --provider anthropic --output results.json
```

## Sample Scenarios

Located in `examples/eval-scenarios.jsonl` with 3 baseline scenarios.
