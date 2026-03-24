# OCO v2 — Architecture Specification

## 1. Overview

OCO v2 builds on v1's hybrid Claude Code plugin architecture to add: verification integrity, working memory, improved context selection, measurable decision logic, replay/evaluation, and repo-specific adaptation.

The v1 foundation established the core loop (observe → decide → act → verify), deterministic policy-driven routing, hybrid retrieval (FTS5 + vector), and a plugin-based integration with Claude Code via hooks, skills, and MCP. v2 extends each of these with state-aware subsystems that make the orchestrator's behavior auditable, reproducible, and self-improving.

All v2 additions are **additive** — the system degrades gracefully when v2 state is absent, preserving full v1 compatibility.

## 2. Architecture Layers

### Plugin Layer

The outermost integration surface. Lives in `.claude/` and `.oco/` within the target repository.

| Component | Location | Purpose |
|-----------|----------|---------|
| Hooks | `.claude/hooks/` | Pre/post triggers for Claude Code events (file save, tool call, etc.) |
| Skills | `.claude/skills/` | Declarative task templates surfaced via slash commands |
| Agents | `.claude/agents/` | Role-scoped sub-agents with isolated system prompts |
| MCP Config | `.claude/settings.json` | MCP server registration, tool permissions |
| OCO Config | `oco.toml` | Orchestrator settings, repo profile overrides |

### Core Runtime (Rust)

Twelve workspace crates compiled into two binaries (`oco-dev-cli`, `oco-mcp-server`). The runtime is fully synchronous in its decision path — async is limited to I/O boundaries (HTTP, file system, LLM calls).

### Optional ML Worker (Python)

A FastAPI sidecar (`py/ml-worker/`) providing sentence-transformer embeddings and cross-encoder reranking. The core runtime communicates with it over HTTP. When unavailable, the system falls back to keyword-only retrieval.

### CLI / Dev Tools

The `oco` binary (`apps/dev-cli/`) exposes all orchestrator functionality: indexing, search, orchestration runs, evaluation, and diagnostics (`oco doctor`).

### VS Code Extension (secondary)

TypeScript extension (`apps/vscode-extension/`) providing command palette integration, trace visualization, and HTTP client for the MCP server. Not required for core operation.

## 3. v2 Subsystems

### 3.1 Verification Integrity

**Problem:** v1 treated verification as a one-shot check. After a verify pass, subsequent edits could silently invalidate the result without the orchestrator knowing.

**Solution:** Track file modification state relative to verification runs.

#### Data Model

```rust
// shared-types/verification.rs

struct VerificationState {
    modified_files: HashMap<PathBuf, FileModification>,
    last_run: Option<VerificationRun>,
}

struct FileModification {
    path: PathBuf,
    modified_at: Instant,
    content_hash: u64,
}

struct VerificationRun {
    run_id: String,
    started_at: Instant,
    completed_at: Option<Instant>,
    file_snapshot: HashMap<PathBuf, u64>,  // path → hash at verify time
    result: VerificationResult,
}
```

#### Freshness Computation

The orchestrator computes a `VerificationFreshness` enum before each decision step:

| State | Condition |
|-------|-----------|
| `Fresh` | Last run passed, no files modified since |
| `Partial` | Last run passed, but some non-verified files were modified |
| `Stale` | Files that were included in the last run have been modified since |
| `None` | No verification has ever been run |

#### Policy Integration

The policy engine consults freshness when selecting the next action. When freshness is `Stale`, the engine injects a verification action with elevated priority, preempting other candidates. This ensures the orchestrator never proceeds on assumptions invalidated by edits.

**Located in:** `crates/shared-types/src/verification.rs`, `crates/orchestrator-core/src/loop_runner.rs`

### 3.2 Working Memory

**Problem:** v1's state consisted of a flat list of observations. The orchestrator had no mechanism to accumulate knowledge across steps, track hypotheses, or distinguish verified facts from tentative findings.

**Solution:** A structured working memory that persists across the action loop.

#### Data Model

