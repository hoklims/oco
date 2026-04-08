---
name: oco
description: >
  Orchestrated coding workflow — the unified entry point for all OCO capabilities.
  Use when the user types /oco followed by any request, or when a task would benefit
  from structured orchestration combining code intelligence, impact analysis, verification,
  and multi-tool coordination.
triggers:
  - "oco"
  - "orchestrate"
  - "full workflow"
  - "smart mode"
  - "do it properly"
---

# OCO: Orchestrated Coding Workflow

Structured workflow with **live dashboard tracking**. Each phase emits rich events to the dashboard after completion.

## Critical Rule: Plan ↔ Execution Consistency

The dashboard renders a DAG of nodes from `plan_generated`. Each node tracks status via `step_started` / `step_completed` events. **If step IDs or names don't match between plan and execution, the nodes won't animate.** Therefore:

1. **First, analyze the task and decide your real steps** (what you will actually do)
2. **Then emit `plan_generated` with those exact steps** (names, IDs, count)
3. **Then execute each step using the same IDs and names**

**NEVER copy example step names.** Names like "Search OSS solutions", "Design JWT schema", "Implement middleware", "Integration tests" are DEMO PLACEHOLDERS — do NOT use them. Every step name must describe YOUR actual task. For example, if the user asks to add auth, your steps might be "Analyze existing auth code", "Add JWT middleware to routes", etc. — specific to the codebase.

## Step 1: Open Dashboard (MANDATORY)

```
oco.open_dashboard({ task: "<user's full request>", workspace: "<cwd>" })
```

Save the returned `session_id`. Then emit run_started:

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "run_started", "provider": "claude-code", "model": "opus", "request_summary": "<user's request>" }
] })
```

## Step 2: Classify

Analyze the task complexity and category. Then emit:

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "flat_step_completed", "step": 0, "action_type": "classifying", "reason": "<complexity — brief rationale>", "duration_ms": <actual_ms>, "budget_snapshot": {"tokens_used":0,"tokens_remaining":0,"tool_calls_used":0,"tool_calls_remaining":0,"retrievals_used":0,"verify_cycles_used":0,"elapsed_secs":0} }
] })
```

## Step 3: Plan — genuinely explore TWO strategies, pick the best

You MUST design **two genuinely different plans** for the task, compare them, and pick the winner. This is NOT cosmetic — each plan must be a real strategy you could execute.

**FORBIDDEN step names** (demo placeholders — NEVER use):
- "Search OSS solutions", "Search research papers", "Synthesize findings"
- "Design JWT schema", "Implement middleware", "Implement refresh"
- "Integration tests", "Analyze & design", "Quick smoke test"

### 3a. Design two real plans

Think through two approaches with **structural differences**, not just different names:

| Dimension | Plan A | Plan B |
|-----------|--------|--------|
| Granularity | Fewer large steps | More focused steps |
| Verification | Verify at end only | Verify after risky steps |
| Parallelism | Sequential chain | DAG with independent branches |
| Scope | Strict minimum | Include edge cases, validation |

Give each plan a short strategy name that describes the approach (e.g. "monolith", "modular", "incremental", "full-coverage"). Do NOT use "speed"/"safety"/"minimal"/"thorough" — be specific to the task.

For each plan, compute:
- `step_count` — number of steps
- `estimated_tokens` — total estimated token cost
- `verify_count` — number of steps with `verify_after: true`
- `parallel_groups` — number of parallelizable batches in the DAG

### 3b. Score and pick the winner

Score each plan 0.0–1.0 based on:
- **Risk coverage** — does it verify after the riskiest steps?
- **Cost efficiency** — tokens spent vs value delivered
- **Parallelism** — can steps run concurrently?
- **Completeness** — does it cover the full task?

The winner is the plan with the higher score. **The minimal plan CAN win** — for simple tasks, fewer steps with targeted verification is genuinely better than over-engineering.

### 3c. Build the winning plan's steps

For the winner, create full step definitions with UUIDs, real names, depends_on DAG, roles, and execution modes. These are the steps you WILL execute in Step 4.

### 3d. Emit both events in a SINGLE call

```
oco.emit_events({ session_id: "<id>", events: [
  {
    "type": "plan_exploration",
    "candidates": [
      { "strategy": "<loser-strategy-name>", "step_count": <N>, "estimated_tokens": <T>, "verify_count": <V>, "parallel_groups": <P>, "score": <0.0-1.0>, "winner": false, "step_names": ["<loser step 1 name>", "<loser step 2 name>", "..."] },
      { "strategy": "<winner-strategy-name>", "step_count": <N>, "estimated_tokens": <T>, "verify_count": <V>, "parallel_groups": <P>, "score": <0.0-1.0>, "winner": true, "step_names": ["<winner step 1 name>", "<winner step 2 name>", "..."] }
    ],
    "winner_strategy": "<winner-strategy-name>",
    "winner_score": <score>
  },
  {
    "type": "plan_generated",
    "plan_id": "<random-uuid>",
    "step_count": <N>,
    "parallel_group_count": <P>,
    "critical_path_length": <C>,
    "estimated_total_tokens": <T>,
    "strategy": "Competitive (<winner-strategy-name> won)",
    "team": null,
    "steps": [
      {
        "id": "<random-uuid-for-this-step>",
        "name": "<task-specific step name>",
        "description": "<what this step accomplishes>",
        "role": "implementer|verifier|investigator",
        "execution_mode": "inline",
        "depends_on": [],
        "verify_after": false,
        "estimated_tokens": <T>,
        "preferred_model": null
      }
    ]
  }
] })
```

