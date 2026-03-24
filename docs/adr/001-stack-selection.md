# ADR-001: Technology Stack Selection

## Status
Accepted

## Context
OCO needs a polyglot stack that balances:
- Performance for the core orchestration loop
- Rich ML ecosystem for embeddings/reranking
- IDE integration capabilities
- Local-first operation

## Decision

### Core Runtime: Rust
- Tokio for async, Axum for HTTP/API
- SQLite + FTS5 for persistence and lexical search
- Tree-sitter for code parsing
- Zero-cost abstractions for the hot path (orchestration loop)

### ML Worker: Python
- Sentence Transformers for embeddings and reranking
- ONNX Runtime as optional accelerated inference path
- FastAPI for HTTP API (gRPC planned for v2)
- Graceful degradation: core works without ML worker via heuristic fallbacks

### IDE Extension: TypeScript
- VS Code Extension API
- Biome for linting/formatting (replaces ESLint + Prettier)
- esbuild for bundling

### IPC: Protobuf + gRPC
- Between Rust core and Python ML worker where justified
- HTTP/JSON as primary protocol for v1 (simpler deployment)

## Consequences
- Developers need Rust, Python, and Node toolchains
- Build system must orchestrate polyglot compilation
- Proto schemas serve as the contract between components
