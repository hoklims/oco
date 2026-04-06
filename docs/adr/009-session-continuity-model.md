# ADR-009: Session Continuity Model

## Status

Accepted

## Context

### The compaction problem

Claude Code operates under a finite context window. When a session accumulates enough conversation history (tool outputs, observations, reasoning), Claude Code compacts the context: it summarizes the conversation into a shorter form and discards the original messages. This is a lossy operation.

For short tasks this is irrelevant. For Medium+ orchestrated tasks that span dozens of tool calls and verification cycles, compaction destroys critical state:

- **Verified facts** that took multiple tool calls to establish must be re-discovered.
- **Active hypotheses** with accumulated supporting/contradicting evidence lose their evidence links.
- **Planner state** (current step, phase, replan count) is lost, causing the orchestrator to restart from scratch or repeat already-completed work.
- **Verification freshness** is unknown: the orchestrator cannot tell whether modified files have been verified since their last change.
- **Inspected areas** (files and symbols already analyzed) disappear, leading to redundant re-reads.

OCO already maintains a `WorkingMemory` structure (`shared-types/memory.rs`) that tracks all of the above during a session. It also exposes a `compact_snapshot()` method that produces a minimal JSON representation of the active state. Claude Code emits `PreCompact` and `PostCompact` hook events (documented in `claude-adapter/events.rs`) that bracket the compaction lifecycle.

The missing piece is a formal model that defines exactly what survives compaction, how it is captured, how it is reinjected, and what trust level applies to restored state.

### Existing infrastructure

| Component | Location | Role |
|-----------|----------|------|
| `WorkingMemory` | `shared-types/memory.rs` | Session state: findings, facts, hypotheses, questions, plan, planner state, inspected areas |
| `WorkingMemory::compact_snapshot()` | `shared-types/memory.rs` | Produces minimal JSON with active entries only (no audit trail) |
| `PlannerState` | `shared-types/memory.rs` | Current step, replan count, phase, lease ID |
| `VerificationState` | `shared-types/verification.rs` | Modified files, verification runs, freshness computation |
| `VerificationFreshness` | `shared-types/verification.rs` | Fresh / Partial / Stale / None |
| `VerificationTier` | `shared-types/verification.rs` | Light / Standard / Thorough |
| `hook_pre_compact` | `mcp-server/hooks.rs` | Hook handler called before compaction (snapshot trigger) |
| `hook_post_compact` | `mcp-server/hooks.rs` | Hook handler called after compaction (reinjection point) |
| `ClaudeHookEvent::PreCompact` | `claude-adapter/events.rs` | Event variant for pre-compaction |
| `ClaudeHookEvent::PostCompact` | `claude-adapter/events.rs` | Event variant carrying `compact_summary` |

## Decision

### Introduce `CompactSnapshot` as the survival unit

A new type `CompactSnapshot` in `shared-types/memory.rs` formalizes what survives compaction. It is the single serializable artifact that crosses the compaction boundary.

```rust
pub struct CompactSnapshot {
    /// Timestamp when the snapshot was taken.
    pub created_at: DateTime<Utc>,
    /// Verified facts (confidence = 1.0, confirmed by evidence).
    pub verified_facts: Vec<String>,
    /// Active hypotheses with their effective confidence.
    pub hypotheses: Vec<HypothesisSnapshot>,
    /// Current execution plan steps (names only, not full PlanStep).
    pub plan: Vec<String>,
    /// Planner execution state.
    pub planner_state: Option<PlannerState>,
    /// Verification freshness at snapshot time.
    pub verification_freshness: VerificationFreshness,
    /// Files modified but not yet verified at snapshot time.
    pub unverified_files: Vec<String>,
    /// Open questions that still need answers.
    pub open_questions: Vec<String>,
    /// Paths already inspected (dedup on restore).
    pub inspected_areas: Vec<String>,
    /// Session-level metadata for continuity tracking.
    pub session_id: Option<String>,
    /// Monotonic snapshot sequence number (detects stale snapshots).
    pub sequence: u64,
}

pub struct HypothesisSnapshot {
    pub text: String,
    pub confidence_pct: u8,
}
```

### PreCompact + PostCompact hook lifecycle

The compaction survival cycle uses both Claude Code hooks:

