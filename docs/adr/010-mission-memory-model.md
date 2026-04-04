# ADR-010: Mission Memory Model

## Status

Accepted

## Context

### The inter-session boundary problem

ADR-009 introduced `CompactSnapshot` as the survival unit for intra-session compaction. It solves the immediate problem: when Claude Code compacts the context window mid-session, OCO preserves verified facts, hypotheses, plan state, and verification freshness so the orchestrator can resume without re-discovering everything.

However, `CompactSnapshot` is scoped to a single session. When a session ends -- whether by budget exhaustion, user stop, task completion, or process termination -- the snapshot is meaningless. A new `oco run` invocation starts with a blank slate. All accumulated knowledge is lost.

For complex tasks that span multiple sessions (multi-day investigations, iterative fixes, large refactors), OCO needs a durable artifact that survives inter-session boundaries. This artifact must answer five questions:

1. **What is true?** -- Verified facts with provenance.
2. **What is hypothetical?** -- Active hypotheses with confidence levels.
3. **What is open?** -- Unanswered questions that need investigation.
4. **What is the plan?** -- Current objective, completed steps, remaining work, execution phase.
5. **What is the confidence level?** -- Verification status, trust verdict, identified risks.

### Existing infrastructure

| Component | Location | Role |
|-----------|----------|------|
| `CompactSnapshot` | `shared-types/memory.rs` | Intra-session compaction survival (ADR-009) |
| `WorkingMemory` | `shared-types/memory.rs` | Session state: findings, facts, hypotheses, questions, plan, planner state |
| `VerificationState` | `shared-types/verification.rs` | Modified files, verification runs, freshness computation |
| `TrustVerdict` | `shared-types/telemetry.rs` | High / Medium / Low / None trust assessment |
| `OrchestrationState` | `orchestrator-core/state.rs` | Full runtime state during orchestration |
| Run artifacts | `.oco/runs/<id>/` | `trace.jsonl` + `summary.json` persisted per run |
| `SessionManager` | `mcp-server/hooks.rs` | Session lifecycle and state access |

### What CompactSnapshot does not cover

| Gap | Impact |
|-----|--------|
| Inter-session handoff | New session starts from zero; previous knowledge is inaccessible |
| Human review | CompactSnapshot is a minimal wire format, not designed for readability |
| Hypothesis provenance | Evidence links are dropped during compaction |
| Key decisions | Not captured in CompactSnapshot at all |
| Risk tracking | Not captured in CompactSnapshot at all |
| Narrative summary | No mechanism to carry forward a high-level description of mission state |

## Decision

### Introduce `MissionMemory` as the durable inter-session handoff artifact

A new type `MissionMemory` in `shared-types/mission.rs` serves as the authoritative record of what the orchestrator knows, believes, questions, and plans. Unlike `CompactSnapshot` (optimized for fast intra-session reinjection), `MissionMemory` is designed for durability, completeness, and human readability.

```rust
pub struct MissionMemory {
    /// Schema version for forward compatibility.
    pub schema_version: u32,
    /// Session ID that created this mission memory.
    pub session_id: SessionId,
    /// When this mission memory was captured.
    pub created_at: DateTime<Utc>,
    /// The original user request / mission statement.
    pub mission: String,

    // -- Epistemic state --
    pub facts: Vec<MissionFact>,
    pub hypotheses: Vec<MissionHypothesis>,
    pub open_questions: Vec<String>,

    // -- Plan state --
    pub plan: MissionPlan,

    // -- Verification state --
    pub verification: MissionVerificationStatus,

    // -- Artifact metadata --
    pub modified_files: Vec<String>,
    pub key_decisions: Vec<String>,
    pub risks: Vec<String>,
    pub trust_verdict: TrustVerdict,
    pub narrative: String,
}
```

### Supporting types

**`MissionFact`** -- A verified fact with provenance:
- `content: String` -- human-readable fact text.
- `source: Option<String>` -- where the fact was established (file path, tool output).
- `established_at: DateTime<Utc>` -- when the fact was confirmed.

**`MissionHypothesis`** -- An active hypothesis with confidence and evidence:
- `content: String` -- hypothesis text.
- `confidence_pct: u8` -- confidence as integer percentage (0-100).
- `supporting_evidence: Vec<String>` -- human-readable evidence summaries.

**`MissionPlan`** -- Current plan state:
- `current_objective: Option<String>` -- the immediate objective.
- `completed_steps: Vec<String>` -- steps already done.
- `remaining_steps: Vec<String>` -- steps still to do.
- `phase: Option<String>` -- current execution phase (explore, implement, verify).

**`MissionVerificationStatus`** -- Verification snapshot:
- `freshness: VerificationFreshness` -- Fresh / Partial / Stale / None.
- `unverified_files: Vec<String>` -- files modified but not yet verified.
- `last_check: Option<DateTime<Utc>>` -- when the last verification ran.
- `checks_passed: Vec<String>` -- verification strategies that passed.
- `checks_failed: Vec<String>` -- verification strategies that failed.

