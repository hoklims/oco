# Contributing to OCO

Thanks for your interest in contributing to the Open Context Orchestrator.

## Getting Started

### Prerequisites

- Rust 1.85+ (edition 2024)
- Node 20+ and pnpm
- Python 3.11+ and uv
- SQLite 3.35+ (bundled via rusqlite)

### Setup

```bash
# Clone and build
git clone https://github.com/open-context-orchestrator/oco.git
cd oco
cargo build

# Python ML worker (optional — OCO degrades gracefully without it)
cd py/ml-worker && uv sync

# VS Code extension (optional)
cd apps/vscode-extension && pnpm install
```

### Running Tests

```bash
cargo test          # Full Rust test suite
cargo clippy -- -D warnings   # Lint check
```

## How to Contribute

### Reporting Bugs

Open a [GitHub issue](https://github.com/open-context-orchestrator/oco/issues/new?template=bug_report.md) with:
- Steps to reproduce
- Expected vs actual behavior
- OS, Rust version, relevant config

### Suggesting Features

Open a [GitHub issue](https://github.com/open-context-orchestrator/oco/issues/new?template=feature_request.md) describing:
- The problem you're solving
- Your proposed approach
- Alternatives you considered

### Submitting Code

1. Fork the repo and create a branch: `feat/my-feature` or `fix/my-fix`
2. Make your changes following the conventions below
3. Ensure all tests pass: `cargo test && cargo clippy -- -D warnings`
4. Open a pull request against `main`

## Conventions

### Rust

- Edition 2024, workspace dependencies in root `Cargo.toml`
- No `unwrap()` in production code — use `?`, `anyhow`, or `thiserror`
- Clippy clean: `cargo clippy -- -D warnings`
- Format: `cargo fmt` before committing

### Commits

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add hybrid search ranking
fix: prevent budget overflow on retry
refactor: extract session state into module
docs: update ADR for context assembly
test: add verifier edge cases
chore: bump tokio to 1.44
```

### Architecture Decisions

For significant changes, write an ADR in `docs/adr/` following the existing format (see `docs/adr/001-stack-selection.md` for reference).

## Code Review

All PRs require at least one review. Reviewers check for:
- Correctness and test coverage
- No `unwrap()` or panics in library code
- Deterministic policy (no LLM calls in routing logic)
- Bounded resource usage (budgets, timeouts)

## License

By contributing, you agree that your contributions will be licensed under the [Apache License 2.0](LICENSE).
