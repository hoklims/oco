# ADR-005: Structured Working Memory

## Status
Accepted

## Context
In v1, all session state was stored in an observation ring buffer (`VecDeque<Observation>`). This flat structure had several limitations:

- **No semantic distinction**: Raw tool outputs, search results, user messages, and important findings were all stored as observations with equal weight. The system could not distinguish between a transient shell output and a verified architectural insight.
- **Compression losses**: When the context window filled up, the compressor applied uniform summarization across all observations. Critical findings discovered early in a session could be lost or degraded during compression, even if they were still relevant.
- **No knowledge accumulation**: Long sessions that involved iterative exploration had no mechanism to accumulate and refine understanding. Each step's output was treated independently, with no way to build on prior conclusions.

This made OCO less effective on complex, multi-step tasks where maintaining curated knowledge across the session was essential.

## Decision
Introduce a `WorkingMemory` structure that operates alongside the existing observation buffer. The observation buffer remains the ground-truth log of all tool interactions; working memory is a curated, structured layer on top.

### WorkingMemory Structure
- **findings**: Discovered facts about the codebase (e.g., "auth module uses JWT with RS256").
- **hypotheses**: Tentative conclusions that need verification (e.g., "the bug is likely in the cache invalidation path").
- **verified_facts**: Findings that have been confirmed through verification or multiple independent observations.
- **questions**: Open questions that the orchestrator has identified but not yet answered.
- **plan**: Current step-by-step plan for achieving the task goal.

### Entry Metadata
Each working memory entry carries:
- **confidence**: Float 0.0–1.0, updated as evidence accumulates.
- **source_step**: The orchestration step that produced this entry.
- **status**: `Active`, `Superseded`, or `Invalidated`.

### Lifecycle Operations
- **add**: Insert a new entry (finding, hypothesis, question, or plan step).
- **promote**: Move a hypothesis to verified_facts when confidence exceeds the threshold or verification confirms it.
- **invalidate**: Mark an entry as invalidated when contradicting evidence is found, preserving it for trace auditing but excluding it from active context.

### Context Integration
Before each LLM call, the context engine injects a working memory summary into the prompt. This summary includes active findings, the current plan, and open questions. Invalidated and superseded entries are excluded from the summary but retained in the full session trace.

## Consequences

### Positive
- **Better context preservation**: Critical findings survive compression because they are stored separately from the observation buffer and injected directly into the LLM context.
- **Structured reasoning**: The distinction between hypotheses and verified facts makes the orchestrator's reasoning process more transparent and auditable.
- **Long session performance**: Sessions that span many steps can accumulate knowledge rather than losing it to context window pressure.

### Negative
- **Dual tracking overhead**: Maintaining both observations and working memory adds a small memory and processing cost. Mitigated by the bounded size of working memory (entries are invalidated or superseded over time).
- **Unbounded growth risk**: Without active management, working memory could grow indefinitely. This is addressed by enforcing a maximum entry count per category and requiring explicit lifecycle transitions.
- **LLM dependency for extraction**: Extracting structured entries from raw observations may require LLM calls, adding latency. A heuristic extraction path is provided as a fallback for common patterns.
