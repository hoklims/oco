# Architecture Overview

## System Diagram

```
                    ┌─────────────────────────────────────┐
                    │          VS Code Extension           │
                    │         (TypeScript / pnpm)          │
                    └──────────────┬──────────────────────┘
                                   │ HTTP/JSON
                    ┌──────────────▼──────────────────────┐
                    │     MCP Server (Axum)                │
                    │  ┌─────────────────────────────────┐│
                    │  │ HTTP Hook Endpoints (v2.1.63+)  ││
                    │  │ post-tool, task-completed,       ││
                    │  │ file-changed, post-compact, stop ││
                    │  └─────────────────────────────────┘│
┌──────────────┐    │  ┌─────────────────────────────────┐│    ┌─────────────┐
│  Claude Code │◄──►│  │      Orchestrator Core          ││◄──►│  LLM APIs   │
│  (MCP/Hooks) │    │  │  ┌──────────┐  ┌────────────┐  ││    │ (any model) │
└──────────────┘    │  │  │ Policy   │  │  Planner   │  ││    └─────────────┘
                    │  │  │ Engine   │  │ Direct/LLM │  ││
                    │  │  │ Classify │  ├────────────┤  ││
                    │  │  │ Select   │  │ GraphRunner│  ││
                    │  │  │ Budget   │  │ (parallel) │  ││
                    │  │  │ Gates    │  ├────────────┤  ││
                    │  │  └──────────┘  │ LlmRouter  │  ││
                    │  │                │ model+effort│  ││
                    │  │  ┌──────────┐  ├────────────┤  ││
                    │  │  │ Context  │  │AgentTeams  │  ││
                    │  │  │ Engine   │  │ Executor   │  ││
                    │  │  │ +StepCtx │  └────────────┘  ││
                    │  │  └──────────┘                   ││
                    │  │  ┌──────────┐  ┌────────────┐  ││
                    │  │  │ Tool RT  │  │ Code Intel │  ││
                    │  │  │ Shell/FS │  │ Tree-sitter│  ││
                    │  │  └──────────┘  └────────────┘  ││
                    │  │  ┌──────────┐  ┌────────────┐  ││
                    │  │  │ Verifier │  │ Retrieval  │  ││    ┌─────────────┐
                    │  │  │ test/lint│  │ FTS5+Vec   │  ││◄──►│  SQLite     │
                    │  │  └──────────┘  └────────────┘  ││    └─────────────┘
                    │  │  ┌──────────┐                   ││
                    │  │  │Telemetry │                   ││
                    │  │  │ traces   │                   ││
                    │  │  └──────────┘                   ││
                    │  └─────────────────────────────────┘│
                    └──────────────┬──────────────────────┘
                                   │ HTTP/JSON (optional)
                    ┌──────────────▼──────────────────────┐
                    │         ML Worker (Python)           │
                    │  - Sentence Transformers (embed)     │
                    │  - Cross-Encoder (rerank)            │
                    └─────────────────────────────────────┘
```

## Crate Dependency Graph

```
shared-types ◄── shared-proto
     ▲
     │
     ├── policy-engine
     │
     ├── code-intel ◄── context-engine
     │                      ▲
     ├── retrieval ◄────────┘
     │
     ├── tool-runtime
     │
     ├── verifier
     │
     ├── telemetry
     │
     ├── planner (DirectPlanner + LlmPlanner)
     │
     └── orchestrator-core (depends on all above)
              │  ├── GraphRunner (DAG execution)
              │  ├── LlmRouter (model + effort routing)
              │  ├── AgentTeamsExecutor (Claude Code Agent Teams)
              │  └── Eval runner (scenario benchmarking)
              ▲
              │
              ├── mcp-server (HTTP + MCP + hook endpoints)
              │
              └── dev-cli (CLI binary)

architecture-tests (fitness tests, no runtime dependency)
```

## Orchestration Flow

### Trivial/Low tasks — flat action loop
1. User sends request via VS Code extension, Claude Code, or dev-cli
2. MCP server creates a session and starts the orchestration loop
3. Policy engine classifies complexity (Trivial/Low) and selects action
4. Action is executed (retrieve/tool/verify/respond)
5. Result is normalized into a structured Observation
6. State is updated, decision trace is recorded
7. Loop continues until stop condition (complete/budget/error)

### Medium+ tasks — emergent plan engine
1. Classifier detects Medium/High/Critical complexity
2. Planner generates ExecutionPlan DAG (DirectPlanner or LlmPlanner)
3. GraphRunner executes steps in parallel where possible
4. LlmRouter selects model (opus/sonnet/haiku) and effort (low/medium/high) per step
5. AgentTeamsExecutor maps steps to Claude Code Agent Teams (worktree isolation)
6. Verify gates run after implementation steps
7. On failure: replan (max 3 attempts, budget pre-check)
8. Combined outputs are surfaced as Respond action

### Claude Code integration
- **HTTP hooks** receive real-time events (tool use, file changes, session stop)
- **MCP elicitation** enables interactive decisions (replan confirmation, architecture choices)
- **Deferred tool schemas** expose OCO capabilities via ToolSearch
- **Agent Teams** enable parallel step execution with worktree isolation
