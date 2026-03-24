# OCO Deferred Items — Native Runtime Phase 2

## CLI Extensions

### Implemented (P0)

| Command | Wraps | Status |
|---------|-------|--------|
| `oco classify <prompt> --workspace <path> --format json` | `TaskClassifier::classify()` | **DONE** |
| `oco gate-check --tool <name> --input <json> --format json` | `PolicyGate::evaluate()` | **DONE** |
| `oco observe --tool <name> --status <ok/error> --format json` | Telemetry recording to `.oco/observations.jsonl` | **DONE** |

### Remaining (P1)

| Priority | Command | Wraps | Effort |
|----------|---------|-------|--------|
| **P1** | `oco verify --workspace <path> --checks <list> --format json` | `VerificationDispatcher` | 3h |

## Native Runtime Mode (Phase 2)

Restore full OCO orchestration loop as an opt-in advanced mode:

- [ ] Add `--mode native` flag to plugin config
- [ ] Route through `OrchestrationLoop` instead of Claude Code's loop
- [ ] Requires user-provided API key (Anthropic or Ollama)
- [ ] Skills become orchestration triggers instead of workflow templates
- [ ] Hooks become observation sources instead of standalone gates
- [ ] MCP bridge routes to full session management

## Plugin Distribution

- [ ] Publish as npm package for `claude install` workflow
- [ ] Add `package.json` with `bin` entry for MCP bridge
- [ ] Add `postinstall` script to check/build OCO binary
- [ ] Create marketplace metadata (icon, description, screenshots)
- [ ] Add plugin version pinning to `settings.json`

## Enhanced MCP Tools

- [ ] `oco.orchestrate_complex_task` — full session-backed orchestration (only if demand emerges)
- [ ] `oco.diff_analysis` — semantic diff analysis with impact scoring
- [ ] `oco.suggest_tests` — test generation suggestions based on coverage gaps

## VS Code Extension Integration

- [ ] Make VS Code extension consume plugin hooks/skills
- [ ] Add trace panel integration with `oco.collect_findings`
- [ ] Share settings between plugin and extension

## Advanced Hook Features

- [ ] Budget-aware hook behavior (adjust strictness based on remaining budget)
- [ ] Session-persistent loop detection (across tool calls, not just per-process)
- [ ] Workspace-aware policy profiles (different gates per project type)
- [ ] Custom hook chains (user-defined pre/post sequences)

## ML Worker Integration

- [ ] Add `oco.search_codebase` semantic mode (requires ML worker)
- [ ] Reranking in search results (via ml-worker)
- [ ] Embedding-based code similarity (for refactor impact)

## Testing

- [ ] Integration tests for hook scripts (mock `oco` binary)
- [ ] MCP bridge protocol compliance tests
- [ ] End-to-end plugin installation test
- [ ] Skill trigger accuracy benchmarks
- [ ] Graceful degradation verification suite
