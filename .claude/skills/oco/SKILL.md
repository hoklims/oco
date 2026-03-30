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

### 4a. Plan
```
oco.emit_phase({ session_id: "<id>", phase: "planning", detail: "<N files to create/modify>" })
```

Break the task into ordered sub-tasks:
1. List files to create/modify and in what order
2. Identify dependencies between steps
3. Estimate scope

### 4b. Execute

```
oco.emit_phase({ session_id: "<id>", phase: "executing", detail: "Step 1: <description>" })
```

For each step:
1. Read target files before editing
2. Make changes using Edit/Write tools
3. Update the dashboard detail as you progress:
   ```
   oco.emit_phase({ session_id: "<id>", phase: "executing", detail: "Step N: <current step>" })
   ```

### 4c. Verify

```
oco.emit_phase({ session_id: "<id>", phase: "verifying" })
```

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