### Construction: `from_working_state()`

`MissionMemory` is built from live orchestration state at session end:

```
from_working_state(session_id, mission, &WorkingMemory, &VerificationState, TrustVerdict)
```

The builder:
1. Extracts verified facts from `WorkingMemory.verified_facts`, mapping each to a `MissionFact` with content, source, and timestamp.
2. Filters hypotheses to active-only (`MemoryStatus::Active`), computing `confidence_pct` from `effective_confidence()`.
3. Copies open questions from `WorkingMemory.questions`.
4. Builds `MissionPlan` from `WorkingMemory.plan` and `WorkingMemory.planner_state` (current step, phase).
5. Computes `MissionVerificationStatus` from `VerificationState`: freshness, unverified files (modified after last check), passed/failed strategies.
6. Collects modified file paths from `VerificationState.modified_files`.

Fields not yet populated by the builder (`key_decisions`, `risks`, `narrative`) default to empty. These are intended to be set explicitly by higher-level orchestration logic or by the user.

### Rendering: `to_handoff_text()`

`to_handoff_text()` produces a structured, human-readable text block suitable for review or LLM context injection. Each section is conditionally included only when non-empty:

```
OCO Mission Handoff
====================
Mission: <original request>
Session: <session UUID>
Captured: <timestamp>
Trust: <high|medium|low|none>

VERIFIED FACTS (<count>):
  - <fact> (source: <path>)

ACTIVE HYPOTHESES (<count>):
  - <hypothesis> (confidence: <N>%)

OPEN QUESTIONS (<count>):
  ? <question>

PLAN:
  Current objective: <objective>
  Phase: <phase>
  [done] 1. <completed step>
  [todo] 1. <remaining step>

VERIFICATION:
  Freshness: <Fresh|Partial|Stale|None>
  Last check: <timestamp>
  Passed: <strategies>
  Failed: <strategies>
  ! <unverified file>

MODIFIED FILES (<count>):
  - <path>

KEY DECISIONS:
  - <decision>

RISKS:
  ! <risk>

NARRATIVE:
  <free-form summary>
```

### Persistence

`MissionMemory` is persisted as pretty-printed JSON via `save_to()`:

```
.oco/runs/<session-id>/mission.json
```

This sits alongside the existing `trace.jsonl` and `summary.json` run artifacts. The save operation:
1. Creates parent directories if they do not exist (`create_dir_all`).
2. Serializes to pretty-printed JSON via `serde_json::to_string_pretty`.
3. Writes atomically to the target path.

### Loading and resume

**`load_from(path)`** loads a `MissionMemory` from disk with schema validation:
1. Checks file existence (returns `MissionLoadError::NotFound` if missing).
2. Parses raw JSON into `serde_json::Value`.
3. Checks `schema_version`: if the on-disk version exceeds `MISSION_SCHEMA_VERSION`, returns `MissionLoadError::IncompatibleSchema`. Older versions are accepted (backward compatible via `#[serde(default)]` on all optional fields).
4. Deserializes into `MissionMemory`.

**`merge_from_previous(previous)`** carries knowledge forward when resuming:
- Facts are merged with content-based deduplication.
- Hypotheses are merged with content-based deduplication.
- Open questions are merged with exact-string deduplication.
- Key decisions are merged with exact-string deduplication.
- Risks and narrative are NOT merged (they belong to the originating session).

**`restore_from_mission()`** (planned) will be the orchestrator-side restore path that:
1. Loads a `MissionMemory` via `load_from()`.
2. Populates `WorkingMemory` with facts, hypotheses, and questions from the loaded mission.
3. Restores `PlannerState` from `MissionPlan`.
4. Seeds `VerificationState` with unverified files and freshness.

### Schema versioning strategy

- `MISSION_SCHEMA_VERSION` is a `u32` constant (currently `1`), bumped when the on-disk format changes.
- `load_from()` rejects files with `schema_version > MISSION_SCHEMA_VERSION` (future versions are incompatible).
- Older schema versions are accepted: all new fields use `#[serde(default)]` or `#[serde(default, skip_serializing_if)]`, so missing fields deserialize to their default values.
- This provides backward compatibility without migration logic for additive changes.

## Relationship to CompactSnapshot

| Dimension | CompactSnapshot (ADR-009) | MissionMemory (this ADR) |
|-----------|--------------------------|--------------------------|
| **Scope** | Intra-session | Inter-session |
| **Trigger** | PreCompact/PostCompact hooks | Session end (budget, completion, stop) |
| **Lifetime** | Ephemeral (in-memory + session disk) | Durable (persisted to `.oco/runs/`) |
| **Content** | Minimal: facts, hypotheses, plan, verification freshness, inspected areas | Full: facts, hypotheses, questions, plan, verification, decisions, risks, narrative, modified files |
| **Format** | Structured text for LLM reinjection | JSON on disk + human-readable text via `to_handoff_text()` |
| **Resume** | Automatic (PostCompact hook) | Manual (`--resume <session-id>`) |
| **Audience** | LLM (post-compaction context) | Human reviewer + LLM (next session) |
| **Trust model** | PolicyPack (Fast/Balanced/Strict) | Merge-based (deduplicated carry-forward) |

