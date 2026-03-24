# ADR-002: Orchestration Model

## Status
Accepted

## Context
The core problem: given a user request and workspace state, what action should the system take next?

## Decision

### Five-Action Model
At each step, the orchestrator selects exactly one action from:
1. **Respond** — generate a response to the user
2. **Retrieve** — fetch additional context (code, docs, search)
3. **ToolCall** — execute an external tool (shell, file ops, LSP)
4. **Verify** — run tests, build, lint, type-check
5. **Stop** — terminate the loop

### Deterministic Policy Engine
Action selection is deterministic (no LLM call for routing):
- Keyword-based task complexity classification
- Score-based action selection with transparent alternatives
- Budget enforcement at every step
- Write actions get stricter verification gates

### Bounded Loops
- Max steps per session (default: 25)
- Token/time/tool-call budgets explicitly tracked
- Consecutive error limit (3) triggers automatic stop
- All decisions produce structured traces

### State Machine
```
[start] -> classify -> [select_action] -> execute -> normalize -> [select_action] -> ... -> [stop]
```

## Consequences
- No "thinking" token cost for routing decisions
- Fully auditable decision traces
- Predictable behavior under budget constraints
- LLM is used only for content generation, not for routing
