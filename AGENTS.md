# Open Context Orchestrator (OCO)

## Quick Start

```bash
cargo build                              # Build all crates
cargo test                               # Run full test suite (110+ tests)
cargo run -p oco-dev-cli -- --help       # CLI help

oco index ./path                         # Index a workspace
oco search "query" --workspace ./path    # Full-text search
oco run "request" --workspace ./path     # Orchestrate an action
oco serve --port 3000                    # Start HTTP/MCP server
oco doctor --workspace ./path            # Check plugin health
oco eval scenarios.jsonl                 # Run evaluation scenarios
```

## Architecture

Polyglot monorepo: **Rust core** + **Python ML worker** + **TypeScript VS Code extension**.

### Rust Crates (dependency order)

| # | Crate | Role |
|---|-------|------|
| 1 | `shared-types` | Domain types: Session, Action, Observation, Budget, Context, VerificationState, WorkingMemory, RepoProfile, ReplayScenario, TelemetryEvent |
| 2 | `shared-proto` | Protobuf definitions (gRPC IPC) |
| 3 | `policy-engine` | Deterministic action selection, budget enforcement, task classification |
| 4 | `code-intel` | Tree-sitter parser (regex fallback), symbol indexer |
| 5 | `retrieval` | SQLite FTS5, in-memory vector search, hybrid RRF ranking |
| 6 | `tool-runtime` | Shell/file executors, observation normalizer |
| 7 | `verifier` | Test/build/lint/typecheck runners with auto-detection |
| 8 | `telemetry` | Tracing init, decision trace collector, event recording |
| 9 | `context-engine` | Context assembly, dedup, compression, staleness decay, category budgets |
| 10 | `orchestrator-core` | State machine, action loop, LLM providers, runtime, eval runner, repo profiles |
| 11 | `mcp-server` | Axum HTTP + MCP server, session management |
| 12 | `dev-cli` | CLI binary (index, search, run, serve, eval, doctor) |

### Python (`py/`)

- **`ml-worker`** — FastAPI server for sentence-transformers embeddings and reranking
- **`eval-harness`** — Evaluation scenario framework

### TypeScript (`apps/vscode-extension/`)

- VS Code extension: command palette, trace panel, HTTP client

## Conventions

### Rust
- **Edition 2024**, workspace dependencies in root `Cargo.toml`
- **No `unwrap()` in production code** — use `?`, `anyhow::Result`, or `thiserror`
- **Clippy clean** — `cargo clippy -- -D warnings`
- Errors: typed with `thiserror` per crate, `anyhow` at binary boundaries
- Imports: workspace crate aliases (`oco-shared-types`, etc.)

### Design Principles
- **Deterministic policy** — no LLM calls for routing decisions
- **Structured observations** — all tool/retrieval outputs normalized before entering state
- **Bounded loops** — max steps enforced via token/time/tool budgets
- **Provider-agnostic** — works with Anthropic, Ollama, or stub provider
- **Local-first** — no cloud dependencies required (ML worker optional)

### Git
- Conventional commits: `feat:`, `fix:`, `refactor:`, `docs:`, `test:`, `chore:`
- Branch naming: `feat/xxx`, `fix/xxx`
- Never push directly to `main`

## Testing

```bash
cargo test                               # All tests
cargo test -p oco-shared-types           # 14 tests — domain types, verification, memory, profiles
cargo test -p oco-policy-engine          # 30 tests — classifier, selector, budget, gates
cargo test -p oco-context-engine         # 15 tests — assembler, dedup, compression, staleness
cargo test -p oco-code-intel             # 16 tests — parser, indexer, language detection
cargo test -p oco-retrieval              #  9 tests — FTS5, vector, hybrid ranking
cargo test -p oco-telemetry              #  2 tests — event recording, JSONL export
cargo test -p oco-orchestrator-core      #  8 tests — eval, integration
```

## LLM Providers

| Provider | Config (`oco.toml`) | Requirements |
|----------|-------------------|--------------|
| `stub` | `provider = "stub"` | None — returns placeholder responses |
| `anthropic` | `provider = "anthropic"` | `ANTHROPIC_API_KEY` env var |
| `ollama` | `provider = "ollama"` | Local Ollama server at `localhost:11434` |

## Configuration

Runtime config lives in `oco.toml` at workspace root. See `examples/oco.toml` for a documented template.

Key sections:
- **Server** — bind address, port, max sessions
- **Budget** — token limits, tool call caps, duration, verify cycles
- **LLM** — provider, model, API key env var, retries

## Codex Integration

This repo includes a `.Codex/` directory with project-specific tooling:

- **Skills** — `/oco-inspect-repo-area`, `/oco-investigate-bug`, `/oco-safe-refactor`, `/oco-trace-stack`, `/oco-verify-fix`
- **Agents** — `codebase-investigator`, `patch-verifier`, `refactor-reviewer`
- **MCP bridge** — Exposes OCO tools (search, trace, verify, findings) as MCP resources
- **Hooks** — Pre/post tool-use validation, session init, stop handlers

## Project Layout

```
oco/
├── apps/
│   ├── dev-cli/                  # CLI binary
│   └── vscode-extension/         # VS Code extension
├── crates/                       # 11 Rust crates (see table above)
├── docs/
│   ├── adr/                      # Architecture Decision Records
│   ├── architecture/             # System overview
│   └── specs/                    # Feature specifications
├── examples/                     # Sample repo, traces, config template
├── py/
│   ├── ml-worker/                # Python embedding/reranking server
│   └── eval-harness/             # Evaluation framework
├── schemas/
│   ├── jsonschema/               # Config schema
│   └── proto/                    # Protobuf definitions
├── scripts/                      # Bootstrap, CI, dev helpers
├── .Codex/                      # Codex skills, hooks, MCP bridge
├── Cargo.toml                    # Workspace root
├── oco.toml                      # Runtime configuration
└── pyproject.toml                # Python workspace
```
