# ADR-011: Evaluation Scorecard Model

## Status

Accepted

## Date

2026-04-04

## Context

### The gap between "run succeeded" and "run was good"

OCO can determine whether a run completed (the orchestrator reached a terminal state) and whether verification passed (ADR-004). But these two signals are insufficient to answer the question that matters for continuous improvement: **was this run good?**

A run that succeeds but burns 90% of the token budget on a trivial task is wasteful. A run that passes verification but produces no mission memory (ADR-010) breaks session continuity. A run that replans four times before converging is fragile. A run that modifies ten files but only verifies three has a coverage gap.

Without a structured, multi-dimensional evaluation model, OCO cannot:

1. **Detect regressions** between versions or provider changes. A new model might succeed on the same scenarios but produce lower-quality outcomes along dimensions that a simple pass/fail check ignores.
2. **Compare candidates** in A/B evaluations. When testing a new planner strategy or LLM router configuration, operators need more than "both passed" -- they need to know which performed better and on which axes.
3. **Track trends** over time. Individual runs are ephemeral; scorecards aggregated across scenario suites reveal systemic improvements or degradations.
4. **Gate CI pipelines**. Automated quality gates require numeric thresholds on defined dimensions, not just exit codes.

### Existing infrastructure

| Component | Location | Role |
|-----------|----------|------|
| `RunSummary` | `orchestrator-core/eval.rs` | Pass/fail outcome with timing and step count |
| `MissionMemory` | `shared-types/mission.rs` | Durable inter-session handoff artifact (ADR-010) |
| `VerificationState` | `shared-types/verification.rs` | Modified files, verification runs, freshness |
| `TrustVerdict` | `shared-types/telemetry.rs` | High / Medium / Low / None trust assessment |
| `CancellationToken` | `orchestrator-core/graph_runner.rs` | Budget enforcement on parallel steps |
| `ReplayScenario` | `orchestrator-core/eval.rs` | Eval scenario definition (JSONL) |
| Run artifacts | `.oco/runs/<id>/` | `trace.jsonl` + `summary.json` + `mission.json` |

### What RunSummary does not cover

| Gap | Impact |
|-----|--------|
| Multi-dimensional quality | Pass/fail is a single bit; no granularity on how well the run performed |
| Cost awareness | Token and step counts are recorded but not scored against budget |
| Verification coverage ratio | `VerificationState` tracks freshness but not the coverage ratio as a score |
| Mission memory quality | Presence of `mission.json` is not evaluated for substantive content |
| Replan stability | Number of replans is logged but not assessed as a quality signal |
| Cross-run comparison | No mechanism to compare two runs on the same scenario |
| Batch regression detection | No aggregate view across a scenario suite |

## Decision

### Introduce a 7-dimension scorecard model with weighted composite scoring and regression detection

A new module `scorecard.rs` in `shared-types` provides four core types:

- **`RunScorecard`** -- per-run evaluation across 7 dimensions with a weighted composite score.
- **`ScorecardComparison`** -- pairwise comparison between a baseline and a candidate scorecard, with per-dimension delta analysis and regression flagging.
- **`BatchComparison`** -- aggregate comparison across a scenario suite, producing counts of improved/stable/regressed scenarios and an overall verdict.
- **`ComparisonVerdict`** -- the final assessment: Improved, Stable, or Regressed.

### RunScorecard structure

```rust
pub struct RunScorecard {
    pub run_id: String,
    pub computed_at: DateTime<Utc>,
    pub dimensions: Vec<DimensionScore>,
    pub overall_score: f64,       // weighted average, 0.0-1.0
    pub cost: CostMetrics,
}
```

The `overall_score` is computed as a weighted average of all dimension scores. Empty dimension lists produce a score of 0.0.

`CostMetrics` captures the raw resource consumption: steps, tokens, duration, tool calls, verify cycles, and replans. These are not normalized -- they serve as inputs to dimension scoring and as raw data for comparison reports.

## Dimensions

