# ADR-016: Baseline Promotion & Audit Trail

## Status

Accepted

## Date

2026-04-04

## Context

### The gap between saving a baseline and knowing why it was saved

ADR-012 introduced `EvalBaseline` for persisting scorecard snapshots, ADR-013 added per-repo gate configuration with `baseline_path`, and ADR-014 provided freshness detection and review artifacts. Together, these give OCO a credible quality gate. But one critical operation remains unmanaged: **replacing the current baseline with a new one**.

Today, `oco baseline-save` overwrites the file at `gate.baseline_path` with no record of what was replaced, why the replacement happened, or what changed. This creates three problems:

1. **No audit trail.** A team cannot answer "when was the baseline last updated, by whom, and was the gate passing at that point?" The overwrite is silent and irreversible.

2. **No diff visibility.** When a baseline is replaced, the per-dimension score changes are lost. A reviewer cannot tell whether the new baseline represents an improvement, a regression accepted under duress, or a lateral move after a scenario suite change.

3. **No promotion safety.** There is no signal to prevent a failing candidate from being promoted to baseline. A developer can run `baseline-save` after a gate failure, silently lowering the quality bar. The system should provide a recommendation — promote, review, or reject — based on evidence the gate already produces.

### Existing infrastructure

| Component | Location | Role |
|-----------|----------|------|
| `EvalBaseline` | `shared-types/gate.rs` | Saved scorecard snapshot with `created_at` (ADR-012) |
| `GateVerdict` | `shared-types/gate.rs` | Pass / Warn / Fail with exit codes (ADR-012) |
| `GateConfig` | `shared-types/gate.rs` | Per-repo gate config with `baseline_path` (ADR-013) |
| `BaselineFreshness` | `shared-types/gate.rs` | Fresh / Aging / Stale / Unknown (ADR-014) |
| `BaselineFreshnessCheck` | `shared-types/gate.rs` | Freshness evaluator with configurable thresholds (ADR-014) |
| `RunScorecard` | `shared-types/scorecard.rs` | 7-dimension quality scorecard (ADR-011) |
| `oco baseline-save` | `dev-cli/` | CLI command that saves a scorecard as a named baseline |

### What prior ADRs do not cover

| Gap | Impact |
|-----|--------|
| No promotion recommendation | A failing candidate can silently become the baseline |
| No baseline diff | Score changes between old and new baseline are invisible |
| No promotion record | No structured artifact captures what was replaced, when, and why |
| No audit history | Repeated promotions leave no trace — impossible to reconstruct baseline evolution |
| No atomic backup | Overwriting the baseline with no backup is a destructive, irreversible operation |

## Decision

### Introduce baseline promotion with deterministic recommendations and an append-only audit trail

Six additions to `shared-types/gate.rs`:

#### 1. `PromotionRecommendation` enum

```rust
pub enum PromotionRecommendation {
    Promote,  // candidate passes all checks — safe to promote
    Review,   // candidate has warnings — promotion requires explicit review
    Reject,   // candidate fails critical checks — do not promote
}
```

A deterministic recommendation derived from gate verdict and baseline freshness via `from_gate_and_freshness()`:

| Gate Verdict | Freshness | Recommendation |
|-------------|-----------|----------------|
| Fail | any | **Reject** |
| Warn | any | **Review** |
| Pass | Stale | **Review** |
| Pass | Fresh / Aging / Unknown | **Promote** |

The logic is a pure function — no LLM calls, no heuristics, no configuration. A `Reject` aborts the promotion by default — the user must provide `--force` to override, and the override is recorded in the audit trail. Methods: `label()` returns `"promote"` / `"review"` / `"reject"`, `symbol()` returns `[PROMOTE]` / `[REVIEW]` / `[REJECT]`.

#### 2. `BaselineDiffSummary`

```rust
pub struct BaselineDiffSummary {
    pub dimension_deltas: Vec<DimensionDelta>,
    pub old_overall: f64,
    pub new_overall: f64,
    pub overall_delta: f64,
    pub summary: String,
}
```

Computes per-dimension score deltas between the old baseline scorecard and the new candidate scorecard via `compute(old, new)`. The `summary` field is a human-readable one-liner (e.g., `"0.72 -> 0.85 (+0.13): 5 improved, 1 regressed, 1 unchanged"`). Dimensions with delta > 0.01 are classified as improved, delta < -0.01 as regressed, otherwise unchanged.

Output formats:
- **`to_report()`** — plain-text table for terminal display.
- **`to_markdown()`** — Markdown table for PR comments and review artifacts.

#### 3. `DimensionDelta`

```rust
pub struct DimensionDelta {
    pub dimension: ScorecardDimension,
    pub old_score: f64,
    pub new_score: f64,
    pub delta: f64,
}
```

A single dimension's score change. Used by `BaselineDiffSummary` to provide granular visibility into what changed between baselines.

#### 4. `PromotionRecord`

