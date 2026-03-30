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

Structured workflow with **live dashboard tracking**. Each phase emits rich events to the dashboard after completion. The dashboard plays them back with choreographed animations — always one step behind for the user to admire.

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

## Step 2: Classify & Route

Do your analysis, then emit the classification result:

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "flat_step_completed", "step": 0, "action_type": "classifying", "reason": "<complexity + reason>", "duration_ms": <actual_ms>, "budget_snapshot": {"tokens_used":0,"tokens_remaining":0,"tool_calls_used":0,"tool_calls_remaining":0,"retrievals_used":0,"verify_cycles_used":0,"elapsed_secs":0} }
] })
```

## Step 3: Plan — emit plan_exploration THEN plan_generated

**CRITICAL SEQUENCE**: For Medium+ tasks, emit these events **in this exact order** in a **single** `emit_events` call. The dashboard plays the PlanExplorer animation (~13s) from `plan_exploration`, then reveals the PlanMap from `plan_generated`.

```
oco.emit_events({ session_id: "<id>", events: [
  {
    "type": "plan_exploration",
    "candidates": [
      { "strategy": "speed", "step_count": 3, "estimated_tokens": 20000, "verify_count": 1, "parallel_groups": 1, "score": 0.55, "winner": false },
      { "strategy": "safety", "step_count": 7, "estimated_tokens": 50000, "verify_count": 3, "parallel_groups": 2, "score": 0.82, "winner": true }
    ],
    "winner_strategy": "safety",
    "winner_score": 0.82
  },
  {
    "type": "plan_generated",
    "plan_id": "00000000-0000-0000-0000-000000000001",
    "step_count": 7,
    "parallel_group_count": 2,
    "critical_path_length": 5,
    "estimated_total_tokens": 50000,
    "strategy": "Orchestrated",
    "team": null,
    "steps": [
      { "id": "00000000-0000-0000-0000-000000000010", "name": "Root config", "description": "...", "role": "implementer", "execution_mode": "inline", "depends_on": [], "verify_after": false, "estimated_tokens": 5000, "preferred_model": null },
      ...more steps...
    ]
  }
] })
```

**Build your candidates realistically**: the "speed" candidate should have fewer steps/verification, the "safety" candidate should have more steps with verification gates. Adjust step_count and scores based on actual task analysis.

## Step 4: Execute — emit step_started BEFORE, step_completed AFTER each step

For **each step** in the plan:

**Before starting work:**
```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "step_started", "step_id": "00000000-0000-0000-0000-000000000010", "step_name": "Root config", "role": "implementer", "execution_mode": "inline" }
] })
```

**Do the actual work** (Edit/Write/Bash tools).

**After completing the step:**
```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "step_completed", "step_id": "00000000-0000-0000-0000-000000000010", "step_name": "Root config", "success": true, "duration_ms": 4500, "tokens_used": 3200, "detail_ref": null },
  { "type": "progress", "completed": 1, "total": 7, "active_steps": [], "budget": {"tokens_used":3200,"tokens_remaining":46800,"tool_calls_used":8,"tool_calls_remaining":42,"retrievals_used":0,"verify_cycles_used":0,"elapsed_secs":12} }
] })
```

**Measure real durations** — track the time between step_started and step_completed calls.

## Step 5: Verify — emit verify_gate_result

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "verify_gate_result", "step_id": "00000000-0000-0000-0000-000000000070", "step_name": "Verify", "checks": [{"check_type":"build","passed":true,"summary":"Build succeeded"},{"check_type":"lint","passed":true,"summary":"No lint errors"}], "overall_passed": true, "replan_triggered": false }
] })
```

## Step 6: Complete

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "run_stopped", "reason": "task_complete", "total_steps": 7, "total_tokens": 50000 }
] })
```

## Rules

- **Always open dashboard first** — Step 1 is non-negotiable
- **Emit events AFTER each phase completes** — deferred, with real timing data
- **plan_exploration MUST come before plan_generated** — this triggers the PlanExplorer animation
- **Every step needs step_started + step_completed** — this populates the PlanMap
- **Use consistent step IDs** — UUIDs from plan_generated must match step_started/step_completed
- **Measure real durations** — `duration_ms` should reflect actual work time
- **Route to sub-skills** when intent matches — don't reinvent them
- **Verify after code changes** — non-negotiable
- **Max 3 retries** — after 3 failures, escalate to user
