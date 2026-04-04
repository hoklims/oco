# ADR-014: Baseline Lifecycle & Review Artifacts

## Status

Accepted

## Date

2026-04-04

## Context

### The gap between "the gate works" and "the gate is trustworthy and reviewable"

ADR-012 introduced the evaluation gate model and ADR-013 added per-repo configuration, giving OCO a fully functional quality gate with preset policies, CI exit codes, and zero-argument invocation. But two gaps remain between a working gate and a gate that teams can trust and audit over time:

1. **Baseline staleness.** A baseline created two months ago may not reflect current quality expectations. The codebase has evolved, the scenario suite has changed, and the scoring rules may have been tuned -- yet the gate silently evaluates against an outdated reference. `EvalBaseline` includes a `created_at` timestamp, but nothing in the system inspects it or warns when a baseline is aging or stale.

2. **No reviewable artifact.** The gate produces terminal output and an exit code, which are sufficient for CI pass/fail decisions but inadequate for PR review, archival, or audit. When a gate warns or fails, a reviewer must re-run the command locally to understand why. There is no structured document that captures the gate decision, its inputs, and its rationale in a form suitable for attachment to a pull request or storage in a CI artifact archive.

Teams need to know two things at gate time: (1) does the candidate pass? (2) is the baseline still credible? And they need the answer in a format that survives beyond the terminal session.

### Existing infrastructure

| Component | Location | Role |
|-----------|----------|------|
| `GateVerdict` | `shared-types/gate.rs` | Pass / Warn / Fail with exit codes (ADR-012) |
| `GatePolicy` | `shared-types/gate.rs` | Per-dimension thresholds + strategy + overall limits (ADR-012) |
| `GateResult` | `shared-types/gate.rs` | Full gate evaluation artifact (ADR-012) |
| `EvalBaseline` | `shared-types/gate.rs` | Saved scorecard snapshot with `created_at` metadata (ADR-012) |
| `GateConfig` | `shared-types/gate.rs` | Per-repo gate configuration with `baseline_path` and `default_policy` (ADR-013) |
| `oco eval-gate` | `dev-cli/` | CLI command that evaluates a candidate against a baseline |
| `config.schema.json` | `schemas/jsonschema/` | JSON Schema for `oco.toml` validation |

### What ADR-012 and ADR-013 do not cover

| Gap | Impact |
|-----|--------|
| No baseline freshness check | A stale baseline silently produces misleading gate results |
| No freshness classification | No way to distinguish "recently created" from "months old" |
| No configurable freshness thresholds | Teams with different release cadences cannot tune what "stale" means |
| No structured review document | Gate results exist only as terminal output and exit codes |
| No Markdown artifact for PRs | Reviewers cannot inspect gate decisions without re-running the command |
| No JSON artifact for automation | Downstream tools cannot consume gate results programmatically from CI artifacts |

## Decision

### Introduce baseline freshness detection and structured review artifacts, extending the gate model without breaking existing behavior

Five additions to `shared-types/gate.rs`:

#### 1. `BaselineFreshness` enum

```rust
pub enum BaselineFreshness {
    Fresh,    // within fresh_days threshold
    Aging,    // between fresh_days and stale_days
    Stale,    // beyond stale_days threshold
    Unknown,  // no created_at timestamp available
}
```

A simple classification that maps baseline age to one of four states. `Unknown` handles legacy baselines (raw `RunScorecard` files without `created_at` metadata) and future-dated timestamps. `BaselineFreshnessCheck::unknown()` produces an `Unknown` result with a recommendation to use `oco baseline-save`. The `--report` flag and terminal freshness display both support `Unknown` end-to-end, producing valid artifacts without requiring an `EvalBaseline`.

#### 2. `BaselineFreshnessCheck`

```rust
pub struct BaselineFreshnessCheck {
    pub freshness: BaselineFreshness,
    pub baseline_age_days: Option<u64>,
    pub fresh_threshold_days: u32,   // default: 14
    pub stale_threshold_days: u32,   // default: 30
}
```

Evaluates baseline freshness from `EvalBaseline::created_at` against configurable thresholds:

- Age <= `fresh_threshold_days`: **Fresh**
- Age <= `stale_threshold_days`: **Aging**
- Age > `stale_threshold_days`: **Stale**
- No `created_at`: **Unknown**

The defaults (14 days fresh, 30 days stale) match a typical two-week sprint cadence. Teams with longer release cycles can increase these via config.

#### 3. `GateReviewArtifact`

```rust
pub struct GateReviewArtifact {
    pub gate_result: GateResult,
    pub freshness_check: BaselineFreshnessCheck,
    pub summary: String,
    pub recommendations: Vec<String>,
    pub generated_at: DateTime<Utc>,
}
```

A structured review document that combines the gate result with freshness assessment, a human-readable summary, and actionable recommendations. Two constructors: `generate()` takes an `EvalBaseline` reference, `generate_with_name()` takes just a baseline name string — the latter supports raw `RunScorecard` baselines with `Unknown` freshness. It produces two output formats:

- **Markdown** (`to_markdown()`): A formatted document suitable for PR comments, CI artifact archives, or team review. Includes a verdict header, dimension table, freshness status, and recommendation list.
- **JSON** (`to_json()`): A machine-readable representation for downstream tooling, dashboards, or automated PR annotation bots.

#### 4. `GateConfig` extension

Two new optional fields on `GateConfig`:

```rust
pub fresh_days: Option<u32>,   // overrides the 14-day default
pub stale_days: Option<u32>,   // overrides the 30-day default
```

These map to the `[gate]` section in `oco.toml`:

```toml
[gate]
fresh_days = 14
stale_days = 30
```