```rust
pub struct PromotionRecord {
    pub promoted_at: DateTime<Utc>,
    pub old_baseline_name: String,
    pub new_baseline_name: String,
    pub source: String,
    pub reason: Option<String>,
    pub recommendation: PromotionRecommendation,
    pub gate_verdict: Option<GateVerdict>,
    pub baseline_freshness: Option<BaselineFreshness>,
    pub diff: BaselineDiffSummary,
}
```

A durable record of a single baseline promotion event. Captures the full context at promotion time: who replaced what, the computed recommendation, the gate and freshness state, and the score diff. The `source` field identifies where the new baseline came from (a run ID, a file path, or a scorecard reference). The `reason` field is an optional human-provided justification.

Output formats:
- **`to_summary()`** — compact multi-line summary for terminal and history reports.
- **`to_markdown()`** — structured Markdown table with the embedded diff table for review artifacts.

#### 5. `BaselineHistoryEntry`

```rust
pub struct BaselineHistoryEntry {
    pub sequence: u32,
    pub promotion: PromotionRecord,
}
```

A wrapper that pairs a `PromotionRecord` with a monotonic 1-based sequence number. The sequence number provides a stable, human-readable identifier for each promotion event.

#### 6. `BaselineHistory`

```rust
pub struct BaselineHistory {
    pub schema_version: u32,   // currently 1
    pub entries: Vec<BaselineHistoryEntry>,
}
```

The full baseline audit trail for a repository. Persisted as `.oco/baseline-history.json`. Key behaviors:

- **Append-only**: `append(promotion)` adds a new entry and returns the assigned sequence number. No entry is ever removed or modified in normal operation.
- **Schema-versioned**: `schema_version` enables forward-compatible migrations. Current version is `BASELINE_HISTORY_SCHEMA_VERSION = 1`.
- **Graceful load**: `load_from(path)` returns an empty history if the file does not exist, allowing first-time promotion to work without manual initialization.
- **Persistence**: `save_to(path)` writes pretty-printed JSON. `to_json()` serializes without writing to disk.

Query methods:
- `latest()` — most recent entry.
- `recent(n)` — last `n` entries, most recent first.
- `len()`, `is_empty()` — size queries.

Output formats:
- **`to_report()`** — plain-text report with all entries (most recent first).
- **`to_markdown()`** — Markdown document with per-entry promotion tables and diff tables.
- **`to_json()`** — pretty-printed JSON for machine consumption.

### Promotion workflow

The promotion operation (triggered by `oco baseline-promote`) follows this sequence:

```
1. Load current baseline from gate.baseline_path
2. Load candidate scorecard (from run ID, file, or last eval)
3. Compute BaselineDiffSummary between old and new
4. Compute PromotionRecommendation from gate verdict + freshness
5. If Reject and no --force: abort with explanation
6. Back up current baseline to .oco/baseline.json.bak (atomic)
7. Overwrite gate.baseline_path with new baseline
8. Create PromotionRecord with full context
9. Load BaselineHistory, append record, save
```

The backup step (6) ensures that a promotion can be manually reverted by restoring the `.bak` file. This is a safety net, not a versioning system — the history file is the authoritative audit trail.

### CLI surface

Two new subcommands:

```bash
oco baseline-promote <source>              # Promote a candidate to baseline
oco baseline-promote last                  # Promote from last run
oco baseline-promote last --reason "..."   # With human justification
oco baseline-promote last --force          # Override Reject recommendation
oco baseline-history                       # Show promotion history
oco baseline-history --json                # Machine-readable history
oco baseline-history --markdown            # Markdown history document
oco baseline-history --limit 5             # Last 5 entries only
```

### File layout

```
.oco/
├── baseline.json              # Current baseline (gate.baseline_path)
├── baseline.json.bak          # Backup of previous baseline (overwritten each promotion)
├── baseline-history.json      # Append-only audit trail
└── runs/                      # Existing run artifacts
```

### Design principles

- **Backward compatible**: Promotion and history are additive. Existing `oco baseline-save` continues to work unchanged — it writes a baseline without going through the promotion workflow. Teams adopt `oco baseline-promote` when they want audit trail guarantees.
- **Deterministic**: `PromotionRecommendation` is a pure function of `GateVerdict` and `BaselineFreshness`. No LLM calls, no probability thresholds, no external dependencies.
- **Local-first**: The history file is plain JSON on disk. No database, no cloud service, no network dependencies. Consistent with the project's local-first principle.
- **Append-only**: The history file is never truncated or rewritten (except for JSON re-serialization when appending). This provides a reliable audit trail without the complexity of a write-ahead log or database.
- **Safe by default**: A `Reject` recommendation aborts the promotion (exit code 2). The user can override with `--force`, and the override is recorded in the history. This balances safety with developer autonomy.

## Consequences

### Positive