Both can coexist. During a session, CompactSnapshot handles compaction events. At session end, MissionMemory captures the full epistemic state for durable handoff. MissionMemory is a strict superset in terms of content: it includes everything CompactSnapshot captures plus key decisions, risks, narrative, modified files, and richer hypothesis evidence.

## Guarantees

1. **Schema versioning**: The `schema_version` field enables forward-compatible rejection. Files from future OCO versions are refused rather than silently misinterpreted.

2. **Atomic save**: `save_to()` writes the complete JSON in a single `std::fs::write` call after `create_dir_all`. Parent directory creation is idempotent.

3. **No data invention**: `from_working_state()` only extracts data already present in `WorkingMemory` and `VerificationState`. No synthetic facts, hypotheses, or decisions are generated. Empty fields default to empty collections or empty strings.

4. **Backward compatible defaults**: All optional and collection fields use `#[serde(default)]`, so mission files from older schema versions (with fewer fields) deserialize successfully with sensible defaults.

5. **Deduplication on merge**: `merge_from_previous()` deduplicates by content string, preventing fact/hypothesis/question duplication across sessions.

## Limitations

1. **No automatic resume**: Resuming from a previous mission requires the user to explicitly pass `--resume <session-id>`. There is no automatic detection of incomplete missions or suggestion to resume.

2. **Narrative is empty unless set**: `from_working_state()` sets `narrative` to an empty string. A meaningful narrative must be set explicitly by orchestration logic or by the user. This is by design (no data invention) but means the narrative section is absent in the handoff text unless populated.

3. **No incremental save**: `MissionMemory` is captured once at session end, not updated incrementally during the session. If the process crashes before session end, no mission memory is persisted. The existing `trace.jsonl` provides crash recovery data.

4. **Key decisions and risks are not auto-populated**: `from_working_state()` leaves `key_decisions` and `risks` empty. These require explicit orchestrator logic to populate (e.g., recording decisions made during replanning, risks identified during verification).

5. **Merge is additive only**: `merge_from_previous()` adds knowledge from a previous session but does not remove facts that may have been invalidated. If a fact from session N is contradicted in session N+1, both remain until the orchestrator explicitly removes the stale fact.

6. **Disk I/O on critical path**: `save_to()` performs synchronous file I/O. For typical mission sizes (under 100 KB JSON), this is negligible, but it is not async.

## Consequences

### Positive

- **Durable handoff**: Complex multi-session tasks can carry forward verified knowledge, hypotheses, and plan state across session boundaries, eliminating the cold-start problem.
- **Human reviewable**: `to_handoff_text()` produces a structured, readable summary that operators can inspect via `oco runs show <id> --mission` to understand what the orchestrator knew, believed, and planned.
- **Resumable**: `merge_from_previous()` enables a new session to inherit accumulated knowledge, reducing re-discovery work for continuation tasks.
- **Schema-safe**: Version-gated loading prevents silent data corruption when OCO versions diverge.
- **Leverages existing infrastructure**: Built on `WorkingMemory`, `VerificationState`, and `TrustVerdict` already in `shared-types`. No new crates or external dependencies.

### Negative

- **New type surface**: `MissionMemory`, `MissionFact`, `MissionHypothesis`, `MissionPlan`, `MissionVerificationStatus`, and `MissionLoadError` add six types to `shared-types`. Tests required for serialization, construction, merge, and persistence.
- **Disk I/O**: Each session end writes a JSON file to `.oco/runs/<id>/`. Negligible for normal use but adds a filesystem dependency.
- **Manual resume**: Users must remember to pass `--resume` to benefit from mission continuity. Discoverability depends on CLI hints and documentation.

### Neutral

- **No breaking changes**: All new types are additive. Sessions that do not use `--resume` are unaffected.
- **Coexists with CompactSnapshot**: The two mechanisms serve different lifecycle phases and do not interfere.
- **Serialization format**: JSON, consistent with existing run artifacts (`trace.jsonl`, `summary.json`).

## References

- `MissionMemory` and supporting types: `crates/shared-types/src/mission.rs`
- `CompactSnapshot` and `WorkingMemory`: `crates/shared-types/src/memory.rs`
- `VerificationState` and `VerificationFreshness`: `crates/shared-types/src/verification.rs`
- `TrustVerdict`: `crates/shared-types/src/telemetry.rs`
- `OrchestrationState`: `crates/orchestrator-core/src/state.rs`
- Session management: `crates/mcp-server/src/hooks.rs`
- ADR-009 Session Continuity Model: `docs/adr/009-session-continuity-model.md`
- ADR-005 Working Memory: `docs/adr/005-working-memory.md`