| Dimension | Weight | Score Range | Source | Scoring Rule |
|-----------|--------|-------------|--------|--------------|
| **Success** | 3.0 | 0.0 or 1.0 | Orchestrator terminal state | 1.0 if the run completed successfully; 0.0 otherwise. Binary -- no partial credit. |
| **TrustVerdict** | 2.0 | 0.0 -- 1.0 | `TrustVerdict` from telemetry | High = 1.0, Medium = 0.66, Low = 0.33, None = 0.0. Maps directly from the existing trust assessment. |
| **VerificationCoverage** | 1.5 | 0.0 -- 1.0 | `VerificationState` | Ratio of verified files to modified files. If no files were modified, score is 1.0 (vacuously satisfied). |
| **MissionContinuity** | 1.0 | 0.0 -- 1.0 | `MissionMemory` artifact | 1.0 if a mission memory was produced with substantive content (non-empty facts or hypotheses); 0.0 if absent or empty. Intermediate values possible based on content richness. |
| **CostEfficiency** | 1.0 | 0.0 -- 1.0 | `CostMetrics` vs budget | Measures how efficiently the run used its budget. Higher score = less waste. Computed as `1.0 - (tokens_used / token_budget)`, clamped to [0.0, 1.0]. A run that uses 30% of budget scores 0.7. |
| **ReplanStability** | 0.5 | 0.0 -- 1.0 | Replan count from trace | Penalizes excessive replanning. 0 replans = 1.0, 1 replan = 0.66, 2 replans = 0.33, 3+ replans = 0.0. Stability signals plan quality. |
| **ErrorRate** | 1.0 | 0.0 -- 1.0 | Step outcomes from trace | Ratio of error-free steps to total steps. A run with 10 steps and 2 errors scores 0.8. |

**Total weight**: 10.0. The weights reflect priority: Success and TrustVerdict dominate because a run that fails or produces untrustworthy results is fundamentally flawed regardless of efficiency.

### Weight rationale

- **Success (3.0)**: The most important dimension. A failed run scores 0.0 on the most heavily weighted axis, ensuring low composite scores regardless of other dimensions.
- **TrustVerdict (2.0)**: Verification trust is the core OCO guarantee. An untrustworthy run is worse than an expensive one.
- **VerificationCoverage (1.5)**: Coverage gaps are a risk signal but less severe than outright trust failure.
- **MissionContinuity (1.0)**: Important for multi-session workflows (ADR-010) but not all tasks need it.
- **CostEfficiency (1.0)**: Waste matters but should not override correctness.
- **ErrorRate (1.0)**: Errors in steps indicate fragility; parity with cost efficiency.
- **ReplanStability (0.5)**: Lowest weight because replanning is sometimes the correct response to genuine complexity. Penalizing it too heavily discourages adaptive behavior.

## Comparison Model

### Pairwise comparison (`ScorecardComparison`)

`ScorecardComparison::compare(baseline, candidate)` produces a detailed comparison:

1. **Per-dimension delta**: For each of the 7 dimensions, compute `candidate_score - baseline_score`. Positive delta = improvement, negative delta = regression.

2. **Threshold classification**:
   - Delta < -0.01: regression detected.
   - Delta > +0.01: improvement detected.
   - Otherwise: no significant change (within noise tolerance).

3. **Regression severity** (for negative deltas):
   - **Critical**: delta <= -0.5, or any drop on the Success dimension >= 0.5 (e.g., pass to fail).
   - **Warning**: delta <= -0.2.
   - **Minor**: delta < -0.2 (small degradation).

4. **Overall verdict**:
   - **Regressed**: any Critical regression exists, or overall score delta < -0.1.
   - **Improved**: overall score delta > +0.05 and no Critical regressions.
   - **Stable**: everything else.

The asymmetric thresholds (0.05 for improvement, 0.1 for regression) reflect a conservative posture: it is easier to declare a regression than an improvement, reducing false confidence in candidates.

### Batch comparison (`BatchComparison`)

`BatchComparison::from_paired(baselines, candidates)` matches scorecards by `run_id` and produces:

- Individual `ScorecardComparison` for each matched pair.
- Counts: improved, stable, regressed.
- Overall verdict: **Regressed** if any scenario regressed; **Improved** if improved count exceeds stable count and none regressed; **Stable** otherwise.

Unmatched scorecards (present in baseline but not candidate, or vice versa) are silently skipped. This allows incremental scenario adoption without breaking batch comparisons.

### Report generation

`ScorecardComparison::to_report()` produces a human-readable text block:

```
Scorecard Comparison: baseline-v0.5 vs candidate-v0.6
  Overall: 0.82 -> 0.78 (delta: -0.04) [==]
  REGRESSIONS (1):
    Warning verification_coverage: 0.90 -> 0.65 (-0.25) [Warning]
  IMPROVEMENTS (1):
    cost_efficiency: 0.60 -> 0.85 (+0.25)
  Verdict: [==] stable
```

## Consequences

### Positive