```rust
// shared-types/memory.rs

struct WorkingMemory {
    findings: Vec<MemoryEntry>,
    hypotheses: Vec<MemoryEntry>,
    verified_facts: Vec<MemoryEntry>,
    questions: Vec<String>,
    plan: Vec<PlanStep>,
}

struct MemoryEntry {
    id: String,
    content: String,
    confidence: f64,       // 0.0–1.0
    source: MemorySource,  // Verification, ToolOutput, UserInput, Inference
    tags: Vec<String>,
    created_at_step: usize,
    invalidated: bool,
}
```

#### Lifecycle

1. **`add_finding`** — New observation enters as a finding with initial confidence.
2. **`promote_to_fact`** — When a finding is confirmed by verification or user input, it moves to `verified_facts` with confidence raised.
3. **`invalidate`** — When a fact is contradicted (e.g., a previously passing test now fails), it is marked `invalidated` and excluded from context injection.

#### Auto-population

- Verification failures automatically generate findings with relevant error details.
- Build errors are parsed and added as findings with file/line tags.
- Successful verifications promote related findings to facts.

#### Context Injection

Before each LLM call, a summary of working memory is serialized and injected as a pinned context item at the top of the assembled context. This ensures the model always has access to accumulated knowledge without re-discovering it.

**Located in:** `crates/shared-types/src/memory.rs`, `crates/orchestrator-core/src/loop_runner.rs`

### 3.3 Context Selection

**Problem:** v1 assembled context by relevance score alone, with no awareness of recency or source-type balance. Long-running sessions suffered from stale context crowding out fresh information.

**Solution:** Staleness decay and per-category token budgets.

#### Staleness Decay

Each `ContextItem` now carries `added_at` (timestamp) and `added_at_step` (loop step index). During assembly, the effective relevance score is adjusted:

```
effective_score = base_score * decay_factor(current_step - added_at_step, half_life)
```

Where `decay_factor` uses exponential decay with a configurable half-life (default: 5 steps). Items from 10+ steps ago have their relevance reduced by 75%, allowing fresher results to surface.

#### Category Budgets

```rust
// shared-types/context.rs

struct CategoryBudgets {
    search_results: usize,    // max tokens for retrieval results
    tool_outputs: usize,      // max tokens for tool observations
    verification: usize,      // max tokens for verify output
    memory: usize,            // max tokens for working memory summary
    user_context: usize,      // max tokens for user-provided context
}
```

The `ContextAssembler` enforces these caps during assembly. Within each category, items compete by effective relevance score. This prevents any single source type from monopolizing the context window.

#### API

```rust
let context = ContextAssembler::new(token_budget)
    .with_staleness(half_life_steps)
    .with_category_budgets(budgets)
    .assemble(items, current_step);
```

**Located in:** `crates/shared-types/src/context.rs`, `crates/context-engine/src/assembler.rs`

### 3.4 Telemetry & Measurable Decisions

**Problem:** v1 collected decision traces but lacked structured event types and outcome measurement. It was impossible to answer "did this intervention help?"

**Solution:** Typed telemetry events with intervention outcome tracking.

#### Event Types

```rust
// shared-types/telemetry.rs

enum TelemetryEvent {
    HookTriggered { hook: String, trigger: String },
    SkillInvoked { skill: String, args: Vec<String> },
    SubagentLaunched { agent: String, task: String },
    VerifyCompleted { freshness: VerificationFreshness, passed: bool },
    ContextAssembled { total_tokens: usize, categories: HashMap<String, usize> },
    MemoryUpdated { action: MemoryAction, entry_count: usize },
    VerificationStale { modified_files: usize },
    BudgetThreshold { resource: String, used: f64, limit: f64 },
}
```

#### Intervention Outcome

Each orchestrator intervention (a complete sequence from trigger to resolution) is tagged with an outcome:

| Outcome | Definition |
|---------|------------|
| `Useful` | Intervention resolved the issue or advanced the task |
| `Redundant` | Intervention was correct but unnecessary (user would have done it) |
| `Harmful` | Intervention introduced errors or wasted significant budget |
| `Unknown` | Outcome could not be determined |

#### Aggregation

`InterventionSummary` provides per-session statistics: total interventions, outcome distribution, average tokens per intervention, and trigger-type breakdown. This data feeds the evaluation pipeline.

#### Export

