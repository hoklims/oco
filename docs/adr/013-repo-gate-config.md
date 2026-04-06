# ADR-013: Per-Repo Gate Configuration

## Status

Accepted

## Date

2026-04-04

## Context

### The gap between gate mechanics and repo-level contracts

ADR-012 introduced the evaluation gate model -- `GatePolicy`, `GateResult`, `EvalBaseline`, and the `oco eval-gate` CLI command -- giving OCO the ability to enforce quality thresholds and produce pass/warn/fail verdicts for CI pipelines. But every invocation of `oco eval-gate` requires the caller to supply three explicit arguments: a baseline path, a candidate path, and a policy name.

This works, but it creates friction:

1. **CI scripts are verbose.** Every pipeline must hard-code `oco eval-gate .oco/baseline.json eval-results.json --policy balanced`, even though most repos use the same baseline path and the same policy.
2. **The quality contract is implicit.** The choice of policy and baseline lives in CI scripts or Makefile targets, not in a reviewable configuration file. When a team changes their quality expectations, the change is buried in CI plumbing rather than visible in a config diff.
3. **Zero-argument invocation is impossible.** A developer running `oco eval-gate` locally must remember (or look up) the baseline path and policy. This discourages casual use during development.

Teams need to **declare their quality contract in the repository configuration** so that:

- `oco eval-gate` works with zero arguments (reads defaults from `oco.toml`).
- The baseline path and policy are versioned alongside the code they evaluate.
- CI scripts simplify to a single `oco eval-gate` call.
- Policy overrides (e.g., stricter min score for a critical service) are explicit and reviewable.

### Existing infrastructure

| Component | Location | Role |
|-----------|----------|------|
| `GateVerdict` | `shared-types/gate.rs` | Pass / Warn / Fail with exit codes (ADR-012) |
| `GatePolicy` | `shared-types/gate.rs` | Per-dimension thresholds + strategy + overall limits (ADR-012) |
| `GateResult` | `shared-types/gate.rs` | Full gate evaluation artifact (ADR-012) |
| `EvalBaseline` | `shared-types/gate.rs` | Saved scorecard snapshot with metadata (ADR-012) |
| `OrchestratorConfig` | `shared-types/config.rs` | Top-level config loaded from `oco.toml` |
| `oco eval-gate` | `dev-cli/` | CLI command that evaluates a candidate against a baseline |
| `config.schema.json` | `schemas/jsonschema/` | JSON Schema for `oco.toml` validation |

### What ADR-012 does not cover

| Gap | Impact |
|-----|--------|
| No config-level gate settings | Every `oco eval-gate` call requires explicit arguments |
| Baseline path not declarative | CI scripts hard-code the path; changes require script edits |
| Policy not in config | Policy choice is not reviewable in the repo's config file |
| No threshold overrides in config | Teams that want slightly different thresholds from a preset must build a custom `GatePolicy` programmatically |
| Zero-argument CLI not possible | Local developer experience requires remembering arguments |

## Decision

### Introduce `GateConfig` as a `[gate]` section in `oco.toml` with smallest useful surface, backward compatible, deterministic, and no LLM calls

A new struct `GateConfig` in `shared-types/gate.rs` provides four fields:

```rust
pub struct GateConfig {
    pub baseline_path: String,             // default: ".oco/baseline.json"
    pub default_policy: String,            // "strict" | "balanced" | "lenient", default: "balanced"
    pub min_overall_score: Option<f64>,    // override preset's min
    pub max_overall_regression: Option<f64>, // override preset's max regression
}
```

### Integration into OrchestratorConfig

`GateConfig` is added as an optional `[gate]` section in `OrchestratorConfig` with `#[serde(default)]`, meaning:

- Existing `oco.toml` files without a `[gate]` section continue to work unchanged.
- The defaults (`baseline_path = ".oco/baseline.json"`, `default_policy = "balanced"`, no overrides) apply automatically.

### CLI behavior with config

`oco eval-gate` resolves arguments with the following precedence:

1. **Explicit CLI arguments** take priority (full backward compatibility).
2. **`[gate]` config values** fill in any arguments not provided on the command line.
3. **Built-in defaults** apply when neither CLI nor config specifies a value.

This means:

| Invocation | Baseline | Policy | Overrides |
|------------|----------|--------|-----------|
| `oco eval-gate` | Config `baseline_path` | Config `default_policy` + overrides | Config |
| `oco eval-gate --candidate results.json` | Config `baseline_path` | Config `default_policy` + overrides | Config |
| `oco eval-gate base.json cand.json` | `base.json` (explicit) | Config `default_policy` + overrides | Config |
| `oco eval-gate base.json cand.json --policy strict` | `base.json` (explicit) | `strict` (explicit, no config overrides) | None |

