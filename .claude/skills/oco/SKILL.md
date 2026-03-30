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

You are entering OCO's orchestrated mode — a structured workflow that leverages all available
code intelligence tools to handle the user's request with maximum precision.

## Step 1: Boot & Discover Capabilities

Before any action, gather context about the workspace and available tools.

**If yoyo MCP tools are available** (`boot`, `index`):
```
yoyo.boot() — discover workspace structure
yoyo.index() — ensure AST index is fresh
```

**If OCO MCP tools are available** (`oco.search_codebase`, `oco.verify_patch`, etc.):
- Note them as available for downstream steps

**Always**:
- Detect project type from manifests (Cargo.toml, package.json, pyproject.toml, go.mod)
- Note the language, build system, and test framework
- Check recent git activity: `git log --oneline -5`

Produce a one-line context summary:
```
[workspace: <name> | lang: <lang> | tools: yoyo+oco / oco-only / basic]
```

## Step 2: Classify Intent & Route

Analyze the user's request and route to the optimal workflow:

| Intent Signal | Route To | Description |
|---------------|----------|-------------|
| explore, understand, "how does X work", architecture | `/oco-inspect-repo-area` | Structured codebase exploration |
| bug, broken, regression, "doesn't work" (no stacktrace) | `/oco-investigate-bug` | Evidence-first bug investigation |
| stacktrace, panic, exception, crash, error + line number | `/oco-trace-stack` | Stack trace root cause analysis |
| refactor, rename, move, extract, split, restructure | `/oco-safe-refactor` | Impact-gated staged refactoring |
| verify, test, check, validate, "does it build" | `/oco-verify-fix` | Full verification suite |
| **new feature, implement, add, create** | **Plan + Implement** (see Step 3) | Feature implementation workflow |
| **complex / multi-step / unclear** | **Decompose** (see Step 3) | Break down then route sub-tasks |

**If the intent maps to an existing `/oco-*` skill**, invoke that skill with the user's request.
Do NOT re-implement what those skills already do — delegate to them.

## Step 3: Plan + Implement (for feature work & complex tasks)

For tasks that don't map to a single sub-skill:

### 3a. Assess Scope

Use impact analysis tools to understand the change surface:

**If yoyo is available**:
```
yoyo.judge_change({ description: "<what will change>" })  — ownership, invariants, risk
yoyo.impact({ symbol: "<target>" })                       — dependency graph
yoyo.routes({ symbol: "<target>" })                       — call chain
```

**If OCO call graph is available** (`oco.routes`, `oco.impact`):
```
oco.routes({ symbol: "<target>", workspace: "." })
oco.impact({ symbol: "<target>", workspace: "." })
```

**Otherwise**: Use Grep/Glob to find all usages and map dependencies manually.

### 3b. Decompose

Break the task into ordered sub-tasks:
1. **Understand** — read the relevant code (use `/oco-inspect-repo-area` pattern)
2. **Plan** — list the files to change and in what order
3. **Implement** — make changes, smallest unit first
4. **Verify** — after EACH implementation step, run the verification sequence

### 3c. Implement with Guard Rails

For each code change:
1. Read the target file before editing
2. **If yoyo `change` is available**: use it for AST-safe writes with compiler rollback
3. **Otherwise**: use Edit tool, then immediately verify
4. After all changes: run `/oco-verify-fix` workflow

## Step 4: Enrich with Code Intelligence (throughout)

Use these tools proactively during any workflow step:

**Search & Navigation**:
- `yoyo.search({ query })` or `oco.search_codebase({ query })` — symbol-aware search
- `yoyo.inspect({ symbol })` — deep symbol introspection (signature, scope, relations)
- `yoyo.map({ query })` — find files by intent
- `yoyo.ask({ question })` — natural language code queries ("who calls foo?")

**Impact & Dependencies**:
- `yoyo.impact({ symbol })` or `oco.impact({ symbol })` — what depends on this?
- `yoyo.routes({ symbol })` or `oco.routes({ symbol })` — call chain traversal
- `yoyo.health()` — dead code, symbol coverage, module health

**Change Assessment**:
- `yoyo.judge_change({ description })` — pre-change risk assessment
- `oco.collect_findings({ workspace })` — synthesize evidence from investigation

**Fallback**: If neither yoyo nor OCO MCP tools are available, use Grep + Glob + Read directly.
The workflow structure remains the same — only the tool calls differ.

## Step 5: Mandatory Verification

After ANY code modification, run the verification sequence:

1. **Delegate to `/oco-verify-fix`** — it handles project detection and runs build → types → lint → tests
2. If verification fails, fix and re-verify (max 3 attempts per issue)
3. After 2 failed attempts on the same issue, switch to `/oco-investigate-bug` workflow

## Step 6: Synthesize & Report

After completing the workflow, produce a compact summary:

```
## Result
- **Task**: <what was requested>
- **Actions taken**: <numbered list>
- **Files changed**: <list with brief description>
- **Verification**: PASS / FAIL / PARTIAL
- **Risks / follow-ups**: <any remaining concerns>
```

## Rules

- **Always boot before acting** — never skip Step 1
- **Route to sub-skills** — don't reinvent `/oco-investigate-bug` or `/oco-safe-refactor`
- **Verify after every code change** — non-negotiable
- **Evidence before fixes** — never guess, always read the code first
- **Delegate large scopes** — >5 files to `@codebase-investigator`, >10 files review to `@refactor-reviewer`
- **Use the best tool available** — prefer yoyo/OCO MCP tools over raw Grep/Glob when available
- **Compact context** — summarize before acting, don't dump raw output
- **Max 3 retries** — after 3 failures on the same step, escalate to user
