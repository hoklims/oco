# Emergent Orchestration UX — Implementation Plan

**Origin**: External review of Orchestration v2 architecture.
**Goal**: Make emergent orchestration observable, parsimonious, and measurably better than fixed workflows.

---

## Diagnosis: Current State vs Review Expectations

### What exists and works well
- Deterministic complexity classifier (no LLM call)
- Budget tiers per complexity (Trivial→Critical with hard caps)
- Planning token budget capped at 5% per call
- Replan limited to 3 attempts, preserving completed steps
- DAG validation (cycles, dangling deps, duplicates)
- No-progress deadlock guard in GraphRunner
- Step count guidance in planner prompt (2-5/3-8/5-12)
- Event-driven CLI with Terminal/JSONL/Quiet renderers
- Trace persistence (trace.jsonl + summary.json) with replay

### What's missing (from review + codebase analysis)
1. **No plan visualization** — users see step-by-step events but never the DAG structure
2. **No execution dashboard** — no "X of Y steps done", no parallel batch visibility
3. **Verification output opaque** — gate failures logged but full output not surfaced
4. **No audit trail** — no cross-cutting log of why decisions were made
5. **Team coordination invisible** — TeamStatus event exists but GraphRunner doesn't emit it
6. **Replan transitions invisible** — old plan vs new plan not diffable
7. **No hard step count enforcement** at execution time (only prompt guidance)
8. **No plan cost estimation** shown to user before execution
9. **No human control layer** — can't impose constraints (folder exclusions, model restrictions, budget overrides)
10. **Mesh topology too easy to trigger** — should be last resort, not default for interdependent parallels

---

## Implementation Plan

### Phase 1: Observable Plans (the biggest gap)

The review's core point: "emergent has value only if it's readable, inspectable, diffable, reproducible." Everything here makes the plan visible.

#### 1.1 — Plan Summary Event (`OrchestrationEvent::PlanGenerated`)

**Files**: `crates/shared-types/src/telemetry.rs`, `crates/orchestrator-core/src/graph_runner.rs`

Currently TODO #15 in graph_runner.rs — `PlanGenerated` events are hacked as synthetic `StepCompleted`. Fix:

- Add `OrchestrationEvent::PlanGenerated` variant with:
  - `plan_id: Uuid`
  - `step_count: usize`
  - `parallel_group_count: usize`
  - `critical_path_length: usize`
  - `estimated_total_tokens: u64`
  - `team_config: Option<TeamSummary>` (name, topology, member count)
  - `steps_summary: Vec<StepSummary>` (id, objective, role, execution_mode, deps)
- Emit from GraphRunner at plan start and after each replan
- Remove synthetic StepCompleted hack

#### 1.2 — Plan DAG View in Terminal Renderer

**Files**: `apps/dev-cli/src/ui/event.rs`, `apps/dev-cli/src/ui/terminal.rs`

Add `UiEvent::PlanOverview` and render as ASCII DAG or indented tree:

```
Plan: 6 steps, 3 parallel groups, critical path: 4
  [1] Inspect auth entrypoints          (sonnet, inline)
  [2] Map session data flow             (sonnet, inline)  ← depends: 1
  [3a] Inspect backend middleware        (sonnet, inline)  ← depends: 2
  [3b] Inspect frontend consumers        (haiku, inline)   ← depends: 2
  [4] Synthesize + patch                (opus, inline)    ← depends: 3a, 3b
  [5] Verify gate: typecheck, tests     (—, verify)       ← depends: 4
Budget: ~120k tokens est. / 1M available
```

#### 1.3 — Live Execution Progress

**Files**: `crates/orchestrator-core/src/graph_runner.rs`, `apps/dev-cli/src/ui/terminal.rs`

Add `OrchestrationEvent::PlanProgress` variant:
- `completed: usize`
- `total: usize`
- `active_steps: Vec<(Uuid, String)>` (id + objective)
- `budget_used_pct: f32`

Emit after each step completion. Terminal renderer shows:

```
[3/6] ██████░░░░ 50%  Active: [3a] backend middleware, [3b] frontend consumers  Budget: 34%
```

#### 1.4 — Verification Gate Output

**Files**: `crates/orchestrator-core/src/graph_runner.rs`, `crates/shared-types/src/telemetry.rs`

Add `OrchestrationEvent::VerifyGateResult` variant:
- `step_id: Uuid`
- `step_name: String`
- `checks: Vec<CheckResult>` (check_type, passed, summary, truncated_output)
- `overall_passed: bool`
- `replan_triggered: bool`

Surface in terminal as:

```
Verify [5]: typecheck ✓  unit_tests ✗ (2 failures in auth_test.rs)
  → Replan triggered (attempt 1/3)
```

#### 1.5 — Replan Diff