- **Full audit trail.** Every baseline promotion is recorded with timestamp, source, gate state, freshness, recommendation, diff, and optional human reason. Teams can reconstruct the complete history of quality bar changes.
- **Reviewable diffs.** The `BaselineDiffSummary` makes per-dimension score changes visible at promotion time. Regressions accepted during promotion are explicitly documented, not silently absorbed.
- **Promotion safety.** The `PromotionRecommendation` warns against promoting failing candidates. A `Reject` recommendation requires an explicit `--force` override, making unsafe promotions visible and auditable.
- **Atomic backup.** The `.bak` file provides a simple recovery path for accidental promotions without introducing version control complexity.
- **CI integration.** The JSON output of `oco baseline-history` can be consumed by CI pipelines, dashboards, and notification systems. The Markdown output can be included in release notes or attached to PRs.

### Negative / Risks

- **History file growth.** The history file grows unboundedly over time. A project with daily promotions over a year accumulates ~365 entries. For a JSON file with promotion records, this is a few hundred kilobytes — manageable but not zero. Mitigation: no automatic compaction in v1; a future `oco baseline-history prune --keep 50` command can be added if needed.
- **No automatic compaction.** There is no built-in mechanism to trim old history entries. Teams that need bounded storage must manage this manually or wait for a future compaction feature. Mitigation: the schema version enables adding compaction behavior in a backward-compatible way.
- **Single backup file.** Only the most recent pre-promotion baseline is backed up. If two promotions happen in sequence without manual intervention, the first backup is overwritten. Mitigation: the history file records the old baseline's scorecard data (via the diff), so the scores are never lost even if the file is overwritten.
- **Two promotion paths.** `oco baseline-save` (no audit) and `oco baseline-promote` (with audit) coexist. Teams must adopt `baseline-promote` deliberately to get audit trail guarantees. Mitigation: documentation and the `baseline-history` command make the distinction clear.

### Neutral

- **No breaking changes.** All new types and fields are additive. Existing `EvalBaseline`, `GateConfig`, and `oco baseline-save` are unchanged.
- **Builds on Q5-Q8.** `PromotionRecommendation` consumes `GateVerdict` (ADR-012) and `BaselineFreshness` (ADR-014). `BaselineDiffSummary` consumes `RunScorecard` (ADR-011). No logic is duplicated.
- **Serialization format.** JSON for the history file and machine output, Markdown for human review. Both are standard formats with no new dependencies. Consistent with `MissionMemory` (schema-versioned JSON) and `GateReviewArtifact` (Markdown + JSON).

## Alternatives Considered

1. **SQLite for the audit trail.** Store promotion records in a SQLite database instead of a JSON file. Rejected: SQLite adds a runtime dependency and operational complexity (migrations, connection management, corruption recovery) that is disproportionate for an append-only log of a few hundred records. The JSON file is human-readable, diffable in version control, and trivially portable.

2. **Git tags as audit trail.** Tag each promotion with a Git annotated tag (e.g., `baseline/v3`). Rejected: this couples the audit trail to Git, is not visible to non-Git tooling, requires write access to the repository, and does not work in CI environments with shallow clones or detached HEADs. The audit trail should be portable and self-contained.

3. **Embed history in the baseline file itself.** Add a `history` array to `EvalBaseline` that accumulates promotion records. Rejected: this conflates the "current state" artifact (baseline) with the "change log" artifact (history). The baseline file should be a clean snapshot that downstream tools can parse without understanding the history schema. Separation of concerns keeps both files simple.

4. **Automatic promotion after gate pass.** Automatically promote the candidate to baseline when the gate passes without manual intervention. Rejected: baseline promotion is a policy decision that should be explicit. Automatic promotion removes human judgment from a quality-critical operation and can silently shift the quality bar in unexpected ways (e.g., after a scenario suite reduction).

5. **Versioned baseline directory.** Save each baseline as a numbered file (`baseline-001.json`, `baseline-002.json`, ...) instead of overwriting a single file. Rejected: this changes the contract of `gate.baseline_path` (which expects a single file), requires garbage collection logic, and duplicates the full scorecard for each version. The history file captures the essential diff data without full duplication.

## Related

- ADR-011: Evaluation Scorecard Model — provides `RunScorecard` that `BaselineDiffSummary` compares.
- ADR-012: Evaluation Gate Model — provides `GateVerdict` and `EvalBaseline` that `PromotionRecommendation` consumes.
- ADR-013: Per-Repo Gate Configuration — provides `GateConfig` with `baseline_path` that the promotion workflow targets.
- ADR-014: Baseline Lifecycle & Review Artifacts — provides `BaselineFreshness` that `PromotionRecommendation` consumes.
- ADR-015: Unified Review Packet Model — provides `ReviewPacket` that can incorporate promotion history in future iterations.

## References

- `PromotionRecommendation`, `BaselineDiffSummary`, `DimensionDelta`, `PromotionRecord`, `BaselineHistoryEntry`, `BaselineHistory`: `crates/shared-types/src/gate.rs`
- `GateVerdict`, `EvalBaseline`, `BaselineFreshness`, `GateConfig`: `crates/shared-types/src/gate.rs`
- `RunScorecard`, `ScorecardDimension`: `crates/shared-types/src/scorecard.rs`
- History file: `.oco/baseline-history.json`
- Backup file: `.oco/baseline.json.bak`
