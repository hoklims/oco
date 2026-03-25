---
name: oco-investigate-bug
description: >
  Systematic evidence-first bug investigation without a full stacktrace.
  Auto-activates when the user reports a bug, broken behavior, unexpected results, a regression,
  something not working, or a problem without a clear error message.
  Enforces strict workflow: understand symptom → narrow scope → gather evidence → reproduce →
  root cause analysis → fix ONLY after proof. Never guess at fixes.
  Also auto-activates after 2 failed attempts to fix the same problem.
triggers:
  - "debug"
  - "bug"
  - "not working"
  - "broken"
  - "doesn't work"
  - "wrong behavior"
  - "unexpected"
  - "regression"
---

# OCO: Investigate Bug

You are investigating a bug without a clear stack trace. Follow strict evidence-based debugging.

## Step 1: Understand the Symptom

Clarify with the user if needed:
- **Expected behavior**: What should happen?
- **Actual behavior**: What happens instead?
- **Reproduction steps**: How to trigger it?
- **When it started**: Recent change? Always broken?

## Step 2: Narrow the Scope

Identify the subsystem:
1. Search for relevant code using `oco.search_codebase` or standard search
2. Identify the code path from user action to observed behavior
3. List candidate files (max 5 initial candidates)

## Step 3: Gather Evidence

For each candidate:
1. Read the relevant code section
2. Look for: edge cases, missing validation, incorrect logic, state corruption, timing issues
3. Check recent changes (`git log --oneline -10 -- <file>`)
4. Check tests: do existing tests cover this case?

## Step 4: Reproduce or Narrow

Before proposing a fix:
- If tests exist: check if they pass or fail
- If no tests: describe how to reproduce
- Narrow to the smallest possible scope

## Step 5: Root Cause Analysis

State the root cause with evidence:
- **Root cause**: [description]
- **Evidence**: [what you found in the code]
- **Why it wasn't caught**: [missing test, edge case, etc.]

## Step 6: Fix Only After Evidence

Once root cause is confirmed:
1. Propose the minimal fix
2. Explain why the fix addresses the root cause
3. Identify if new tests are needed
4. After applying changes, run the verification workflow described in the `oco-verify-fix` skill (build, test, lint, typecheck)
5. Use `oco.collect_findings` to synthesize current evidence and remaining open questions

## Rules

- **Never guess at fixes** — evidence first
- After 2 failed attempts at the same approach, step back and reconsider
- If scope exceeds 5 files, delegate reading to `@codebase-investigator`
- For semantic review of proposed patches, delegate to `@patch-verifier`
- Always document what you ruled out and why