**Files**: `crates/orchestrator-core/src/graph_runner.rs`, `apps/dev-cli/src/ui/terminal.rs`

When replanning occurs, emit event showing:
- Steps preserved (Completed/InProgress)
- Steps removed (Failed → Replanned)
- Steps added (new plan)
- Changed hypothesis (what the planner decided to do differently)

```
Replan (attempt 1/3):
  Kept:    [1] inspect, [2] map, [3a] backend, [3b] frontend
  Removed: [4] synthesize (failed: type mismatch in session.rs)
  Added:   [4'] fix session type, [5'] re-synthesize
  Gate:    [6'] verify (same checks)
```

---

### Phase 2: Anti-Bloat Guards (the "theater of planning" risk)

The review warns about planners that "sur-décompose tout" — producing beautiful graphs where nothing gets done.

#### 2.1 — Hard Step Count Enforcement

**Files**: `crates/planner/src/llm_planner.rs`, `crates/shared-types/src/plan.rs`

Currently only prompt-guided. Add hard validation:

```rust
impl ExecutionPlan {
    pub fn max_steps_for_complexity(complexity: TaskComplexity) -> usize {
        match complexity {
            Trivial => 1,
            Low => 3,
            Medium => 7,
            High => 10,
            Critical => 15,
        }
    }
}
```

In `LlmPlanner::plan()`, after parsing: if step count exceeds max, either:
1. Reject and re-prompt with "collapse steps" instruction
2. Auto-collapse consecutive read-only steps into one

#### 2.2 — Planning Overhead Metric

**Files**: `crates/shared-types/src/telemetry.rs`, `apps/dev-cli/src/ui/terminal.rs`

Track and display:
- `planning_tokens_used` vs `execution_tokens_used`
- Ratio displayed in summary: `Planning overhead: 8% (12k/150k tokens)`
- Warn if planning overhead exceeds 20%

#### 2.3 — Step Collapse Heuristic

**Files**: `crates/planner/src/llm_planner.rs`

Post-generation pass that merges steps when:
- Two consecutive steps have same agent role + same execution mode
- Both are read-only (no verify_after)
- Both target same file/directory scope
- Combined estimated tokens < single step budget

Example collapse: "inspect auth middleware" + "inspect auth config" → "inspect auth middleware and config"

#### 2.4 — Fast-Exit for Low-Uncertainty Plans

**Files**: `crates/planner/src/llm_planner.rs`, `crates/orchestrator-core/src/graph_runner.rs`

If the planner generates a plan where:
- All steps are Inline (no subagents/teams)
- No parallel groups (linear chain)
- Step count ≤ 3
- No verify gates

Then skip GraphRunner overhead entirely and execute via flat loop_runner. The plan was "emergent" but converged to something simple — honor that.

---

### Phase 3: Topology Discipline (Mesh as last resort)

The review says: "pipeline first, hub/spoke next, mesh only for adversarial review."

#### 3.1 — Topology Selection Tightening

**Files**: `crates/planner/src/llm_planner.rs`

Current heuristic: Mesh if "interdependent parallel steps". Tighten to:

1. **No team** — default for ≤5 steps or no parallelism
2. **Pipeline** — if steps form a clear sequential chain with handoff
3. **HubSpoke** — if parallel steps are independent (fan-out/fan-in)
4. **Mesh** — ONLY if explicitly requested by user constraint OR task involves adversarial review (e.g., "review this PR from security and performance angles")

Add `TopologyJustification` field to plan output explaining why the topology was chosen.

#### 3.2 — Mesh Cost Warning

**Files**: `apps/dev-cli/src/ui/terminal.rs`

If Mesh topology selected, emit warning with estimated overhead:

```
⚠ Mesh topology selected (3 agents, ~2x token cost vs HubSpoke)
  Reason: adversarial review requested
```

---

### Phase 4: Surgical Replanning

The review: "a good replan must resume locally, preserve validated work, change the faulty hypothesis."

#### 4.1 — Replan Context Enrichment

**Files**: `crates/planner/src/llm_planner.rs`, `crates/planner/src/prompt.rs`

Current replan prompt says "does NOT repeat completed steps" but doesn't explain WHY the step failed. Enrich:

- Include failing step's full output (truncated to 2k tokens)
- Include verification output that triggered the replan
- Include which hypothesis was invalidated
- Ask planner to explicitly state what changed in approach

#### 4.2 — Replan Budget Pre-Check

**Files**: `crates/orchestrator-core/src/graph_runner.rs`

Before triggering replan, check:
- Remaining budget ≥ 15% of original (enough for replan call + new steps)
- If not, skip replan and return with partial results + explanation

Currently replan cost (5%/call × 3 max) is not pre-checked against remaining budget.

#### 4.3 — Replan Metrics in Summary

**Files**: `apps/dev-cli/src/main.rs` (save_run_artifacts)