When `min_overall_score` or `max_overall_regression` are set in config, they override the corresponding fields in the resolved `GatePolicy` preset. This allows a team to say "use the balanced preset, but with a stricter minimum score" without defining every threshold.

### Schema and examples

- `schemas/jsonschema/config.schema.json` gains a `gate` property with the four fields, proper types, enums, and constraints.
- `examples/oco.toml` gains a `[gate]` section with sensible defaults and commented-out overrides.
- `examples/ci-eval-gate.sh` gains a comment block showing the simplified Q7 invocations.

### Design principles

- **Smallest useful surface**: Four fields, all with defaults, all optional. No new concepts.
- **Backward compatible**: The `[gate]` section is `#[serde(default)]`. Existing configs and CLI invocations are unaffected.
- **Deterministic**: Config resolution is a pure precedence chain. No LLM calls, no heuristics.
- **Reviewable**: The quality contract lives in `oco.toml`, versioned in git, visible in PR diffs.

## Consequences

### Positive

- **Zero-argument gate**: Developers can run `oco eval-gate` locally without remembering paths or policy names. The repo's config provides the defaults.
- **CI simplification**: CI scripts can drop from `oco eval-gate .oco/baseline.json eval-results.json --policy balanced` to just `oco eval-gate --candidate eval-results.json` or even `oco eval-gate` when the candidate is also derived from config conventions.
- **Reviewable quality contracts**: When a team tightens or loosens their gate policy, the change appears as a diff in `oco.toml` -- visible in pull requests and auditable in git history.
- **Threshold overrides without custom code**: Teams can override `min_overall_score` or `max_overall_regression` in config without building a custom `GatePolicy`. This covers the most common customization need (adjusting the overall bar) with minimal surface area.
- **Backward compatible**: All existing CLI invocations continue to work exactly as before. The `[gate]` section is purely additive.

### Negative / Risks

- **Config drift**: A team might update their `oco.toml` policy but forget to update their baseline, leading to stale gate evaluations. Mitigation: the `EvalBaseline` includes `created_at` metadata, and `oco eval-gate` can warn when a baseline is older than a configurable threshold (future enhancement).
- **Override confusion**: When `min_overall_score` is set in config and a different preset is chosen via `--policy`, the interaction between the preset's defaults and the config override may surprise users. Mitigation: `oco eval-gate` logs the resolved policy (including any overrides) in its report output, making the effective thresholds transparent.
- **String-typed policy field**: `default_policy` is a `String` rather than an enum at the serde level, to keep the TOML surface simple. Invalid values are caught at policy resolution time with a clear error message. A future version could use a serde enum if stricter validation at parse time is needed.

### Neutral

- **No breaking changes**: All new fields have defaults. Existing `oco.toml` files and CLI invocations are unchanged.
- **Builds on ADR-012**: `GateConfig` does not replace or duplicate any ADR-012 types. It provides a declarative way to supply arguments that ADR-012's `GateResult::evaluate()` already accepts.
- **Schema updated**: The JSON Schema gains a `gate` property, keeping IDE autocompletion and validation current.

## Alternatives Considered

1. **Dedicated `gate.toml` file**: A separate config file for gate settings. Rejected: adds file proliferation without benefit. The `[gate]` section in `oco.toml` keeps all OCO configuration in one place.

2. **Full `GatePolicy` in config**: Expose all per-dimension thresholds in TOML. Rejected: too much surface area for the common case. The preset + two overrides cover 90% of needs. Full customization can be added later if demand materializes.

3. **Environment variables for defaults**: Use `OCO_GATE_BASELINE` and `OCO_GATE_POLICY` env vars. Rejected: env vars are not reviewable in PRs, not versioned with the code, and harder to document. Config-file defaults are the right layer for repo-level contracts.

4. **Auto-detect baseline from `.oco/` directory**: Scan for the most recent baseline file instead of requiring a path. Rejected: implicit behavior that could silently use the wrong baseline. An explicit (but defaulted) path is safer and more predictable.

## Related

- ADR-012: Evaluation Gate Model -- provides `GatePolicy`, `GateResult`, and `EvalBaseline` that this ADR configures declaratively.
- ADR-011: Evaluation Scorecard Model -- provides `RunScorecard` and `ScorecardComparison` that the gate evaluates.
- ADR-010: Mission Memory Model -- provides `MissionMemory` used by the MissionContinuity dimension.

## References

- `GateConfig`: `crates/shared-types/src/gate.rs`
- `GatePolicy`, `GateResult`, `EvalBaseline`: `crates/shared-types/src/gate.rs`
- `OrchestratorConfig`: `crates/shared-types/src/config.rs`
- JSON Schema: `schemas/jsonschema/config.schema.json`
- Example config: `examples/oco.toml`
- Example CI script: `examples/ci-eval-gate.sh`