All telemetry events are written to a JSONL file (`.oco/traces/<session-id>.jsonl`) for offline analysis and replay.

**Located in:** `crates/shared-types/src/telemetry.rs`, `crates/telemetry/src/traces.rs`

### 3.5 Replay / Evaluation

**Problem:** No way to regression-test orchestrator behavior or compare decision quality across versions.

**Solution:** A scenario-based evaluation framework with deterministic replay.

#### Data Model

```rust
// shared-types/replay.rs

struct ReplayScenario {
    name: String,
    user_request: String,
    workspace: PathBuf,
    expected_actions: Vec<ExpectedAction>,
    config_overrides: HashMap<String, String>,
}

struct ScenarioResult {
    scenario_name: String,
    step_count: usize,
    total_tokens: usize,
    verification_passed: bool,
    errors: Vec<String>,
    expected_match: f64,  // 0.0–1.0 match ratio
}

struct EvaluationMetrics {
    tokens_per_step: f64,
    error_rate: f64,
    mean_expected_match: f64,
}
```

#### Workflow

1. Define scenarios in JSONL format — each line is a `ReplayScenario`.
2. Run evaluation: `oco eval scenarios.jsonl --output results.json`
3. The evaluator executes each scenario with the `stub` LLM provider, recording all decisions and observations.
4. Results include per-scenario metrics and aggregate `EvaluationMetrics`.

#### Determinism

Replay uses the `stub` provider by default to ensure deterministic LLM responses. Config overrides allow testing specific policy settings (e.g., different budget limits, decay half-lives).

**Located in:** `crates/shared-types/src/replay.rs`, `crates/orchestrator-core/src/eval.rs`

### 3.6 Repo Profiles

**Problem:** v1 required manual configuration of build/test/lint commands per repository. Users had to duplicate information already present in manifests.

**Solution:** Auto-detect repository characteristics from manifests and merge with user overrides.

#### Data Model

```rust
// shared-types/profile.rs

struct RepoProfile {
    name: String,
    stack: Vec<StackComponent>,  // Rust, TypeScript, Python, etc.
    build_cmd: Option<String>,
    test_cmd: Option<String>,
    lint_cmd: Option<String>,
    typecheck_cmd: Option<String>,
    sensitive_paths: Vec<PathBuf>,   // .env, credentials, etc.
    high_value_paths: Vec<PathBuf>,  // src/main, core modules
    risk_level: RiskLevel,           // Low, Medium, High
}

enum StackComponent {
    Rust { edition: String },
    TypeScript { runtime: String },
    Python { version: String },
    Go { version: String },
}
```

#### Detection

The profiler scans the workspace root for manifests:

| Manifest | Detected Stack | Default Commands |
|----------|---------------|-----------------|
| `Cargo.toml` | Rust | `cargo build`, `cargo test`, `cargo clippy` |
| `package.json` | TypeScript/JavaScript | reads `scripts` field |
| `pyproject.toml` | Python | `pytest`, `mypy`, `ruff` |
| `go.mod` | Go | `go build`, `go test`, `golangci-lint` |

#### Merge Strategy

1. Auto-detect from manifests → base profile.
2. Read `[profile]` section from `oco.toml` → override specific fields.
3. CLI flags → highest priority overrides.

This ensures zero-config for standard projects while allowing full customization.

**Located in:** `crates/shared-types/src/profile.rs`, `crates/orchestrator-core/src/config.rs`

### 3.7 Plugin Health

**Problem:** Misconfigured plugin installations silently fail, leading to confusing behavior.

**Solution:** A diagnostic command that validates the full plugin stack.

#### `oco doctor`

Checks performed:

| Check | Pass | Warn | Fail |
|-------|------|------|------|
| `oco.toml` exists | Present and valid | Present but has warnings | Missing or invalid TOML |
| `.oco/` directory | Exists with expected structure | Exists but incomplete | Missing |
| `.claude/` directory | Exists with hooks/skills/agents | Partial setup | Missing |
| Hook registration | All hooks registered in settings | Some hooks missing | Settings file missing |
| MCP server config | Server registered and reachable | Registered but unreachable | Not registered |
| Skill files | All skills present and valid | Some skills have issues | Skills directory missing |
| Index freshness | Index exists and recent | Index exists but stale | No index |

