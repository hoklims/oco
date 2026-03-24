# Architecture Overview

## System Diagram

```
                    ┌─────────────────────────────────────┐
                    │          VS Code Extension           │
                    │         (TypeScript / pnpm)          │
                    └──────────────┬──────────────────────┘
                                   │ HTTP/JSON
                    ┌──────────────▼──────────────────────┐
                    │          MCP Server (Axum)           │
                    │   ┌─────────────────────────────┐   │
                    │   │     Orchestrator Core        │   │
                    │   │  ┌───────────────────────┐  │   │
                    │   │  │    Policy Engine       │  │   │
                    │   │  │  - Task Classifier     │  │   │
                    │   │  │  - Action Selector     │  │   │
                    │   │  │  - Budget Enforcer     │  │   │
                    │   │  │  - Write Gates         │  │   │
                    │   │  │  - Knowledge Estimator │  │   │
                    │   │  └───────────────────────┘  │   │
                    │   │  ┌───────────────────────┐  │   │
                    │   │  │   Context Engine       │  │   │
                    │   │  │  - Assembler           │  │   │
                    │   │  │  - Compressor          │  │   │
                    │   │  │  - Deduplicator        │  │   │
                    │   │  │  - Token Estimator     │  │   │
                    │   │  └───────────────────────┘  │   │
                    │   │  ┌───────────────────────┐  │   │
                    │   │  │   Tool Runtime         │  │   │
                    │   │  │  - Registry            │  │   │
                    │   │  │  - Shell Executor      │  │   │
                    │   │  │  - File Executor       │  │   │
                    │   │  │  - Normalizer          │  │   │
                    │   │  └───────────────────────┘  │   │
                    │   └─────────────────────────────┘   │
                    │                                     │
                    │  ┌──────────┐  ┌─────────────────┐  │
                    │  │ Code     │  │   Retrieval      │  │
                    │  │ Intel    │  │  - FTS5 Index    │  │
                    │  │ (TS)    │  │  - Vector Backend │  │
                    │  └──────────┘  │  - Hybrid        │  │
                    │                └─────────────────┘  │
                    │  ┌──────────┐  ┌─────────────────┐  │
                    │  │ Verifier │  │   Telemetry      │  │
                    │  │ - Tests  │  │  - Traces        │  │
                    │  │ - Build  │  │  - Metrics       │  │
                    │  │ - Lint   │  │  - tracing init  │  │
                    │  └──────────┘  └─────────────────┘  │
                    └──────────────┬──────────────────────┘
                                   │ HTTP/JSON
                    ┌──────────────▼──────────────────────┐
                    │         ML Worker (Python)           │
                    │  - Sentence Transformers (embed)     │
                    │  - Cross-Encoder (rerank)            │
                    │  - Fallback heuristics               │
                    └──────────────┬──────────────────────┘
                                   │
                    ┌──────────────▼──────────────────────┐
                    │           SQLite + FTS5              │
                    └─────────────────────────────────────┘
```

## Crate Dependency Graph

```
shared-types ◄── policy-engine
     ▲            ▲
     │            │
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
     └── orchestrator-core (depends on all above)
              ▲
              │
              ├── mcp-server
              │
              └── dev-cli
```

## Data Flow

1. User sends request via VS Code extension or dev-cli
2. MCP server creates a session and starts the orchestration loop
3. Policy engine classifies complexity and selects first action
4. Action is executed (retrieve/tool/verify/respond)
5. Result is normalized into a structured Observation
6. State is updated, decision trace is recorded
7. Loop continues until stop condition (complete/budget/error)
8. Final state and traces are returned to the client
