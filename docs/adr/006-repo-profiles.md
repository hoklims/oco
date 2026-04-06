# ADR-006: Repository Profile Adaptation

## Status
Accepted

## Context
In v1, verification runners and stack-specific behavior were hardcoded:

- **Fixed commands**: The verifier used hardcoded commands (e.g., `cargo test`, `npm test`) selected by simple file-existence checks. Repositories with custom build systems, monorepo tooling, or non-standard layouts required manual intervention.
- **No risk modeling**: All files were treated equally. There was no way to flag sensitive paths (e.g., `migrations/`, `.env`, `Cargo.toml`) as requiring extra caution or mandatory verification.
- **No stack-specific configuration**: Different ecosystems have different conventions for testing, linting, and building. A Python project using `pytest` with coverage differs fundamentally from a Rust project using `cargo nextest`, but v1 had no mechanism to express these differences.

This limited OCO's out-of-box usefulness across diverse repositories.

## Decision
Introduce `RepoProfile` — a structured description of a repository's stack, conventions, and risk model. Profiles are built from two sources that merge together.

### Auto-Detection
On workspace indexing, OCO scans for manifest files and infers a base profile:

- **Cargo.toml** → Rust stack: `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt --check`
- **package.json** → Node stack: reads `scripts` for build/test/lint commands, detects package manager (npm/pnpm/yarn/bun)
- **pyproject.toml** → Python stack: detects pytest/mypy/ruff from dependencies and config sections
- **go.mod** → Go stack: `go build ./...`, `go test ./...`, `go vet ./...`

Monorepos with multiple manifests produce a composite profile with per-directory stack assignments.

### oco.toml Overrides
Users can provide an `oco.toml` file at the workspace root to override or extend the detected profile:

```toml
[profile]
stack = "rust"

[profile.commands]
build = "cargo build --release"
test = "cargo nextest run"
lint = "cargo clippy -- -D warnings"
typecheck = "cargo check"

[profile.sensitive_paths]
high_risk = ["migrations/", "Cargo.toml", ".env*"]
medium_risk = ["src/config/", "build.rs"]

[profile.verification]
required_before_complete = ["test", "build"]
optional = ["lint", "typecheck"]
```

### Merge Semantics
- **Override-based**: Explicit `oco.toml` values replace detected values. There is no deep merge — if a user specifies `[profile.commands]`, it fully replaces the detected commands block.
- **Additive sensitive paths**: `sensitive_paths` from detection and `oco.toml` are merged (union), since both sources provide useful signals.
- **Detection as fallback**: If no `oco.toml` exists, the detected profile is used as-is. If `oco.toml` exists but omits a section, detection fills the gap.

### Policy Integration
The policy engine uses the repo profile to:
- Select appropriate verification runners for the current stack.
- Assign risk levels to file modifications based on `sensitive_paths`.
- Require mandatory verification for changes touching high-risk paths, even for tasks that would otherwise be exempt.

## Consequences

### Positive
- **Better out-of-box experience**: Most repositories work without any configuration because auto-detection covers the common stacks (Rust, Node, Python, Go).
- **Customizable**: Power users can fine-tune commands, risk levels, and verification requirements through `oco.toml` without modifying OCO's source code.
- **Risk-aware policy**: Sensitive path tracking enables the policy engine to enforce stricter verification for changes that carry higher risk.

### Negative
- **Heuristic detection**: Auto-detection is based on manifest file presence and content parsing. Unusual project layouts or custom build systems may produce incorrect profiles, requiring manual `oco.toml` overrides.
- **Override granularity**: The override-based merge means users must re-specify an entire section if they want to change one command. A future iteration could support per-field overrides if this proves too coarse.
- **Maintenance burden**: As new stacks and tools emerge, the detection logic must be updated. This is mitigated by the `oco.toml` escape hatch — unsupported stacks can always be configured manually.

## Q3 2026 Extension: Policy Packs

`RepoProfile` now includes a `policy_pack` field (`fast`, `balanced`, `strict`) that governs the trust contract for the repository:

- **fast**: build-only gate, stale completion allowed.
- **balanced** (default): build + test required, stale completion blocked.
- **strict**: full verification suite, stale blocked, unverified sensitive paths degrade the trust verdict.

The policy pack is parsed from `oco.toml` via `[profile] policy_pack = "strict"` and defaults to `balanced` when omitted. It feeds into `PolicyPackGate` (completion gating), `effective_tier()` (minimum verification tier), and `TrustVerdict::compute()` (run summary verdict).

See also: ADR-009 (Session Continuity Model) for how the policy pack influences post-compact behavior.