```
Session running
    |
    v
[PreCompact event] -----> hook_pre_compact handler
    |                        |
    |                        +-- Reads WorkingMemory from SessionManager
    |                        +-- Reads VerificationState
    |                        +-- Builds CompactSnapshot
    |                        +-- Persists snapshot to .oco/sessions/<id>/snapshot.json
    |                        +-- Returns 200 OK
    |
    v
[Claude Code compacts context]
    |  (conversation history is summarized, original messages discarded)
    v
[PostCompact event] ----> hook_post_compact handler
    |                        |
    |                        +-- Loads CompactSnapshot from SessionManager
    |                        +-- Serializes to human-readable text block
    |                        +-- Returns HookResponse with message containing
    |                        |   the snapshot as reinjection text
    |                        +-- Claude Code appends message to new context
    |
    v
Session continues with restored state
```

### Reinjection format

The snapshot is returned as the `message` field of `HookResponse` from the `PostCompact` handler. Claude Code includes this message in the post-compaction context. The format is structured text, not raw JSON, to maximize LLM comprehension:

```
OCO Session Continuity (snapshot #3)
=====================================

VERIFIED FACTS (trust: high):
  - Auth middleware validates JWT on every request
  - Rate limiter uses atomic counter, no locks
  - FTS5 index covers all .rs files

ACTIVE HYPOTHESES:
  - Session cookie not HttpOnly (confidence: 70%)
  - Token refresh race condition under load (confidence: 45%)

CURRENT PLAN:
  1. Fix HttpOnly flag on session cookie
  2. Add integration test for token refresh
  3. Run verification (Standard tier)

PLANNER STATE:
  step: "Fix HttpOnly flag on session cookie"
  phase: implement
  replans: 1

VERIFICATION:
  freshness: stale
  unverified files:
    - crates/mcp-server/src/hooks.rs
    - crates/shared-types/src/memory.rs

OPEN QUESTIONS:
  ? Does the rate limiter handle clock skew across replicas?

INSPECTED AREAS:
  - src/auth/middleware.rs
  - src/mcp-server/hooks.rs
  - src/shared-types/memory.rs
```

### Orchestrator restore path

When the orchestrator detects that it is operating post-compaction (via the reinjected snapshot text in context, or by loading the persisted snapshot from disk), it:

1. **Parses** the `CompactSnapshot` (from disk or by recognizing the structured text block).
2. **Restores `PlannerState`**: sets `current_step`, `phase`, `replan_count` so the GraphRunner resumes from the correct step instead of re-planning.
3. **Marks unverified files**: populates `VerificationState.modified_files` with the unverified file list, forcing re-verification before the task can complete.
4. **Rehydrates hypotheses**: injects hypotheses back into `WorkingMemory` at their snapshotted confidence level.
5. **Skips already-inspected areas**: `inspected_areas` prevents re-reading files that were already analyzed.
6. **Does NOT restore**: raw observations, tool output history, intermediate reasoning, or evidence link UUIDs (these are irrecoverably lost).

### PolicyPack: trust contract for restored state

A new `PolicyPack` enum governs how aggressively the orchestrator trusts restored state after compaction. The pack is selected per session (configurable in `oco.toml`).

| Pack | Verified facts | Hypotheses | Plan | Verification freshness | Behavior |
|------|---------------|------------|------|----------------------|----------|
| **Fast** | Trust as-is | Trust at snapshotted confidence | Resume plan from current step | Accept snapshot freshness | Minimizes re-work. Fastest resume. Risk: stale facts if compaction happened during a long edit. |
| **Balanced** (default) | Trust as-is | Degrade confidence by 20% | Resume plan but re-run current step | Force `Stale` if snapshot age > 5 min | Safe default. Re-runs the step that was active during compaction. Hypotheses require re-confirmation sooner. |
| **Strict** | Trust as-is | Degrade confidence by 50% | Resume plan but re-verify all completed steps | Force `Stale` unconditionally | Maximum safety. Re-verifies everything. Used for security-sensitive or production-critical tasks. |

PolicyPack rules:

- **Verified facts are always trusted** across all packs. By definition they were confirmed by evidence before compaction. If a fact becomes stale (e.g., the file it refers to was modified post-snapshot), the normal `VerificationFreshness` mechanism will catch it.
- **Hypotheses degrade** because their supporting evidence (the raw observations) is lost. The confidence penalty encourages the orchestrator to seek re-confirmation.
- **Plan continuity** varies by pack. Fast trusts the plan fully. Balanced re-runs the active step (which may have been interrupted by compaction). Strict re-verifies all prior steps.
- **Verification freshness** is either preserved (Fast) or forced to Stale (Balanced/Strict), which triggers a verification cycle before the task can complete.

## Structure of the Snapshot

### What survives compaction (guaranteed)

| Element | Source | Fidelity |
|---------|--------|----------|
| Verified facts | `WorkingMemory.verified_facts` | Full content, no confidence loss |
| Plan steps | `WorkingMemory.plan` | Step names in order |
| Planner state | `WorkingMemory.planner_state` | Current step, phase, replan count, lease ID |
| Verification freshness | `VerificationState.freshness()` | Enum value at snapshot time |
| Unverified files | `VerificationState.modified_files` minus verified | File paths |
| Open questions | `WorkingMemory.questions` | Content text |
| Inspected area paths | `WorkingMemory.inspected_areas` | File paths (no symbol detail) |

### What is lost (irrecoverable)

| Element | Reason |
|---------|--------|
| Raw observations | Too large for snapshot; append-only log not designed for compaction survival |
| Tool output history | Unbounded size; reinjecting would defeat the purpose of compaction |
| Evidence link UUIDs | `supporting_evidence` / `contradicting_evidence` vectors reference entries that no longer exist post-compaction |
| Invalidated entries | Audit trail only; not needed for forward progress |
| Finding details | Active findings (not yet verified) carry less certainty; hypotheses subsume them |
| Symbol-level inspection detail | Only paths survive; symbols and summaries are dropped to keep snapshot compact |
| Intermediate reasoning | Claude Code's conversation history is the source; OCO never had it |

### What is best-effort

| Element | Condition |
|---------|-----------|
| Hypothesis confidence | Restored at snapshotted value, then degraded per PolicyPack |
| Snapshot freshness itself | If `PreCompact` hook fails (timeout, server down), no snapshot is created and `PostCompact` returns an empty response |
| Disk persistence | Snapshot is written to `.oco/sessions/<id>/snapshot.json`; if disk write fails, the in-memory copy is still available for the `PostCompact` response |

## Guarantees

1. **Snapshot atomicity**: The snapshot is created from a single consistent read of `WorkingMemory` + `VerificationState` during the `PreCompact` handler. No partial snapshots.

2. **Idempotent reinjection**: Multiple `PostCompact` events (e.g., Claude Code compacts twice in a row) produce the same snapshot text as long as the underlying state has not changed. The `sequence` counter lets the orchestrator detect and ignore stale snapshots.

3. **No data invention**: The snapshot never adds information that was not present in `WorkingMemory` before compaction. It is strictly a subset.

4. **Backward compatibility**: If no snapshot exists (older OCO version, hook failure, first compaction before any state), `PostCompact` returns an empty `HookResponse`. Claude Code continues normally with no reinjected context.

5. **Size bound**: The snapshot is bounded by the number of verified facts + hypotheses + plan steps + unverified files. In practice this is under 4 KB for typical sessions, well within the 64 KB hook body limit.

## Limitations

1. **Cannot restore full context**: The snapshot is a curated subset, not a context dump. Raw observations, conversation history, and tool outputs are permanently lost after compaction. Tasks requiring precise recall of earlier tool output must re-execute the tools.

2. **No cross-session continuity**: `CompactSnapshot` is scoped to a single OCO session. It does not support restoring state from a different session or a previous `oco run` invocation. Session persistence across process restarts is a separate concern (see `.oco/runs/<id>/`).

3. **LLM interpretation dependency**: The reinjected text block is consumed by Claude Code's LLM, not by a structured parser. There is no guarantee that the LLM will perfectly interpret every restored element. The structured text format is designed to maximize fidelity, but edge cases (unusual characters in fact text, very long plans) may degrade.

4. **Single compaction model**: The current design assumes Claude Code compacts the entire context at once. If future versions support partial/incremental compaction, the PreCompact/PostCompact hook pair may need revision.

5. **No snapshot versioning**: If the `CompactSnapshot` schema changes between OCO versions, old snapshots on disk may fail to deserialize. A `schema_version` field should be added before v1.0.