Output is a structured report with pass/warn/fail per check, plus actionable remediation steps for any non-pass result.

**Located in:** `apps/dev-cli/src/main.rs`

## 4. Crate Dependency Order (updated)

The v2 dependency graph maintains v1's layered structure. New modules are added within existing crates rather than introducing new crates, keeping the dependency tree flat.

```
1.  shared-types          Domain types (Session, Action, Observation, Budget,
                          Context, Verification, Memory, Telemetry, Replay, Profile)
2.  shared-proto          Protobuf definitions
3.  policy-engine          Deterministic action selection, budget enforcement,
                          task classification, freshness-aware gating
4.  code-intel             Tree-sitter / regex parser, symbol indexer
5.  retrieval              SQLite FTS5, in-memory vector, hybrid RRF
6.  tool-runtime           Shell/file executors, observation normalizer
7.  verifier               Test/build/lint/typecheck runners, auto-detection
8.  telemetry              Tracing init, typed event collection, JSONL export
9.  context-engine         Assembly with staleness decay, category budgets,
                          dedup, compression, token estimation
10. orchestrator-core      State machine, action loop, working memory,
                          verification tracking, eval runner, LLM providers
11. mcp-server             Axum HTTP/MCP server, session management
12. dev-cli                CLI binary (index, search, run, serve, eval, doctor)
```

## 5. Test Coverage

110+ tests across all crates, covering both v1 and v2 functionality:

| Crate | Tests | v2 additions |
|-------|-------|-------------|
| `shared-types` | — | Types are tested via consuming crates |
| `policy-engine` | 30 | Freshness-aware gating (2 tests) |
| `code-intel` | 16 | — |
| `retrieval` | 9 | — |
| `context-engine` | 15 | Staleness decay (1), category budgets (1) |
| `orchestrator-core` | 25+ | Verification state transitions (5), working memory lifecycle (4), eval scenario loading (2), repo profile detection (3) |
| `telemetry` | 4 | Typed event recording (2) |
| `verifier` | 8 | — |
| `mcp-server` | 3+ | — |

Run all tests:

```bash
cargo test
```

Run v2-specific tests:

```bash
cargo test verification
cargo test working_memory
cargo test staleness
cargo test category_budget
cargo test telemetry_event
cargo test scenario
cargo test repo_profile
```

## 6. Configuration

### `oco.toml`

```toml
[orchestrator]
max_steps = 25
token_budget = 100000
time_budget_secs = 300
llm_provider = "stub"

[context]
staleness_half_life = 5

[context.category_budgets]
search_results = 8000
tool_outputs = 6000
verification = 4000
memory = 3000
user_context = 4000

[profile]
build_cmd = "cargo build"
test_cmd = "cargo test"
lint_cmd = "cargo clippy -- -D warnings"
typecheck_cmd = "cargo check"
sensitive_paths = [".env", "secrets/"]
high_value_paths = ["crates/orchestrator-core/", "crates/policy-engine/"]
risk_level = "medium"

[telemetry]
trace_dir = ".oco/traces"
export_format = "jsonl"
```

The `[profile]` section is optional. When absent, the profiler auto-detects from manifests. When present, specified fields override detection results.

## 7. Backwards Compatibility

All v1 behavior is preserved. v2 features are additive — they activate when state is present and degrade gracefully when absent:

- **Verification integrity** — When no `VerificationState` exists, the system behaves as v1 (no freshness tracking). The first verification run initializes state.
- **Working memory** — When memory is empty, no summary is injected into context. The loop runs identically to v1.
- **Staleness decay** — When `with_staleness()` is not called, all items retain their base relevance score.
- **Category budgets** — When `with_category_budgets()` is not called, the assembler uses the global token budget without per-category caps.
- **Telemetry events** — v2 event types are recorded alongside v1 decision traces. Consumers that only read v1 trace format ignore v2 events.
- **Replay/eval** — The eval subsystem is opt-in via CLI. It has no effect on normal orchestration.
- **Repo profiles** — When no profile is detected or configured, the system uses explicit commands from `oco.toml` or falls back to the verifier's auto-detection (v1 behavior).
