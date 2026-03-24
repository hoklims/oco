# OCO v2 — Working Memory Design

## Purpose

Maintain structured task state that survives observation buffer rotation and context compression.

## Memory Categories

1. **Findings** — things discovered but not yet verified (confidence < 1.0)
2. **Verified Facts** — confirmed findings (confidence = 1.0)
3. **Hypotheses** — theories being explored
4. **Questions** — unresolved items needing more information
5. **Plan** — ordered list of next steps

## Entry Structure

- **id**: UUID
- **content**: human-readable text
- **created_at**: timestamp
- **source**: optional file/tool reference
- **tags**: categorization
- **confidence**: 0.0 to 1.0

## Lifecycle Operations

- `add_finding(entry)` — new discovery
- `promote_to_fact(id)` — finding confirmed, sets confidence to 1.0
- `invalidate(id, reason)` — moves to invalidated list with reason tag
- `add_hypothesis(entry)` — theory to explore
- `add_question(entry)` — open question
- `update_plan(steps)` — replace current plan
- `resolve_question(id)` — remove resolved question

## Auto-Population Rules

The orchestration loop automatically adds entries for:

- Verification results (pass -> finding, fail -> finding with failure details)
- Errors (-> finding with error tag)
- Symbol discoveries (-> finding with source reference)

## Context Integration

When `WorkingMemory.active_count() > 0`, its `summary()` is injected as a pinned context item during LLM calls. Format:

```
## Working Memory

Verified facts (N):
  - fact 1
  - fact 2

Findings (N):
  - finding 1 (confidence: 70%)

Open questions (N):
  ? question 1

Current plan:
  1. step 1
  2. step 2
```

## Persistence

Working memory is serializable (`Serialize`/`Deserialize`) and can be persisted to `.oco/memory.json` for session continuity.