6. **Hook dependency**: The entire mechanism depends on Claude Code's HTTP hook infrastructure. If hooks are disabled (EnterpriseSafe mode) or the OCO server is unreachable, no snapshot is created and compaction proceeds with full state loss.

## Link with PolicyPack

The PolicyPack is the trust dial for post-compaction behavior. It does not affect snapshot creation (the snapshot is always complete), only how the orchestrator interprets the restored state.

### Interaction with VerificationTier

The `VerificationTier` (Light/Standard/Thorough) determined by `TierSelector` based on changed files interacts with the PolicyPack:

- **Strict pack + Thorough tier**: Maximum verification. All modified files are re-verified with build + test + lint + typecheck after compaction.
- **Fast pack + Light tier**: Minimum friction. Verification freshness from the snapshot is trusted; only docs/comments were changed.
- **Balanced pack + any tier**: Re-runs the active step and forces Stale freshness, but uses the tier-appropriate verification strategy when verification runs.

### Configuration

```toml
# oco.toml
[session]
# Policy pack for post-compaction trust (fast | balanced | strict)
compaction_policy = "balanced"
```

### Future: adaptive policy

A future enhancement could select the PolicyPack dynamically based on:
- Task complexity (Medium vs. Critical)
- Time since last verification
- Number of compactions in this session (progressive degradation)
- Whether security-sensitive files are in the unverified set

This is out of scope for the initial implementation.

## Consequences

### Positive

- **Session survival**: Medium+ tasks can survive multiple compactions without losing orchestration state, reducing wasted tokens and re-work.
- **Tunable trust**: The PolicyPack lets operators choose between speed and safety based on their risk tolerance.
- **Leverages existing infrastructure**: Builds on `WorkingMemory`, `VerificationState`, and the hook system already in place. No new crates or external dependencies.
- **Observable**: Snapshots are persisted to disk as JSON, enabling debugging and post-mortem analysis of compaction events.
- **Bounded cost**: Snapshot creation and reinjection are O(active entries), not O(conversation history). The overhead is negligible.

### Negative

- **New type surface**: `CompactSnapshot` and `PolicyPack` add to the type surface in `shared-types`. Tests required for serialization, restore logic, and policy application.
- **Hook reliability**: The mechanism fails silently if hooks are unavailable. Operators must run `oco doctor` to verify hook connectivity.
- **LLM fidelity risk**: Reinjected text depends on LLM interpretation. Subtle misreadings could cause the orchestrator to act on slightly incorrect restored state.

### Neutral

- **No breaking changes**: All new types are additive. Sessions that never compact (short tasks) are unaffected.
- **Serialization format**: JSON for both disk persistence and hook response. No new formats introduced.

## Alternatives Rejected

1. **Full context serialization**: Serialize the entire conversation to disk and reload after compaction. Rejected: too large (megabytes), too slow, and Claude Code does not support context replacement via hooks.

2. **No survival mechanism (status quo)**: Let compaction destroy all state. Rejected: unacceptable for Medium+ tasks that spend 50k+ tokens building context only to lose it.

3. **MCP resource-based restore**: Expose the snapshot as an MCP resource that the LLM can read via tool call. Rejected: adds a tool call round-trip post-compaction; the hook-based reinjection is faster and requires no LLM initiative.

4. **Binary checkpoint**: Serialize the full `WorkingMemory` + `VerificationState` as a binary blob. Rejected: not human-readable for debugging, and reinjection still requires text conversion for the LLM.

## References

- `WorkingMemory` and `compact_snapshot()`: `crates/shared-types/src/memory.rs`
- `VerificationState` and `VerificationFreshness`: `crates/shared-types/src/verification.rs`
- `VerificationTier` and `TierSelector`: `crates/shared-types/src/verification.rs`
- Hook handlers: `crates/mcp-server/src/hooks.rs`
- Claude Code hook events: `crates/claude-adapter/src/events.rs`
- ADR-005 Working Memory: `docs/adr/005-working-memory.md`
- ADR-007 Hooks Runtime Validation: `docs/adr/007-hooks-runtime-validation.md`
- ADR-008 Hierarchical Execution Plans: `docs/adr/008-hierarchical-execution-plans.md`
