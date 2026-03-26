# Changelog

## [0.5.0] — 2026-03-26

### Added
- **Effort-level routing** — `EffortLevel` (Low/Medium/High) maps to Claude Code `--effort` flag; `LlmRouter.route_step()` returns `RoutingDecision` (model + effort) with role heuristics and budget-aware downgrade (#52)
- **Claude Code `--bare` optimization** — 14% faster subprocess startup, `CLAUDE_CODE_SUBPROCESS_ENV_SCRUB` and `CLAUDE_STREAM_IDLE_TIMEOUT_MS` env var support (#52)
- **HTTP hook endpoints** — `/api/v1/hooks/{post-tool,task-completed,file-changed,post-compact,stop}` for Claude Code v2.1.63+ event integration, with Bearer auth middleware and 64KB body limit (#53)
- **Deferred tool schema** — `CapabilityRegistry.deferred_tool_names()`, `to_tool_schemas()`, `resolve_deferred_tool()` expose OCO capabilities as Claude Code deferred tools via ToolSearch, with bijective ID encoding and sanitized descriptions (#54)
- **MCP elicitation types** — `ElicitationRequest`/`ElicitationResponse` for interactive orchestration decisions (replan, verify gate, ambiguity) via Claude Code MCP elicitation v2.1.76+ (#55)
- **Agent Teams executor scaffold** — `AgentTeamsExecutor` maps PlanSteps to Claude Code Agent Teams with async lifecycle (`RwLock`), worktree isolation heuristic, typed errors, `TeammateStatus` state machine (#56)
- **Benchmark suite** — `examples/benchmark-v0.5.jsonl` with 17 scenarios across 8 tiers (baseline, budget, effort routing, hooks, capabilities, elicitation, agent teams, integration)

### Fixed
- **Eval pipeline** — `run_with_plan()` now pushes `OrchestratorAction::Respond` with plan output content; flat loop enriches `Respond` action with LLM content before recording; `PlanningContext` uses session budget instead of complexity default; CLI writes full `ScenarioResult` instead of reduced `EvaluationMetrics` (#57)
- **Zero-limit budget guards** — `max_retrievals: 0` or `max_verify_cycles: 0` no longer kills the entire session; only the specific action type is blocked (#57)

## [0.4.0] — 2026-03-26

### Added
- **MCP bridge wired**: `.mcp.json` registers `oco-bridge` server, exposing 4 composite tools (`oco.search_codebase`, `oco.trace_error`, `oco.verify_patch`, `oco.collect_findings`)
- Bridge renamed to `.cjs` to fix ESM/CJS conflict with `"type": "module"` in `package.json`

### Changed
- Skills now use MCP tools as primary with built-in fallback (Grep/Read/Glob)
- Pre-tool-use hook simplified: removed dead `oco gate-check` call, removed incorrect yoyo nudging
- Single source of truth for skills: `.claude/skills/` only, stale copies in `plugin/` and `oco-plugin/` removed

### Fixed
- MCP bridge was coded but never configured — tools were invisible to Claude Code since initial migration
- ACCEPTANCE.md updated to reflect the wiring gap

## [0.3.6] — 2026-03-25

### Fixed
- Stop hook false positives on non-source files: exclude `.sh`, `.bash`, `.zsh`, `Makefile`, `Dockerfile`, `.env`, `.lock`, `.gitignore`, `.editorconfig`, `.prettierrc`, `.eslintrc`

## [0.3.5] — 2026-03-25

### Fixed
- Verify detection rewritten with regex patterns instead of prefix matching — now covers all package managers (npm, pnpm, yarn, bun), monorepo filters (`--filter`), and additional runners (mocha, ava, make)
- Eliminates the "false positive loop" where `pnpm build`/`pnpm type-check` were not recognized as verification

## [0.3.4] — 2026-03-25

### Fixed
- Stop hook cross-project false positives: filter modified files by git root (Windows backslash-safe)
- Verification detection: add `npx vitest`, `playwright test`, `jest`, `cargo fmt`, `dotnet test/build` to recognized commands
- Fix `require()` in ESM: replace `require('node:child_process')` with proper import in post-tool-use hook

## [0.3.3] — 2026-03-25

### Fixed
- Cross-platform cache dir: use `%LOCALAPPDATA%\oco` on Windows, `~/.cache/oco` on Linux/Mac
- Remove dead code in `findProjectRoot` (unused URL parsing)
- Fix cache path in all 8 hook files (plugin/ + .claude/) for Windows compatibility

## [0.3.2] — 2026-03-25

### Fixed
- Skill descriptions rewritten with "Auto-activates when..." pattern for Claude Code auto-trigger (#30)

## [0.3.1] — 2026-03-25

### Fixed
- CLI installer robust across environments (Windows/macOS/Linux path handling) (#29)

## [0.3.0] — 2026-03-25

### Added
- **Observable plans**: `ExecutionPlan` DAG with `ready_steps()`, `parallel_groups()`, `critical_path_length()`
- **Step enforcement**: `GraphRunner` with parallel execution, verify gates, budget pre-reservation
- **Replan budget guard**: max 3 replan attempts, 5% budget per call
- **Semantic plan validation**: `validate_semantic()` checks dep coherence, role consistency
- **Multi-model routing**: `LlmRouter` per-step model selection (opus/sonnet/haiku)
- **Step-scoped context**: `StepContextBuilder` with dependency outputs, error context (Manus pattern)

### Changed
- Medium+ tasks now get unique ExecutionPlan DAG instead of flat loop
- Planner generates plans from task + repo context + available capabilities

## [0.2.0] — 2026-03-24

### Added
- **Event-driven CLI**: `UiEvent` enum (26 variants) + `Renderer` trait decouples business events from presentation
- **Three renderers**: Terminal (colors, spinners, icons), JSONL (machine-readable), Quiet (final results only)
- **Live orchestration trace**: `OrchestrationEvent` streamed via `mpsc` channel, rendered in real-time via `tokio::spawn`
- **Run artifacts**: every `oco run` persists `trace.jsonl` + `summary.json` to `.oco/runs/<id>/`
- **`oco runs show <id|last>`**: replay a past run's trace from artifacts
- **`oco runs list`**: list recent runs with status, steps, duration
- **`--format jsonl`**: structured JSONL output on stdout for all commands
- **`--quiet`**: suppress all output except final result and errors
- **Tracing log redirection**: in human mode, logs go to `.oco/oco.log` keeping the terminal clean
- **`OrchestrationEvent`** enum in `shared-types` (StepCompleted, BudgetWarning, Stopped, IndexProgress)
- **`with_event_channel()`** on `OrchestrationLoop` for live event streaming
- **`index_workspace_with_progress()`** callback for future progress bar support

### Changed
- `FtsIndex` wraps `Connection` in `Mutex` for `Send+Sync` (enables `tokio::spawn` for the loop)
- `TelemetryConfig` gains `log_to_file` and `quiet` fields
- `run()` wrapper guarantees `Stopped` event emission even on early errors
- Run success derived from terminal `Stop { TaskComplete }` action (not `session.status`)
- Artifact save is non-fatal (warning on failure)

### Dependencies
- Added: `indicatif` 0.17, `console` 0.15

## [0.1.0] — 2026-03-20

Initial release: orchestration loop, policy engine, code-intel, retrieval, verifier, telemetry, MCP server, dev CLI.
