---
name: oco-safe-refactor
description: Structured refactoring with impact analysis, staged changes, and verification. Use for renames, restructuring, module extraction.
triggers:
  - "refactor"
  - "rename"
  - "restructure"
  - "extract"
  - "move to"
  - "split into"
  - "reorganize"
  - "decouple"
---

# OCO: Safe Refactor

You are performing a refactoring operation. Follow this staged, verification-gated workflow.

## Step 1: Define the Refactoring Scope

Clearly state:
- **What** is being refactored (symbol, module, pattern)
- **Why** (improve clarity, reduce coupling, fix naming)
- **Boundary**: what files/modules are affected

## Step 2: Impact Analysis

Before making any changes:

1. **Find all usages** of the target symbol/pattern:
   - Use `oco.search_codebase` or Grep for symbol references
   - Check imports, re-exports, type references, test references
   - Check config files, documentation, comments

2. **Map the dependency graph**:
   - What depends on the thing being refactored?
   - What does it depend on?
   - Are there external consumers (API, CLI, exports)?

3. **Produce impact summary**:
   - Files affected: [list]
   - Symbols affected: [list]
   - Risk level: low / medium / high
   - Breaking changes: yes / no

If impact is high (>10 files or breaking changes), delegate deep analysis to `@refactor-reviewer` subagent.

## Step 3: Staged Changes

Apply changes in this order:
1. **Internal implementation** (the core change)
2. **Direct consumers** (files importing/using the changed entity)
3. **Indirect consumers** (transitive dependencies)
4. **Tests** (update to match new structure)
5. **Documentation/config** (if applicable)

**After each stage, verify the build compiles.**

## Step 4: Verification

Run the full verification suite:
1. Build: `cargo build` / `npm run build` / equivalent
2. Type check: `cargo check` / `tsc --noEmit` / equivalent
3. Tests: `cargo test` / `npm test` / equivalent
4. Lint: `cargo clippy` / `eslint` / equivalent

Follow the verification workflow described in the `oco-verify-fix` skill (build, test, lint, typecheck in order).

## Step 5: Review

Delegate reviews to the appropriate subagents:
- `@refactor-reviewer` — check for stale references, breaking changes, and hidden impact
- `@patch-verifier` — semantic review of the change for correctness and completeness

## Rules

- Never rename/move without searching for all usages first
- Never skip the impact analysis step
- If >10 files change, produce a summary for user review before committing
- Preserve all existing test coverage
- Keep each logical change as a separate commit if practical