When absent, the built-in defaults (14 and 30) apply. Validation ensures `fresh_days <= stale_days` when both are set. When `fresh_days == stale_days`, the Aging zone is empty — baselines transition directly from Fresh to Stale, which is a valid configuration for teams that want a binary freshness signal.

#### 5. CLI: `--report <dir>` flag

The `oco eval-gate` command gains a `--report <dir>` option:

```bash
oco eval-gate --report ./gate-artifacts
```

This writes two files to the specified directory:
- `gate-report.md` -- the Markdown review artifact.
- `gate-report.json` -- the JSON review artifact.

The flag is optional and does not affect the existing exit code behavior. When combined with `--json`, the JSON artifact is also printed to stdout as before.

### Design principles

- **Backward compatible**: All new fields are optional with defaults. Existing configs, CLI invocations, and baselines work unchanged.
- **Deterministic**: Freshness classification is a pure function of `created_at` and the current time. No LLM calls, no heuristics.
- **Smallest useful surface**: Two config fields, one enum, one check struct, one artifact struct. No new concepts -- just inspection of data already present in `EvalBaseline`.
- **Reviewable by default**: The Markdown artifact is designed for direct inclusion in PR comments or CI summaries without post-processing.

## Consequences

### Positive

- **Trustworthy baselines**: Teams are warned when evaluating against an aging or stale baseline. A stale baseline does not silently produce a false sense of quality -- the freshness check makes the risk visible.
- **Reviewable artifacts**: Gate decisions are captured in structured documents (Markdown + JSON) that can be attached to pull requests, archived in CI, or consumed by downstream tooling. Reviewers no longer need to re-run the gate command to understand the verdict.
- **CI archival**: The `--report` flag integrates naturally with CI artifact storage (e.g., GitHub Actions upload-artifact, GitLab CI artifacts). Teams can archive gate reports alongside build logs for audit purposes.
- **Configurable cadence**: Teams with different release rhythms (weekly vs. monthly vs. quarterly) can tune `fresh_days` and `stale_days` to match their update cadence, avoiding false staleness warnings.
- **Actionable recommendations**: The review artifact includes specific recommendations (e.g., "Baseline is stale -- consider running `oco baseline-save` to update it") that guide teams toward corrective action.

### Negative / Risks

- **Threshold tuning**: The default freshness thresholds (14/30 days) are a reasonable starting point but may not suit every team's cadence. Teams that rarely update baselines will see persistent "stale" warnings. Mitigation: thresholds are configurable in `oco.toml`.
- **Artifact storage**: Writing Markdown and JSON files to disk adds I/O and storage considerations. In CI pipelines with many gate evaluations, artifact directories can accumulate. Mitigation: artifact generation is opt-in via `--report` and teams control their CI artifact retention policies.
- **Clock dependency**: Freshness classification depends on the system clock. In environments with incorrect clocks (rare but possible in containers), the freshness assessment may be inaccurate. Mitigation: the `Unknown` variant handles missing timestamps gracefully, and the check reports the computed age for transparency.

### Neutral

- **No breaking changes**: All new types and fields are additive. Existing `GateConfig`, `GateResult`, and `EvalBaseline` are unchanged in their existing API surface.
- **Builds on Q5-Q7**: The freshness check inspects `EvalBaseline::created_at` (ADR-012). The review artifact wraps `GateResult` (ADR-012) and respects `GateConfig` thresholds (ADR-013). No duplication.
- **Serialization format**: JSON for the machine artifact, Markdown for the human artifact. Both are standard formats with no new dependencies.

## Alternatives Considered

1. **Automatic baseline refresh**: Automatically re-run the eval suite when the baseline is stale. Rejected: running evaluations is expensive and should be an explicit decision. Warning about staleness is the right default; automatic refresh can be built on top as a CI workflow step.

2. **Freshness as a gate dimension**: Add baseline freshness as an 8th scoring dimension in `RunScorecard`. Rejected: freshness is a property of the baseline, not the candidate run. Mixing the two would conflate "how good was this run?" with "how current is the reference?" The freshness check is intentionally separate from the scorecard.

3. **Embed review artifact in GateResult**: Extend `GateResult` to include freshness and recommendations directly. Rejected: `GateResult` is the evaluation output; the review artifact is a presentation concern that combines the evaluation with additional context. Keeping them separate maintains clean layering.

4. **HTML artifact instead of Markdown**: Produce an HTML document with styling. Rejected: Markdown is more portable, renders natively in GitHub/GitLab PR comments, and can be converted to HTML if needed. HTML adds complexity without clear benefit for the primary use case (PR review).

5. **Warn-only on stale baselines (no config)**: Hard-code the freshness thresholds without exposing config. Rejected: teams have vastly different release cadences. A threshold that is reasonable for a weekly-release team would produce constant warnings for a quarterly-release team.

## Related

- ADR-011: Evaluation Scorecard Model -- provides `RunScorecard` that baselines wrap and the gate evaluates.
- ADR-012: Evaluation Gate Model -- provides `GateVerdict`, `GatePolicy`, `GateResult`, and `EvalBaseline` that this ADR extends with freshness and review artifacts.
- ADR-013: Per-Repo Gate Configuration -- provides `GateConfig` that this ADR extends with `fresh_days` and `stale_days`.

## References

- `BaselineFreshness`, `BaselineFreshnessCheck`, `GateReviewArtifact`: `crates/shared-types/src/gate.rs`
- `GateVerdict`, `GatePolicy`, `GateResult`, `EvalBaseline`, `GateConfig`: `crates/shared-types/src/gate.rs`
- `RunScorecard`, `ScorecardComparison`: `crates/shared-types/src/scorecard.rs`
- JSON Schema: `schemas/jsonschema/config.schema.json`
- Example config: `examples/oco.toml`
- Example CI script: `examples/ci-eval-gate.sh`
