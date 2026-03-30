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

## Step 1: Open Dashboard (MANDATORY)

```
oco.open_dashboard({ task: "<user's full request>", workspace: "<cwd>" })
```

Save the returned `session_id`. Then immediately send the run_started event:

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "run_started", "provider": "claude-code", "model": "opus", "request_summary": "<user's request>" }
] })
```

## Step 2: Classify & Route

Do your analysis, then emit the result:

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "flat_step_completed", "step": 0, "action_type": "classifying", "reason": "<complexity: high — N files>", "duration_ms": <actual_ms>, "budget_snapshot": {"tokens_used":0,"tokens_remaining":0,"tool_calls_used":0,"tool_calls_remaining":0,"retrievals_used":0,"verify_cycles_used":0,"elapsed_secs":0} }
] })
```

Route to sub-skills if appropriate, or proceed to Plan + Implement.

## Step 3: Plan — MUST emit plan_generated with steps

Build your plan, then emit it with a `plan_generated` event. **Use real UUIDs** for step IDs (format: `00000000-0000-0000-0000-00000000000N`).

```
oco.emit_events({ session_id: "<id>", events: [
  {
    "type": "plan_generated",
    "plan_id": "00000000-0000-0000-0000-000000000001",
    "step_count": 5,
    "parallel_group_count": 1,
    "critical_path_length": 5,
    "estimated_total_tokens": 25000,
    "strategy": "Orchestrated",
    "team": null,
    "steps": [
      { "id": "00000000-0000-0000-0000-000000000010", "name": "Root config", "description": "package.json, tsconfig, etc.", "role": "implementer", "execution_mode": "inline", "depends_on": [], "verify_after": false, "estimated_tokens": 5000, "preferred_model": null },
      { "id": "00000000-0000-0000-0000-000000000020", "name": "Shared schemas", "description": "Zod schemas", "role": "implementer", "execution_mode": "inline", "depends_on": [], "verify_after": false, "estimated_tokens": 5000, "preferred_model": null },
      { "id": "00000000-0000-0000-0000-000000000030", "name": "API backend", "description": "Fastify + JWT", "role": "implementer", "execution_mode": "inline", "depends_on": [], "verify_after": false, "estimated_tokens": 5000, "preferred_model": null },
      { "id": "00000000-0000-0000-0000-000000000040", "name": "Web frontend", "description": "Svelte 5", "role": "frontend-dev", "execution_mode": "inline", "depends_on": [], "verify_after": false, "estimated_tokens": 5000, "preferred_model": null },
      { "id": "00000000-0000-0000-0000-000000000050", "name": "Verify", "description": "Build + lint + test", "role": "verifier", "execution_mode": "inline", "depends_on": [], "verify_after": true, "estimated_tokens": 5000, "preferred_model": null }
    ]
  }
] })
```

This populates the PlanMap visualization with step nodes.

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
  { "type": "progress", "completed": 1, "total": 5, "active_steps": [], "budget": {"tokens_used":3200,"tokens_remaining":21800,"tool_calls_used":8,"tool_calls_remaining":42,"retrievals_used":0,"verify_cycles_used":0,"elapsed_secs":12} }
] })
```

**Measure real durations** — track the time between step_started and step_completed calls.

## Step 5: Verify — emit verify_gate_result

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "verify_gate_result", "step_id": "00000000-0000-0000-0000-000000000050", "step_name": "Verify", "checks": [{"check_type":"build","passed":true,"summary":"Build succeeded"},{"check_type":"lint","passed":true,"summary":"No lint errors"}], "overall_passed": true, "replan_triggered": false }
] })
```

## Step 6: Complete

```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "run_stopped", "reason": "task_complete", "total_steps": 5, "total_tokens": 15000 }
] })
```

On failure:
```
oco.emit_events({ session_id: "<id>", events: [
  { "type": "run_stopped", "reason": {"type":"error","message":"Build failed: ..."}, "total_steps": 3, "total_tokens": 8000 }
] })
```

## Rules

- **Always open dashboard first** — Step 1 is non-negotiable
- **Emit events AFTER each phase completes** (deferred, not during) — include real timing data
- **Every step needs step_started + step_completed** — this populates the PlanMap
- **Use consistent step IDs** — the UUID from plan_generated must match step_started/step_completed
- **Measure real durations** — `duration_ms` should reflect actual work time
- **Route to sub-skills** when intent matches — don't reinvent them
- **Verify after code changes** — non-negotiable
- **Max 3 retries** — after 3 failures, escalate to user
