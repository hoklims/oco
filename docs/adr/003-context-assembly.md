# ADR-003: Context Assembly Strategy

## Status
Accepted

## Context
LLM context windows are expensive and bounded. We need a strategy to maximize signal-to-noise ratio.

## Decision

### Priority-Based Assembly
Context items are ranked by:
1. Priority tier (System > Pinned > High > Medium > Low > Summary)
2. Relevance score within each tier
3. Greedy packing until token budget is reached

### Pinned Context
Users/system can pin items that persist across steps. Pinned items get priority inclusion.

### Deduplication
- Exact duplicate removal by key
- Overlapping code snippets from the same file are merged

### Compression
- Truncation: keep first/last N lines for long code blocks
- Summary compression (placeholder for LLM-based summarization)

### Token Estimation
Heuristic: `text.len() / 4` with adjustments for code density.

## Consequences
- Predictable context assembly behavior
- No wasted tokens on duplicate content
- Clear observability of what was included/excluded
