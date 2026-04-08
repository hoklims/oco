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

## Step 3: Plan — emit plan_exploration THEN plan_generated

**CRITICAL**: Emit both events in a **single** `emit_events` call. The dashboard plays the PlanExplorer animation from `plan_exploration`, then reveals the PlanMap from `plan_generated`.

**Before emitting**: decide your real execution steps. Give each a stable UUID (use crypto.randomUUID() format). These are the steps you WILL execute in Step 4.

**FORBIDDEN step names** (these are from the demo and MUST NOT appear in real runs):
- "Search OSS solutions", "Search research papers", "Synthesize findings"
- "Design JWT schema", "Implement middleware", "Implement refresh"
- "Integration tests", "Analyze & design", "Quick smoke test"
- Any JWT/auth-related name unless the user's actual task is about JWT/auth

```
oco.emit_events({ session_id: "<id>", events: [
  {
    "type": "plan_exploration",
    "candidates": [
      { "strategy": "minimal", "step_count": <N>, "estimated_tokens": <T>, "verify_count": <V>, "parallel_groups": <P>, "score": <0.0-1.0>, "winner": false },
      { "strategy": "thorough", "step_count": <N>, "estimated_tokens": <T>, "verify_count": <V>, "parallel_groups": <P>, "score": <0.0-1.0>, "winner": true }
    ],
    "winner_strategy": "thorough",
    "winner_score": <score>
  },
  {
    "type": "plan_generated",
    "plan_id": "<random-uuid>",
    "step_count": <N>,
    "parallel_group_count": <P>,
    "critical_path_length": <C>,
    "estimated_total_tokens": <T>,
    "strategy": "Orchestrated",
    "team": null,
    "steps": [
      {
        "id": "<random-uuid-for-this-step>",
        "name": "<YOUR real step — e.g. 'Add validation to UserService.create'>",
        "description": "<what this step accomplishes>",
        "role": "implementer",
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

**Build candidates realistically**: "minimal" should have fewer steps/verification, "thorough" should have more. The winner's steps become your `plan_generated.steps`. All step names must be specific to the user's actual task.

## Step 4: Execute — emit step_started BEFORE, step_completed AFTER each step

For **each step** in the plan, using the **exact same ID and name** from `plan_generated`:

**Before starting work:**
```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "step_started", "step_id": "<step-uuid-1>", "step_name": "<SAME name as plan>", "role": "implementer", "execution_mode": "inline" }
] })
```

**Do the actual work** (Edit/Write/Bash tools).

**After completing the step:**
```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "step_completed", "step_id": "<step-uuid-1>", "step_name": "<SAME name as plan>", "success": true, "duration_ms": <actual_ms>, "tokens_used": <estimated>, "detail_ref": null },
  { "type": "progress", "completed": <N>, "total": <total_steps>, "active_steps": [], "budget": {"tokens_used":<N>,"tokens_remaining":<N>,"tool_calls_used":<N>,"tool_calls_remaining":<N>,"retrievals_used":0,"verify_cycles_used":<N>,"elapsed_secs":<N>} }
] })
```

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
- **Emit events AFTER each phase completes** — with real timing data
- **plan_exploration + plan_generated in one call** — triggers the PlanExplorer → PlanMap animation sequence
- **Measure real durations** — `duration_ms` should reflect actual work time
- **Route to sub-skills** when intent matches — don't reinvent them
- **Verify after code changes** — non-negotiable
- **Max 3 retries** — after 3 failures, escalate to user
