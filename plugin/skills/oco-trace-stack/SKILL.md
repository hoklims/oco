---
name: oco-trace-stack
description: Analyze a stack trace or runtime error to identify root cause. Use when a stacktrace or runtime error is present.
triggers:
  - "stacktrace"
  - "stack trace"
  - "traceback"
  - "runtime error"
  - "panic"
  - "exception"
  - "crash"
  - "error at line"
---

# OCO: Trace Stack Error

You are analyzing a runtime error or stack trace. Follow this evidence-based workflow.

## Step 1: Parse the Stack Trace

Extract from the error:
- **Error type and message**
- **File paths and line numbers** (ordered by stack depth)
- **Relevant variable values** if visible
- **Error chain** (caused by / wrapped errors)

## Step 2: Map to Codebase

Use `oco.trace_error` MCP tool if available:

```
oco.trace_error({ stacktrace: "<paste stacktrace>", workspace: "." })
```

Otherwise, manually inspect the files referenced in the stack trace, starting from the deepest application frame (skip library/framework frames).

## Step 3: Inspect Likely Root Cause Regions

For each candidate location:
1. Read the file at the specific line
2. Read surrounding context (function scope)
3. Check for: null/undefined access, type mismatches, missing error handling, race conditions, invalid state

## Step 4: Generate Hypotheses

Produce 1-3 ranked hypotheses:
- **H1** (most likely): description + evidence
- **H2** (alternative): description + evidence
- **H3** (edge case): description + evidence

## Step 5: Verify Before Claiming a Fix

**Do NOT propose a fix until you have:**
1. Confirmed the hypothesis by reading the actual failing code
2. Checked if the error is reproducible from the described scenario
3. Verified the fix won't introduce regressions

## Rules

- Never guess at a fix without reading the code
- If the stack trace references >5 files, delegate deep reading to `@codebase-investigator`
- Always state which hypothesis you're most confident in and why
- After applying a fix, run the verification workflow described in the `oco-verify-fix` skill (build, test, lint, typecheck)
- Use `oco.collect_findings` to synthesize evidence and open questions before concluding