Add to summary.json:
- `replan_count: u32`
- `replan_token_cost: u64`
- `steps_preserved: usize`
- `steps_replaced: usize`

---

### Phase 5: Human Control Layer

The review's "Mode 3: Human Control" — users impose constraints.

#### 5.1 — Plan Constraints in oco.toml

**Files**: `crates/shared-types/src/lib.rs` (config), `crates/planner/src/context.rs`

New config section:

```toml
[orchestration.constraints]
max_steps = 8
excluded_paths = ["vendor/", "generated/"]
required_verify_checks = ["typecheck", "test"]
allowed_models = ["sonnet", "haiku"]  # no opus
max_parallel_steps = 3
no_mesh = true
```

Planner receives these constraints and respects them during generation. GraphRunner enforces at execution time.

#### 5.2 — Interactive Plan Approval (opt-in)

**Files**: `apps/dev-cli/src/main.rs`, `apps/dev-cli/src/ui/terminal.rs`

New flag: `oco run --approve-plan "request"`

Flow:
1. Classifier runs → Medium+ → Planner generates plan
2. Plan displayed (using 1.2 DAG view)
3. User prompted: `[A]pprove / [E]dit constraints / [R]eject`
4. On approve → GraphRunner executes
5. On edit → re-prompt with user constraints
6. On reject → abort

#### 5.3 — Per-Run Budget Override

**Files**: `apps/dev-cli/src/main.rs`

`oco run --max-tokens 50000 --max-tools 10 "request"`

Overrides both configured and complexity-tier budgets. Simple but critical for control.

---

### Phase 6: Audit Trail (the "why" layer)

#### 6.1 — Decision Log in Trace

**Files**: `crates/orchestrator-core/src/graph_runner.rs`, `crates/shared-types/src/telemetry.rs`

New `OrchestrationEvent::Decision` variant for key choices:
- Why this complexity was assigned (keywords matched)
- Why this plan was generated (planner reasoning summary)
- Why this model was routed to this step (LlmRouter logic)
- Why this topology was chosen
- Why replan was triggered (verification failure details)

Each decision has: `decision_type`, `chosen`, `alternatives_considered`, `reason`.

#### 6.2 — Enriched summary.json

**Files**: `apps/dev-cli/src/main.rs`

Add to run summary:
- `plan_structure`: step list with deps, roles, execution modes
- `execution_timeline`: ordered list of (step_id, start_ms, end_ms, tokens_used)
- `decisions`: list of key decisions with reasons
- `verification_results`: per-gate results
- `planning_overhead_pct`: planning tokens / total tokens

#### 6.3 — Trace Diffing Tool

**Files**: `apps/dev-cli/src/main.rs`

New command: `oco runs diff <id1> <id2>`

Compares two runs on same/similar request:
- Plan structure diff (steps added/removed/changed)
- Token usage comparison
- Verification outcomes
- Time comparison

Useful for measuring whether emergent planning is actually better than fixed workflows.

---

## Priority Order

| Priority | Phase | Impact | Effort | Why first |
|----------|-------|--------|--------|-----------|
| **P0** | 1.1–1.3 | High | Medium | Without visible plans, nothing else matters |
| **P0** | 2.1 | High | Low | Hard step limit is a 20-line change with outsized safety value |
| **P1** | 1.4–1.5 | High | Medium | Verify + replan visibility complete the core loop |
| **P1** | 4.1–4.2 | High | Low | Surgical replanning is cheap to improve |
| **P1** | 5.3 | Medium | Low | Per-run budget override is trivial and immediately useful |
| **P2** | 2.2–2.4 | Medium | Medium | Anti-bloat metrics and heuristics |
| **P2** | 3.1–3.2 | Medium | Low | Topology tightening prevents waste |
| **P2** | 5.1 | Medium | Medium | Config-based constraints |
| **P3** | 6.1–6.2 | Medium | Medium | Audit trail is important but not blocking |
| **P3** | 5.2 | Low | Medium | Interactive approval is nice-to-have |
| **P3** | 6.3 | Low | High | Trace diffing is a power-user tool |

---

## Success Criteria (from review)

The review's verdict: success depends on making emergence **observable, parsimonious, and profitable**.

Measurable criteria:
1. **Observable**: Any `oco run` on Medium+ task shows plan DAG, progress, verify results, and replan diffs in terminal
2. **Parsimonious**: Planning overhead stays <15% of total tokens; no plan exceeds complexity-tier step limit
3. **Profitable**: On eval scenarios, emergent plans complete tasks with fewer total tokens and higher verification pass rate than flat loop (measure via `oco eval`)
4. **Inspectable**: `oco runs show <id>` reconstructs full decision trail from trace.jsonl
5. **Controllable**: Users can cap steps, exclude paths, restrict models, and approve plans before execution
