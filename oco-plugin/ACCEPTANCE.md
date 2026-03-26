# OCO Plugin Migration — Acceptance Checklist

## Core Requirements

- [x] OCO can be installed and used as a Claude Code plugin
- [x] Baseline usage does not require direct API billing beyond Claude Code
- [x] Hooks enforce deterministic policy where appropriate
- [x] Skills provide structured workflows with low context overhead
- [x] Subagents isolate heavy reading/review tasks
- [x] MCP surface is small and composite (4 tools)
- [x] Plugin mode is more adoptable than the previous runtime-first default
- [x] Native runtime path still remains possible for future advanced mode

## Core OCO Strengths Preserved

- [x] Deterministic policy engine (`TaskClassifier`, `PolicyGate`, `BudgetEnforcer`)
- [x] Structured observations (normalized via PostToolUse hook → `oco observe`)
- [x] Budget-aware context assembly (`ContextAssembler`, `TokenEstimator`)
- [x] Decision traces (captured via telemetry, exposed via `oco.collect_findings`)
- [x] Verification discipline (Stop hook gating + `oco-verify-fix` skill + PostToolUse auto-detection)
- [x] Graceful degradation (all hooks/MCP degrade silently without OCO binary)

## Plugin Components Delivered

### Hooks (Phase 3)
- [x] UserPromptSubmit — task classification via `oco classify`, compact JSON guidance (jq-safe output)
- [x] PreToolUse — destructive command blocking, sensitive file protection, session-stable loop detection
- [x] PostToolUse — telemetry capture, modified file tracking, verification auto-detection
- [x] Stop — verification gating (blocks completion if files modified without build/test/lint run)

### Skills (Phase 4)
- [x] `oco-inspect-repo-area` — structured repo exploration with `oco.search_codebase`
- [x] `oco-trace-stack` — stack trace analysis with `oco.trace_error` + `oco.collect_findings`
- [x] `oco-investigate-bug` — evidence-based debugging with `@patch-verifier` delegation
- [x] `oco-safe-refactor` — impact analysis + staged refactoring with `@refactor-reviewer` + `@patch-verifier`
- [x] `oco-verify-fix` — structured verification suite with `oco.verify_patch`

### Subagents (Phase 5)
- [x] `@codebase-investigator` — isolated reading, Haiku model, read-only (`.claude/agents/`)
- [x] `@patch-verifier` — change review, Sonnet model, read-only (`.claude/agents/`)
- [x] `@refactor-reviewer` — refactor validation, Sonnet model, read-only (`.claude/agents/`)

### MCP (Phase 6)
- [x] `oco.search_codebase` — composite search via `oco search` CLI
- [x] `oco.trace_error` — composite error analysis with local stack parsing
- [x] `oco.verify_patch` — composite verification with project auto-detection
- [x] `oco.collect_findings` — composite state/evidence extraction via `oco trace`

> **Note**: Bridge code (`bridge.js`) was implemented but not wired into `.mcp.json` until #36.
> The ESM/CJS conflict (`package.json` has `"type": "module"`) required renaming to `bridge.cjs`.

### CLI Extensions (Phase 7)
- [x] `oco classify` — calls `TaskClassifier::classify()`, JSON output
- [x] `oco gate-check` — calls `PolicyGate::evaluate()`, JSON output
- [x] `oco observe` — records to `.oco/observations.jsonl`

### Documentation (Phase 8)
- [x] Plugin README
- [x] Migration plan (MIGRATION.md)
- [x] Architecture summary (ARCHITECTURE.md)
- [x] Acceptance checklist (this file)
- [x] Deferred items list (DEFERRED.md)

## Design Rules Compliance

- [x] Prefer hooks over MCP when deterministic automation is enough
- [x] Prefer skills over giant CLAUDE.md text dumps
- [x] Prefer subagents over polluting main session with broad reading
- [x] Prefer composite MCP tools over many tiny tools
- [x] Plugin outputs are compact and structured
- [x] Claude's main context stays clean
- [x] Auditability preserved (decision traces, telemetry)
- [x] Local-first execution (no cloud dependencies beyond Claude Code)
- [x] Provider-agnostic backend (stub, anthropic, ollama all preserved)
- [x] No critical policy behavior hidden in vague prose

## Hard Constraints Verified

- [x] Core Rust runtime and domain logic preserved (zero changes to crates/)
- [x] No OCO logic rewritten into TypeScript (bridge.js is only protocol glue)
- [x] Anthropic API not mandatory (hooks and skills work without it)
- [x] MCP surface is small (4 composite tools, not fine-grained)
- [x] No giant always-on CLAUDE.md blob (skills are on-demand)
- [x] No assumption of full control over Claude Code's reasoning
- [x] Future native runtime ownership still possible

## Known Limitations (P2)

- [ ] `oco.verify_patch` in bridge.js re-implements project detection instead of calling `oco verify` CLI (verifier crate declared but not routed)
- [ ] `oco.collect_findings` requires `oco serve` running for session traces; degrades to empty findings otherwise
- [ ] Hook paths in settings.json are relative to project root — requires plugin directory at project level
- [ ] No automated tests for hook scripts or bridge.js protocol compliance

## Audit Trail

- **Initial migration**: All 8 phases completed
- **Compliance audit**: Identified 3 P0 bugs (PID cross-hook state, MCP server config, agent path), 4 P1 issues
- **Remediation**: All P0 and P1 issues fixed
- **v0.3.6 audit**: Bridge was coded but never wired — `.mcp.json` was missing. Fixed in #36. ESM conflict fixed (bridge.cjs). Stale skill copies in `plugin/` and `oco-plugin/` removed (#38).
  - Hooks now use workspace-hash session ID (`md5sum $PWD`) for stable cross-hook state
  - MCP server config updated to use `node bridge.js` (working stdio transport)
  - Agents moved to `.claude/agents/` for Claude Code discoverability
  - PostToolUse auto-detects verification commands and writes verify marker
  - All JSON output uses `jq -n` for safe escaping
  - Cross-skill references use natural language instead of broken `/` syntax
  - `@patch-verifier` referenced by `oco-investigate-bug` and `oco-safe-refactor`
  - `oco.collect_findings` referenced by `oco-trace-stack` and `oco-investigate-bug`
