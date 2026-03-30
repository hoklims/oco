# ADR-008: Hierarchical Execution Plans

## Status

Proposed

## Context

OCO's current `ExecutionPlan` is a flat DAG — a `Vec<PlanStep>` where each step has a single `StepExecution` mode (Inline, Subagent, Teammate, McpTool). This works well for simple orchestrations but fails to capture the reality of complex agent workflows:

1. **Claude Code Agent Teams** support Mesh topology where teammates operate as full Claude Code instances. A teammate doing complex work naturally decomposes it into sub-tasks — but OCO can't represent this.

2. **Subagents** perform isolated work. When a subagent's task is complex (e.g., "implement auth module"), it internally runs multiple actions (analyze, implement, test). OCO has no visibility into this sub-work.

3. **Visualization**: The dashboard needs to show "what's happening inside" a teammate or subagent step — expanding branches, sub-agent cards, progress within a step. Without hierarchical data, the dashboard can only fake it.

4. **Claude Code limitation** (as of March 2026): Teammates cannot spawn subagents (GitHub issue #31977 — Agent tool not in teammate toolkit). However, this is a documented gap, not a design choice. OCO should be ready when this is resolved.

### Current Architecture

```
ExecutionPlan
  └── steps: Vec<PlanStep>
        ├── PlanStep { execution: Inline, ... }
        ├── PlanStep { execution: Subagent { model }, ... }
        ├── PlanStep { execution: Teammate { team_name }, ... }
        └── PlanStep { execution: McpTool { server, tool }, ... }
```

Steps are peers. No step contains other steps. The GraphRunner executes them as a flat DAG with parallel groups.

## Decision

### Add `sub_plan: Option<ExecutionPlan>` to `PlanStep`

A step can optionally carry a nested execution plan representing the sub-work that will be performed within that step.

```rust
pub struct PlanStep {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub role: AgentRole,
    pub execution: StepExecution,
    pub depends_on: Vec<Uuid>,
    pub verify_after: bool,
    pub estimated_tokens: u32,
    pub preferred_model: Option<String>,
    pub effort: Option<EffortLevel>,
    // NEW
    pub sub_plan: Option<ExecutionPlan>,
}
```

### Semantics

- **Subagent + sub_plan**: The subagent will execute the sub_plan's steps in sequence/parallel within its isolated context. Each sub-step emits `SubStepProgress` events visible to the parent.

- **Teammate + sub_plan**: The teammate uses the sub_plan as its work decomposition. In Mesh topology, the teammate may delegate sub-steps to its own subagents (when Claude Code supports it) or execute them inline.

- **Inline + sub_plan**: The main orchestrator executes the sub_plan recursively via a nested `GraphRunner`. This is the simplest case — no agent spawning, just structured decomposition.

- **No sub_plan** (default): Behavior unchanged. Step executes as a single atomic action.

### Execution Model

```
GraphRunner.execute(plan):
  for each ready step:
    if step.sub_plan is Some:
      emit SubPlanStarted { parent_step_id, sub_step_count }
      let child_runner = GraphRunner::new(sub_plan, child_budget)
      child_runner.execute()  // recursive
        → emits SubStepProgress for each sub-step
      emit SubPlanCompleted { parent_step_id, success }
    else:
      execute_step(step)  // existing behavior
```

### Constraints

1. **Max depth: 2 levels** (configurable in `oco.toml`). Root plan → sub-plan. No sub-sub-plans. This prevents token explosion and matches Claude Code's current 2-level hierarchy (lead → teammates/subagents).

2. **Budget pre-reservation**: Before executing a sub-plan, the parent GraphRunner reserves estimated tokens from its budget. If insufficient, the step fails without starting the sub-plan.

3. **Cancellation propagation**: Parent's `CancellationToken` is cloned to child `GraphRunner`. Cancelling the parent cancels all sub-plans.

4. **Replan semantics**: Sub-plan can replan internally (up to `max_replan_attempts`). If all retries fail, the parent step fails and parent-level replan is triggered.

5. **Context flow**: Sub-steps receive the parent step's context (role instructions, dependency outputs) via `StepContextBuilder::with_parent_context()`.

### New Event Types

```rust
// Emitted when a step's sub-plan starts executing
SubPlanStarted {
    parent_step_id: Uuid,
    sub_step_count: usize,
    sub_steps: Vec<SubStepSummary>,
}

// Emitted for each sub-step state change
SubStepProgress {
    parent_step_id: Uuid,
    sub_step_id: String,
    sub_step_name: String,
    status: StepStatus,  // Pending, Running, Passed, Failed
}

// Emitted when the sub-plan completes
SubPlanCompleted {
    parent_step_id: Uuid,
    success: bool,
    duration_ms: u64,
    tokens_used: u32,
}

// Emitted when teammates exchange messages (Mesh topology)
TeammateMessageSent {
    from_step_id: Uuid,
    to_step_id: Uuid,
    from_name: String,
    to_name: String,
    summary: String,
}
```

## Consequences

### Positive

- **Accurate modeling**: Plans reflect actual execution hierarchy, not just task dependencies
- **Dashboard visualization**: Sub-plan data enables expanding branches, sub-agent cards, progress tracking within steps
- **Future-proof**: Ready for Claude Code teammate→subagent support when issue #31977 is resolved
- **Backward compatible**: `sub_plan: None` is the default — all existing plans work unchanged
- **Better observability**: Parent-child event chain enables tracing at any depth

### Negative

- **Complexity in GraphRunner**: Recursive execution adds error handling complexity (deadlock prevention, budget accounting across levels)
- **Planner complexity**: LlmPlanner must decide when to generate sub-plans vs. keeping steps flat
- **Testing surface**: Need toxic scenarios (nested timeout, circular refs, budget overflow at depth)
- **Token cost**: Sub-plans increase prompt size for the planner LLM call

### Neutral

- **Serialization**: `ExecutionPlan` is already `Serialize/Deserialize`. Adding `Option<ExecutionPlan>` to `PlanStep` is recursive but bounded by depth limit.
- **Dashboard**: Sub-plan visualization is additive — the flat DAG view still works, sub-plans add optional expansion.

## Alternatives Rejected

1. **Fully flat plans (status quo)**: Can't represent what agents actually do inside a step. Dashboard must fake sub-activities with pattern matching. No visibility into teammate work decomposition.

2. **Unlimited nesting depth**: Exponential complexity, impossible to visualize meaningfully, token explosion. Bounded to 2 levels covers 99% of real-world cases.

3. **Separate sub-plan storage** (sub-plans in a side-table, referenced by UUID): Over-engineered. The plan is already a tree — making it explicit with `Option<ExecutionPlan>` is simpler and keeps all data co-located.

4. **Runtime-only decomposition** (agents decompose at execution time, not planning time): Loses the benefit of upfront planning, budget estimation, and visualization before execution starts.

## References

- Claude Code Agent Teams: `code.claude.com/docs/en/agent-teams`
- Claude Code issue #31977: Teammates lack Agent tool (subagent spawning)
- OCO GraphRunner: `crates/orchestrator-core/src/graph_runner.rs`
- OCO ExecutionPlan: `crates/shared-types/src/plan.rs`
