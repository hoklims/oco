---
name: oco-inspect-repo-area
description: >
  Structured codebase exploration with OCO-backed code intelligence.
  Auto-activates when the user asks to explore, understand, explain how a module works,
  a feature, an architecture, a data flow, or asks "how does this work", "where is",
  "show me", "what is this module". Enforces: compact summary before any action,
  selective reading (no directory dumps), explicit confidence level.
triggers:
  - "explore"
  - "understand"
  - "how does"
  - "what does"
  - "show me the"
  - "explain the"
  - "where is"
  - "codebase"
  - "module"
  - "architecture"
---

# OCO: Inspect Repository Area

You are performing a focused exploration of a codebase area. Follow this structured workflow.

## Step 1: Identify the Target Area

Determine which part of the codebase needs exploration:
- A module, package, or directory
- A feature or capability
- A data flow or interaction pattern

## Step 2: Gather Ranked Context

Use `oco.search_codebase` if available (ranked, symbol-aware results):
```
oco.search_codebase({ query: "<area description>", workspace: "." })
```

Otherwise, use Grep for key symbols and Glob for file patterns.

Focus on symbol-level results — prefer targeted searches over raw file dumping.

## Step 3: Read Key Files Selectively

Based on search results, read only the most relevant files. **Do NOT dump entire directories.**

Priority order:
1. Entry points and public API surfaces
2. Core types and data structures
3. Key implementation logic
4. Tests (for behavior documentation)

## Step 4: Summarize Before Acting

Before taking any action, produce a **compact summary**:
- Purpose of the area
- Key types and their relationships
- Entry points and data flow
- Potential concerns or complexity hotspots

## Rules

- Never read more than 10 files without summarizing first
- Prefer symbol-level inspection over full file reads
- If an area is complex (>5 files), delegate to the `@codebase-investigator` subagent
- Report confidence level: high / medium / low
