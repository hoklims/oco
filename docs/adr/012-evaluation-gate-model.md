# ADR-012: Evaluation Gate Model

## Status

Accepted

## Date

2026-04-04

## Context

### The gap between comparison and decision

ADR-011 introduced `RunScorecard` and `ScorecardComparison`, giving OCO the ability to measure run quality across 7 dimensions and detect regressions between two scorecards. But comparison alone does not answer the operational question: **should this candidate pass or fail a quality gate?**

`ScorecardComparison` reports *what changed*. It does not encode *what is acceptable*. A regression of -0.05 on CostEfficiency might be fine; the same delta on Success is catastrophic. Without an explicit policy that maps comparison results to a pass/warn/fail verdict, every regression report requires human judgment -- which defeats the purpose of automated CI gating.

Teams need:

1. **Preset policies** with sensible defaults so that `oco eval gate` works out of the box.
2. **Per-dimension thresholds** so that critical dimensions (Success, TrustVerdict) are held to stricter standards than secondary ones (CostEfficiency, ReplanStability).
3. **A clear verdict** with exit codes that CI pipelines can branch on without parsing output.
4. **A reviewable artifact** that shows exactly which thresholds were checked, which passed, and which failed -- so that a human reviewer can audit the gate decision.
5. **Baselines as files** that can be committed to the repository and versioned alongside the code they evaluate.

### Existing infrastructure

| Component | Location | Role |
|-----------|----------|------|
| `RunScorecard` | `shared-types/scorecard.rs` | Per-run 7-dimension quality score (ADR-011) |
| `ScorecardComparison` | `shared-types/scorecard.rs` | Pairwise comparison with regression flags (ADR-011) |
| `BatchComparison` | `shared-types/scorecard.rs` | Suite-level aggregate comparison (ADR-011) |
| `RegressionSeverity` | `shared-types/scorecard.rs` | Critical / Warning / Minor severity levels |
| `MissionMemory` | `shared-types/mission.rs` | Durable inter-session handoff artifact (ADR-010) |
| `VerificationState` | `shared-types/verification.rs` | Modified files, verification runs, freshness (ADR-004) |
| Run artifacts | `.oco/runs/<id>/` | `trace.jsonl` + `summary.json` + `mission.json` + `scorecard.json` |

### What ScorecardComparison does not cover

| Gap | Impact |
|-----|--------|
| Pass/warn/fail verdict | Comparison reports deltas but does not declare a verdict |
| Per-dimension minimum scores | A candidate can have zero verification coverage and still show "stable" if the baseline was also zero |
| Policy presets | Every CI integration must invent its own threshold logic |
| Exit code mapping | No standard exit code convention for gate outcomes |
| Baseline persistence | No defined format for saving and loading baseline scorecards |
| Asymmetric dimension importance | Success regression should block; CostEfficiency regression should warn |

## Decision

### Introduce a policy-driven evaluation gate with per-dimension thresholds, preset strategies, and a reviewable result artifact

A new module `gate.rs` in `shared-types` provides seven core types:

- **`GateVerdict`** -- the outcome enum: Pass, Warn, or Fail.
- **`GateThreshold`** -- per-dimension minimum score and maximum allowed regression delta.
- **`GateStrategy`** -- how to combine per-dimension results: Strict, Balanced, or Lenient.
- **`GatePolicy`** -- a complete policy: thresholds + strategy + overall minimum score + overall max regression.
- **`DimensionGateCheck`** -- the result of checking a single dimension against its threshold.
- **`GateResult`** -- the full gate evaluation artifact: per-dimension checks, overall scores, comparison, verdict, and reasons.
- **`EvalBaseline`** -- a saved baseline: a scorecard snapshot with metadata for identification and versioning.

### GateVerdict

```rust
pub enum GateVerdict {
    Pass,  // exit code 0 — all thresholds met
    Warn,  // exit code 1 — minor regressions, review recommended
    Fail,  // exit code 2 — critical regression or threshold violation, blocks merge
}
```

`GateVerdict::is_blocking()` returns true only for `Fail`. This allows CI pipelines to distinguish between "proceed with caution" (Warn) and "stop" (Fail) using the exit code alone.

### GatePolicy and presets

A `GatePolicy` combines per-dimension `GateThreshold` values, a `GateStrategy`, a minimum overall composite score, and a maximum overall regression delta.

Three presets cover the most common needs:

