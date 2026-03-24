# Open Context Orchestrator (OCO)

Intelligent orchestration middleware for IDE-based coding assistants.

OCO sits between your IDE, an LLM, local tools, and context sources. It decides at each step whether to respond, retrieve context, call a tool, verify a result, or stop — producing structured decision traces for full auditability.

## Architecture

```
┌──────────────┐     ┌──────────────────┐     ┌─────────────┐
│  VS Code Ext │◄───►│  Orchestrator    │◄───►│  ML Worker  │
│  (TypeScript)│     │  Core (Rust)     │     │  (Python)   │
└──────────────┘     │  ┌────────────┐  │     └─────────────┘
                     │  │ Policy Eng │  │
                     │  │ Context Eng│  │     ┌─────────────┐
                     │  │ Code Intel │  │◄───►│  LLM APIs   │
                     │  │ Tool RT    │  │     │  (any)      │
                     │  │ Retrieval  │  │     └─────────────┘
                     │  │ Verifier   │  │
                     │  └────────────┘  │     ┌─────────────┐
                     │  MCP Server      │◄───►│  SQLite     │
                     └──────────────────┘     └─────────────┘
```

## Key Principles

- **Provider-agnostic** — works with any LLM API
- **Local-first** — no cloud dependencies required
- **Auditable** — every decision produces a structured trace
- **Bounded** — explicit token, time, and tool-call budgets
- **Graceful degradation** — works without ML components via heuristic fallbacks

## Stack

| Layer | Technology |
|-------|-----------|
| Core runtime | Rust, Tokio, Axum |
| Storage | SQLite + FTS5 |
| Code analysis | Tree-sitter |
| IPC | gRPC / Protobuf |
| IDE extension | TypeScript, VS Code API |
| ML worker | Python, Sentence Transformers |
| Telemetry | tracing + OpenTelemetry |

## Getting Started

```bash
# Prerequisites: Rust 1.85+, Node 20+, Python 3.11+, pnpm, uv

# Build Rust crates
cargo build

# Setup Python ML worker
cd py/ml-worker && uv sync

# Setup VS Code extension
cd apps/vscode-extension && pnpm install

# Run dev CLI
cargo run -p oco-dev-cli -- --help
```

## License

Apache-2.0