## Step 4: Execute — PARALLEL when possible, SEQUENTIAL when dependent

Walk the plan DAG in topological order. At each level, find all steps whose `depends_on` are ALL completed. Execute those steps **in parallel** using the Agent tool.

### 4a. Detect parallel groups

From `plan_generated.steps`, compute ready steps: steps whose `depends_on` are all completed (or empty). If multiple steps are ready simultaneously, they form a **parallel group**.

Example DAG with depends_on:
```
Step A (depends: [])      ← ready immediately
Step B (depends: [])      ← ready immediately  → parallel group [A, B]
Step C (depends: [A])     ← ready after A
Step D (depends: [A, B])  ← ready after A AND B
```

### 4b. Execute a parallel group (2+ ready steps)

**Emit step_started for ALL parallel steps at once:**
```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "step_started", "step_id": "<id-A>", "step_name": "<name-A>", "role": "implementer", "execution_mode": "subagent" },
  { "type": "step_started", "step_id": "<id-B>", "step_name": "<name-B>", "role": "implementer", "execution_mode": "subagent" }
] })
```

**Spawn one Agent per step in a SINGLE message** (this makes them run concurrently):
```
Agent({ description: "Step A: <name>", prompt: "<full context + instructions for step A>" })
Agent({ description: "Step B: <name>", prompt: "<full context + instructions for step B>" })
```

Each agent prompt MUST include:
- The workspace path
- What files to create/modify
- Enough context about the codebase (types, conventions) to work independently
- The acceptance criteria for the step

**After ALL agents return**, emit step_completed for each:
```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "step_completed", "step_id": "<id-A>", "step_name": "<name-A>", "success": true, "duration_ms": <ms>, "tokens_used": <est>, "detail_ref": null },
  { "type": "step_completed", "step_id": "<id-B>", "step_name": "<name-B>", "success": true, "duration_ms": <ms>, "tokens_used": <est>, "detail_ref": null },
  { "type": "progress", "completed": <N>, "total": <total_steps>, "active_steps": [], "budget": {"tokens_used":<N>,"tokens_remaining":<N>,"tool_calls_used":<N>,"tool_calls_remaining":<N>,"retrievals_used":0,"verify_cycles_used":<N>,"elapsed_secs":<N>} }
] })
```

### 4c. Execute a single step (1 ready step, or step is simple)

For sequential steps or when only 1 step is ready:

**Before starting work:**
```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "step_started", "step_id": "<step-uuid>", "step_name": "<name>", "role": "implementer", "execution_mode": "inline" }
] })
```

**Do the actual work** directly (Edit/Write/Bash tools).

**After completing:**
```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "step_completed", "step_id": "<step-uuid>", "step_name": "<name>", "success": true, "duration_ms": <actual_ms>, "tokens_used": <estimated>, "detail_ref": null },
  { "type": "progress", "completed": <N>, "total": <total_steps>, "active_steps": [], "budget": {"tokens_used":<N>,"tokens_remaining":<N>,"tool_calls_used":<N>,"tool_calls_remaining":<N>,"retrievals_used":0,"verify_cycles_used":<N>,"elapsed_secs":<N>} }
] })
```

### 4d. When to parallelize vs not

**DO parallelize** when:
- 2+ steps have all dependencies met simultaneously
- Steps create/modify DIFFERENT files (no file conflicts)
- Steps are substantial enough to justify agent spawn overhead

**Do NOT parallelize** when:
- Steps edit the SAME file
- One step needs the output of another (even if not in depends_on)
- Steps are trivial (< 20 lines of code) — just do them inline sequentially

## Step 5: Verify — emit verify_gate_result

After running verification (build, test, lint):

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "verify_gate_result", "step_id": "<verify-step-uuid>", "step_name": "<verify step name>", "checks": [{"check_type":"build","passed":true,"summary":"..."},{"check_type":"test","passed":true,"summary":"..."}], "overall_passed": true, "replan_triggered": false }
] })
```

## Step 6: Complete

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "run_stopped", "reason": "task_complete", "total_steps": <N>, "total_tokens": <N> }
] })
```

## Rules

- **Always open dashboard first** — Step 1 is non-negotiable
- **Plan = execution contract** — `plan_generated.steps` must list the steps you will actually execute, with the exact IDs and names you will use in `step_started`/`step_completed`. If they don't match, the DAG nodes won't animate.
- **Parallelize independent steps** — When 2+ steps have all dependencies met, execute them concurrently via Agent tool. Emit all step_started events together, spawn agents in one message, emit step_completed after all return.
- **Emit events AFTER each phase completes** — with real timing data
- **plan_exploration + plan_generated in one call** — triggers the PlanExplorer → PlanMap animation sequence
- **Measure real durations** — `duration_ms` should reflect actual work time
- **Route to sub-skills** when intent matches — don't reinvent them
- **Verify after code changes** — non-negotiable
- **Max 3 retries** — after 3 failures, escalate to user