| Preset | Strategy | Overall min | Overall max regression | Critical dim min | Non-critical dim min |
|--------|----------|-------------|----------------------|------------------|---------------------|
| **Strict** | Any failure blocks | 0.6 | -0.1 | Success: 1.0, TrustVerdict: 0.6 | 0.2--0.5 |
| **Balanced** | Only critical dims block; others warn | 0.4 | -0.15 | Success: 0.5, TrustVerdict: 0.3 | 0.0--0.3 |
| **Lenient** | Only critical-severity regressions block | 0.0 | -0.5 | All: 0.0 | All: 0.0 |

The presets are constructed via `GatePolicy::strict()`, `GatePolicy::default_balanced()`, and `GatePolicy::lenient()`.

### GateStrategy behavior

The strategy determines how per-dimension failures are escalated to the final verdict:

- **Strict**: Any dimension below its `min_score` or exceeding its `max_regression` triggers `Fail`.
- **Balanced**: Only "critical" dimensions (Success, TrustVerdict) trigger `Fail` on threshold violation. Non-critical dimension violations produce `Warn`.
- **Lenient**: Threshold violations produce `Warn` unless the underlying `ScorecardComparison` flags the regression as `RegressionSeverity::Critical`, in which case the dimension result is `Fail`.

### GateResult::evaluate

`GateResult::evaluate(baseline, candidate, policy)` is the core evaluation function:

1. Compute a `ScorecardComparison` between baseline and candidate (reusing the ADR-011 comparison logic).
2. For each of the 7 dimensions:
   - Look up the `GateThreshold` from the policy.
   - Check if the candidate score is below `min_score`.
   - Check if the delta exceeds `max_regression`.
   - Apply the strategy to determine the dimension verdict (Pass/Warn/Fail).
   - Record a `DimensionGateCheck` with all inputs and the verdict.
3. Check overall composite score against `policy.min_overall_score`.
4. Check overall delta against `policy.max_overall_regression`.
5. Combine: if any check produced `Fail`, the final verdict is `Fail`. If any produced `Warn`, the final verdict is `Warn`. Otherwise, `Pass`.

The result includes:
- All dimension checks with their individual verdicts and reasons.
- Overall scores and delta.
- The full `ScorecardComparison` for detailed drill-down.
- Human-readable reasons that drove the verdict.
- The policy that was applied (for auditability).

### EvalBaseline

An `EvalBaseline` wraps a `RunScorecard` with metadata:

```rust
pub struct EvalBaseline {
    pub name: String,                          // e.g., "v0.5-stable"
    pub created_at: DateTime<Utc>,
    pub scorecard: RunScorecard,
    pub description: Option<String>,
    pub source: String,                        // e.g., "oco eval --output ..."
}
```

Baselines are persisted as JSON files (via `save_to()` / `load_from()`) and can be committed to the repository. The `oco eval gate` command loads a baseline file and evaluates a candidate scorecard against it.

### Report generation

`GateResult::to_report()` produces a human-readable gate report:

```
Eval Gate: baseline-v0.5 vs candidate-v0.6
  Policy: Balanced | min_overall: 0.40 | max_regression: -0.15
  Overall: 0.81 -> 0.74 (delta: -0.07)

  Dimension                Baseline  Candidate  Delta   Min    Verdict
  -----------------------------------------------------------------------
  success                     0.90      0.80   -0.10   0.50  [PASS]
  trust_verdict               0.80      0.70   -0.10   0.30  [PASS]
  verification_coverage       0.75      0.40   -0.35   0.30  [WARN]
  mission_continuity          0.70      0.70   +0.00   0.00  [PASS]
  cost_efficiency             0.60      0.55   -0.05   0.00  [PASS]
  replan_stability            0.85      0.80   -0.05   0.00  [PASS]
  error_rate                  0.90      0.85   -0.05   0.30  [PASS]

  Reasons (1):
    - verification_coverage: regression -0.35 exceeds limit -0.30

  Verdict: [WARN] warn
  Exit code: 1
```

### CI integration

The CLI surface `oco eval gate <baseline.json> <candidate-results.json> --policy <name>`:

1. Loads the baseline from a JSON file.
2. Loads or computes the candidate scorecard.
3. Applies the named policy (strict/balanced/lenient).
4. Prints the gate report (human mode) or emits a JSON `GateResult` (JSONL mode).
5. Exits with the verdict's exit code: 0 (pass), 1 (warn), 2 (fail).

CI pipelines can gate on the exit code:

