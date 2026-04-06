# ADR-015: Unified Review Packet Model

## Status

Accepted

## Date

2026-04-04

## Context

### The gap between separate artifacts and a reviewable merge decision

ADR-010 through ADR-014 introduced five quality artifacts:

| # | Artifact | Location | What it answers |
|---|----------|----------|----------------|
| Q4 | `MissionMemory` | `mission.json` | What does the orchestrator know, believe, plan? |
| Q5 | `RunScorecard` | `scorecard.json` | How did the run perform across 7 dimensions? |
| Q6 | `GateResult` | (in-memory / `--report`) | Does the candidate pass quality thresholds? |
| Q7 | `GateConfig` | `oco.toml` | What is the repo's quality contract? |
| Q8 | `GateReviewArtifact` | `gate-report.{md,json}` | Is the baseline credible? What are the gate details? |

Each artifact answers one question well. But a reviewer asking "can I merge this run?" must open 3-4 files, mentally combine verdicts, cross-reference baseline freshness, and check for missing data. This friction slows review cycles and makes CI integration harder.

Teams need a single artifact that answers all review questions in one read:

1. What changed?
2. What was verified?
3. What is the trust verdict?
4. What is the gate verdict?
5. Is the baseline credible?
6. What risks remain open?
7. **Is this run merge-ready?**

### Design constraints

- **Aggregate, don't duplicate.** The review packet must reference existing artifacts, not reinvent their logic. Scorecard computation stays in `ScorecardBuilder`, gate evaluation stays in `GateResult::evaluate`, freshness stays in `BaselineFreshnessCheck`.
- **Graceful degradation.** Not every run produces every artifact. A quick run may have no mission memory, no baseline, and no gate result. The packet must say "not available" honestly, not invent data.
- **Deterministic.** The merge-readiness verdict is a pure function of trust, gate, freshness, and risks. No LLM calls.
- **Local-first.** The packet is built from files on disk (`.oco/runs/<id>/`). No network dependencies.

## Decision

### Introduce `ReviewPacket` as the unified merge-readiness bundle

A new module `review_packet.rs` in `shared-types` provides:

- **`MergeReadiness`** — the final verdict: Ready, ConditionallyReady, NotReady, Unknown.
- **`ReviewPacket`** — the unified bundle aggregating scorecard, gate result, mission memory, baseline freshness, and a computed merge-readiness verdict.
- **`ChangesSummary`**, **`VerificationSummary`**, **`OpenRisks`** — thin section wrappers that extract relevant fields from existing artifacts.

A new module `review_packet.rs` in `orchestrator-core` provides:

- **`build_review_packet()`** — loads artifacts from `.oco/runs/<id>/` and delegates to `ReviewPacket::build()`.

A new CLI subcommand:

- **`oco runs review-pack <id>`** — generates and displays the review packet with `--json`, `--markdown`, and `--save` options.

### MergeReadiness computation

```
Gate Fail                           → NotReady
Trust None                          → NotReady
No trust data at all                → Unknown
Gate missing OR Freshness missing   → ConditionallyReady (incomplete evidence)
Gate Warn OR Baseline Aging/Stale   → ConditionallyReady
  OR Trust Low OR Open Risks
Gate Pass + Trust High/Medium       → Ready
  + Baseline Fresh + no blockers
```

**Key invariant:** `Ready` requires *all* critical evidence present and positive. Missing gate or missing freshness is incomplete evidence, not a green light.

### ReviewPacket structure

```rust
pub struct ReviewPacket {
    pub schema_version: u32,
    pub generated_at: DateTime<Utc>,
    pub run_id: String,
    pub merge_readiness: MergeReadiness,
    pub trust_verdict: Option<TrustVerdict>,
    pub gate_verdict: Option<GateVerdict>,
    pub changes: ChangesSummary,
    pub verification: VerificationSummary,
    pub open_risks: OpenRisks,
    pub scorecard: Option<RunScorecard>,
    pub gate_result: Option<GateResult>,
    pub baseline_freshness: Option<BaselineFreshnessCheck>,
}
```

All artifact fields are `Option` because not every run produces every artifact.

### CLI surface

```bash
oco runs review-pack last              # Terminal output
oco runs review-pack last --json       # JSON output
oco runs review-pack last --markdown   # Markdown output
oco runs review-pack last --save       # Save to run dir
oco runs review-pack last --save ./out # Save to custom dir
```

### Fail-closed config loading

`oco runs review-pack` uses `load_gate_config_strict()` (ADR-013 contract). If `oco.toml` exists but contains an invalid `[gate]` section, the command fails with an explicit error rather than silently falling back to defaults. This is consistent with `oco eval-gate` behavior.

### Rendering

- **`to_review_text()`** — structured plain text for terminal.
- **`to_markdown()`** — Markdown for PR comments / CI archives.
- **`to_json()`** — machine-readable JSON for downstream tooling.

## Consequences

### Positive

- **Single-read review.** A reviewer reads one document and gets trust, gate, baseline, scorecard, changes, risks, and merge readiness in one pass.
- **CI-friendly.** The JSON output can be parsed by CI pipelines. The merge-readiness verdict can gate deployments.
- **Graceful degradation.** Missing artifacts produce "not available" entries, not errors. A minimal run still produces a useful (if incomplete) review packet.
- **No duplication.** `ReviewPacket::build()` delegates to existing types. Scorecard computation, gate evaluation, and freshness checks are not reimplemented.
- **Schema-versioned.** `schema_version` enables forward compatibility, consistent with `MissionMemory`.

### Negative / Risks

- **New type surface.** `ReviewPacket`, `MergeReadiness`, `ChangesSummary`, `VerificationSummary`, `OpenRisks` add 5 types to `shared-types`. All tested.
- **Merge-readiness heuristic.** The readiness computation encodes a judgment call (e.g., "open risks = conditional"). Teams may disagree. The logic is transparent and documented but not configurable in v1.

### Neutral

- **No breaking changes.** All new types are additive. Existing artifacts and CLI commands are unchanged.
- **Builds on Q4-Q8.** The review packet composes existing artifacts without modifying them.

## Related

- ADR-010: Mission Memory Model — provides facts, hypotheses, risks, narrative.
- ADR-011: Evaluation Scorecard Model — provides per-dimension quality scores.
- ADR-012: Evaluation Gate Model — provides pass/warn/fail verdict.
- ADR-013: Per-Repo Gate Configuration — provides baseline path and policy.
- ADR-014: Baseline Lifecycle & Review Artifacts — provides freshness and review documents.

## References

- `ReviewPacket`, `MergeReadiness`: `crates/shared-types/src/review_packet.rs`
- `build_review_packet()`: `crates/orchestrator-core/src/review_packet.rs`
- CLI: `apps/dev-cli/src/main.rs` (`oco runs review-pack`)
