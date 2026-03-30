---
name: oco
description: >
  Orchestrated coding workflow — the unified entry point for all OCO capabilities.
  Use when the user types /oco followed by any request, or when a task would benefit
  from structured orchestration combining code intelligence, impact analysis, verification,
  and multi-tool coordination. Routes to the right sub-skill based on intent, enriches
  context with repo profile and available capabilities, and ensures verification after
  any code change. Use when: "oco", "orchestrate", "full workflow", "smart mode",
  "use all tools", "do it properly", or any complex multi-step coding task.
triggers:
  - "oco"
  - "orchestrate"
  - "full workflow"
  - "smart mode"
  - "do it properly"
---

# OCO: Orchestrated Coding Workflow

You are entering OCO's orchestrated mode — a structured workflow with **live dashboard tracking**.

## Step 1: Open Dashboard (MANDATORY FIRST STEP)

**Before anything else**, open the live dashboard so the user sees progress from the start:

```
oco.open_dashboard({ task: "<user's request>", workspace: "<cwd>" })
```

This returns a `session_id`. **Save it** — you will use it to push phase updates throughout.

Then immediately push the first phase:
```
oco.emit_phase({ session_id: "<id>", phase: "run_started", detail: "<user's request>" })
```

## Step 2: Boot & Discover

Gather context about the workspace:

**If yoyo MCP tools are available** (`boot`, `index`):
```
yoyo.boot() — discover workspace structure
yoyo.index() — ensure AST index is fresh
```

**Always**:
- Detect project type from manifests (Cargo.toml, package.json, pyproject.toml, go.mod)
- Note the language, build system, and test framework
- Check recent git activity: `git log --oneline -5`

Produce a one-line context summary:
```
[workspace: <name> | lang: <lang> | tools: yoyo+oco / oco-only / basic]
```

## Step 3: Classify & Route

Push the classifying phase:
```
oco.emit_phase({ session_id: "<id>", phase: "classifying", detail: "<detected complexity>" })
```

Analyze the user's request and route:

| Intent Signal | Route To |
|---------------|----------|
| explore, understand, architecture | `/oco-inspect-repo-area` |
| bug, broken, regression (no stacktrace) | `/oco-investigate-bug` |
| stacktrace, panic, exception, crash | `/oco-trace-stack` |
| refactor, rename, move, extract, split | `/oco-safe-refactor` |
| verify, test, check, validate | `/oco-verify-fix` |
| **new feature, implement, add, create** | **Plan + Implement** (Step 4) |
| **complex / multi-step** | **Decompose** (Step 4) |

**If routing to a sub-skill**, still push phase updates via `oco.emit_phase`.

## Step 4: Plan + Implement

### 4a. Plan — MUST include steps array

**CRITICAL**: Pass a `steps` array so the PlanMap visualization populates.
Each step needs a `name` and `description`. The bridge generates UUIDs and wires them into `plan_generated`.

```
oco.emit_phase({
  session_id: "<id>",
  phase: "planning",
  detail: "7 steps — 30+ files to create",
  steps: [
    { name: "Root config", description: "package.json, turbo.json, tsconfig, .gitignore" },
    { name: "Shared schemas", description: "packages/shared — Zod schemas + types" },
    { name: "API backend", description: "apps/api — Fastify + JWT + Drizzle" },
    { name: "Web frontend", description: "apps/web — Svelte 5 + TanStack Query" },
    { name: "Docker Compose", description: "PostgreSQL dev environment" },
    { name: "CI pipeline", description: "GitHub Actions — build, lint, test, e2e" },
    { name: "E2E tests", description: "Playwright end-to-end tests" }
  ]
})
```

The dashboard will show each step as a node in the PlanMap.

### 4b. Execute — MUST pass step_id for each step

For each step, pass the `step_id` (from the steps array returned by planning) to highlight it in PlanMap:

```
oco.emit_phase({ session_id: "<id>", phase: "executing", step_id: "<step_id>", detail: "Creating root config files" })
```

Then do the actual work (Edit/Write tools). When moving to the next step, call emit_phase again with the new step_id — the previous step auto-completes.

**Pattern for each step:**
1. Call `oco.emit_phase({ phase: "executing", step_id: "<id>" })` — marks step as running in PlanMap
2. Do the work (Edit/Write/Bash)
3. Move to next step (previous auto-completes)

### 4c. Verify

```
oco.emit_phase({ session_id: "<id>", phase: "verifying" })
```

This auto-completes any running step and updates the stepper.

Run the verification sequence:
- **Delegate to `/oco-verify-fix`** if available
- Otherwise: build → types → lint → tests manually
- If verification fails, fix and re-verify (max 3 attempts)

## Step 5: Complete

On success:
```
oco.emit_phase({ session_id: "<id>", phase: "complete" })
```

On failure:
```
oco.emit_phase({ session_id: "<id>", phase: "failed", detail: "<error description>" })
```

Produce a compact summary:
```
## Result
- **Task**: <what was requested>
- **Actions taken**: <numbered list>
- **Files changed**: <list>
- **Verification**: PASS / FAIL / PARTIAL
- **Dashboard**: <url from open_dashboard>
```

## Rules

- **Always open dashboard first** — Step 1 is non-negotiable
- **Push phases at every transition** — the dashboard must stay in sync
- **Route to sub-skills** when intent matches — don't reinvent them
- **Verify after every code change** — non-negotiable
- **Evidence before fixes** — read the code first
- **Max 3 retries** — after 3 failures, escalate to user
- **The `session_id` from Step 1 must be passed to every `emit_phase` call**