```yaml
- run: oco eval gate .oco/baseline.json eval-results.json --policy balanced
  # Exit 0 = pass, 1 = warn (optional block), 2 = fail (block merge)
```

## Consequences

### Positive

- **CI-ready out of the box**: Three preset policies cover the common needs. Teams can run `oco eval gate` with `--policy balanced` immediately and get meaningful pass/warn/fail decisions without writing custom threshold logic.
- **Reviewable artifacts**: `GateResult` is a structured JSON document that records every threshold check, every dimension verdict, and every reason. PR reviewers can inspect exactly why a gate passed or failed.
- **Deterministic**: Gate evaluation is a pure function of two scorecards and a policy. No LLM calls, no network dependencies, no randomness. The same inputs always produce the same verdict.
- **Asymmetric dimension importance**: The Balanced strategy correctly distinguishes between a Success regression (blocks) and a CostEfficiency regression (warns). This matches how teams actually triage regressions.
- **Exit code convention**: The 0/1/2 mapping is simple, unambiguous, and compatible with standard CI conditional logic.
- **Baseline versioning**: `EvalBaseline` files can be committed alongside the code, making quality expectations explicit and reviewable in pull requests.

### Negative / Risks

- **Threshold tuning**: The preset thresholds are a starting point. Teams with unusual quality profiles (e.g., high tolerance for replanning in complex codebases) will need to adjust thresholds. Mitigation: `GatePolicy` is fully serializable; a future `oco.toml` section can override presets.
- **Baseline staleness**: A baseline that is never updated becomes irrelevant as the system evolves. Mitigation: baselines include `created_at` metadata and the `source` field documents their provenance. Teams should update baselines as part of release cycles.
- **Three-way ambiguity**: The Warn verdict (exit code 1) is inherently ambiguous -- some CI pipelines may want to block on it, others may not. Mitigation: teams choose the policy that matches their risk tolerance. Strict policies produce fewer Warns (most violations are Fails).

### Neutral

- **No breaking changes**: All new types are additive. `RunScorecard`, `ScorecardComparison`, and all existing types are unchanged.
- **Builds on ADR-011**: The gate model composes with the scorecard model -- it does not duplicate or replace it. `GateResult` embeds a `ScorecardComparison` for full drill-down.
- **Serialization format**: JSON, consistent with all existing run artifacts and scorecard files.
- **No runtime overhead**: Gate evaluation happens offline (in the CLI or CI). It does not affect orchestration performance.

## Alternatives Considered

1. **Threshold-only (no strategy presets)**: Require users to specify all thresholds manually. Rejected: too much friction for adoption. Most teams want a reasonable default that works immediately.

2. **Binary pass/fail (no Warn)**: Simplify to two outcomes. Rejected: loses the "review recommended" signal that is valuable in PR workflows. Many regressions are minor and should not block merges but should be visible.

3. **LLM-based gate evaluation**: Use an LLM to assess whether a regression is acceptable given the context. Rejected: violates the deterministic policy principle (same as ADR-011). Gates must be reproducible and auditable.

4. **Percentage-based thresholds**: Express thresholds as "no more than 10% regression" instead of absolute deltas. Rejected: percentage-based thresholds behave poorly at extremes (10% of 0.1 is 0.01, which is noise). Absolute deltas are simpler and more predictable.

5. **Embed policy in oco.toml only**: No preset policies; all configuration via the config file. Rejected: violates the "works out of the box" goal. Presets are the 80% solution; config-file overrides are the 20% extension.

## Related

- ADR-011: Evaluation Scorecard Model -- provides `RunScorecard` and `ScorecardComparison` that the gate evaluates.
- ADR-010: Mission Memory Model -- provides `MissionMemory` used by the MissionContinuity dimension.
- ADR-004: Verification Integrity Model -- provides `VerificationState` and the trust signals that underpin TrustVerdict and VerificationCoverage scoring.

## References

- `GateVerdict`, `GatePolicy`, `GateStrategy`, `GateThreshold`, `GateResult`, `DimensionGateCheck`, `EvalBaseline`: `crates/shared-types/src/gate.rs`
- `RunScorecard`, `ScorecardComparison`, `ScorecardDimension`, `RegressionSeverity`: `crates/shared-types/src/scorecard.rs`
- `VerificationState`: `crates/shared-types/src/verification.rs`
- `MissionMemory`: `crates/shared-types/src/mission.rs`
- Example baseline: `examples/baseline-v0.5.json`
- Example CI script: `examples/ci-eval-gate.sh`