- **Multi-dimensional evaluation**: Runs are assessed on 7 independent axes, surfacing quality problems that pass/fail hides. A run that succeeds but has poor verification coverage, high cost, or no mission memory will score lower than a run that succeeds cleanly.
- **Regression detection**: Pairwise comparison with severity classification enables automated regression gates in CI. A batch comparison across a scenario suite can block a release if any scenario regresses critically.
- **Weighted flexibility**: Default weights encode the project's priorities (correctness > trust > coverage > cost), but the weighted-average computation is transparent and could be made configurable in `oco.toml` in a future iteration.
- **Leverages existing data**: All dimension scores are derived from data already captured by the orchestrator (`VerificationState`, `TrustVerdict`, `MissionMemory`, `CostMetrics`, trace events). No new runtime instrumentation is required.
- **Batch support**: `BatchComparison` enables suite-level regression detection, which is the primary use case for `oco eval compare`.
- **Human-readable reports**: `to_report()` produces structured text suitable for terminal output, PR comments, or CI logs.

### Negative / Risks

- **Weight subjectivity**: Default weights are a judgment call. Different teams may disagree on whether CostEfficiency should weigh as much as VerificationCoverage. Mitigation: weights are defined as a method on `ScorecardDimension`, making them easy to override in a future configuration surface.
- **Dimension coupling**: Some dimensions are correlated. A failed run (Success = 0.0) will likely also have low TrustVerdict and VerificationCoverage, causing a "pile-on" effect in the composite score. This is intentional (failed runs should score very low) but means the composite score is not a linear combination of independent signals.
- **Silent skip on unmatched scenarios**: `BatchComparison` silently ignores scenarios that do not appear in both baseline and candidate sets. If a scenario is accidentally renamed or dropped, the batch comparison will not flag the gap. Mitigation: `total_scenarios` is reported, and operators can compare it to the expected count.
- **New type surface**: `RunScorecard`, `DimensionScore`, `CostMetrics`, `ScorecardComparison`, `RegressionFlag`, `ImprovementFlag`, `ComparisonVerdict`, `RegressionSeverity`, and `BatchComparison` add 9 types to `shared-types`. All require serialization tests and documentation.
- **Static scoring rules**: Dimension scores are computed by fixed formulas (e.g., ReplanStability thresholds). These may need tuning as OCO matures and replanning behavior evolves.

### Neutral

- **No breaking changes**: All new types are additive. Existing `RunSummary`, `MissionMemory`, and `VerificationState` are unchanged.
- **Serialization format**: JSON, consistent with existing run artifacts. Scorecards can be persisted alongside `summary.json` and `mission.json`.
- **No runtime overhead**: Scorecard computation happens after the run completes (in the eval runner or in `oco runs compare`). It does not affect orchestration performance.

## Alternatives Considered

1. **Single composite score only**: Compute one number (0.0--1.0) without exposing per-dimension detail. Rejected: loses diagnostic value. When a regression occurs, operators need to know which dimension degraded.

2. **Pass/fail per dimension with no weighting**: Boolean pass/fail on each dimension, overall pass = all pass. Rejected: too brittle. A run with 99% verification coverage and one unverified generated file would fail identically to a run with 0% coverage.

3. **External evaluation framework**: Delegate scoring to an external tool (e.g., a Python harness). Rejected: adds a deployment dependency and breaks the "local-first" design principle. The Python `eval-harness` can consume scorecards but should not be required to produce them.

4. **LLM-based quality assessment**: Use an LLM to evaluate run quality. Rejected: violates the "deterministic policy" principle. Scorecard computation must be reproducible and explainable without LLM involvement.

## Related

- ADR-004: Verification Integrity Model -- provides `VerificationState` and freshness computation used by the VerificationCoverage dimension.
- ADR-009: Session Continuity Model -- provides `CompactSnapshot` and `PolicyPack` that interact with session-level evaluation.
- ADR-010: Mission Memory Model -- provides `MissionMemory` used by the MissionContinuity dimension.

## References

- `RunScorecard` and supporting types: `crates/shared-types/src/scorecard.rs`
- `VerificationState` and `VerificationFreshness`: `crates/shared-types/src/verification.rs`
- `TrustVerdict`: `crates/shared-types/src/telemetry.rs`
- `MissionMemory`: `crates/shared-types/src/mission.rs`
- `RunSummary` and `ReplayScenario`: `crates/orchestrator-core/src/eval.rs`
- Run artifacts: `.oco/runs/<id>/`
- Eval scenarios: `examples/eval-scenarios.jsonl`, `examples/benchmark-v0.5.jsonl`
