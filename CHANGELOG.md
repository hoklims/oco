# Changelog

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
