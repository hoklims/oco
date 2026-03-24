# OCO Migration Plan: Runtime-First to Plugin-First

## Migration Matrix

### Subsystems Preserved as Local Backend

| Subsystem | Crate | Plugin Role | Notes |
|-----------|-------|-------------|-------|
| Policy Engine | `oco-policy-engine` | Called by hooks (classify, gate-check) | No changes to core logic |
| Context Engine | `oco-context-engine` | Called by MCP tools (search, collect) | Assembler/dedup/compress unchanged |
| Retrieval | `oco-retrieval` | Called by MCP `oco.search_codebase` | FTS5 + vector + RRF preserved |
| Tool Runtime | `oco-tool-runtime` | Executors used by verify_patch | Shell/file executors unchanged |
| Verifier | `oco-verifier` | Called by MCP `oco.verify_patch` and Stop hook | All runners preserved |
| Telemetry | `oco-telemetry` | Called by PostToolUse hook | Traces/metrics preserved |
| Code Intel | `oco-code-intel` | Used by search and trace tools | Parser/indexer unchanged |
| Session Store | `oco-orchestrator-core` | Sessions managed by runtime | State machine preserved |
| ML Worker | `py/ml-worker` | Optional embeddings backend | No changes, remains optional |
| Shared Types | `oco-shared-types` | Used by all crates | No changes |

### Subsystems Re-exposed Through Plugin

| Component | Before (Runtime-First) | After (Plugin-First) |
|-----------|----------------------|---------------------|
| Task classification | MCP tool or internal loop | **Hook**: UserPromptSubmit calls `oco classify` |
| Write policy gates | Internal policy engine | **Hook**: PreToolUse checks destructive patterns |
| Observation capture | Internal state machine | **Hook**: PostToolUse fires `oco observe` |
| Verification gating | Internal verify cycle | **Hook**: Stop blocks until verification done |
| Repo exploration | MCP `oco_orchestrate` | **Skill**: `/oco-inspect-repo-area` |
| Error analysis | MCP `oco_orchestrate` | **Skill**: `/oco-trace-stack` |
| Bug investigation | MCP `oco_orchestrate` | **Skill**: `/oco-investigate-bug` |
| Refactoring | MCP `oco_orchestrate` | **Skill**: `/oco-safe-refactor` |
| Verification | MCP `oco_orchestrate` | **Skill**: `/oco-verify-fix` |
| Deep reading | MCP session context | **Subagent**: `@codebase-investigator` |
| Change review | Not explicit | **Subagent**: `@patch-verifier` |
| Refactor review | Not explicit | **Subagent**: `@refactor-reviewer` |
| Codebase search | MCP `oco_search` (fine-grained) | **MCP**: `oco.search_codebase` (composite) |
| Error tracing | Not available | **MCP**: `oco.trace_error` (composite) |
| Verification run | Not available | **MCP**: `oco.verify_patch` (composite) |
| Session state | MCP `oco_status` + `oco_trace` | **MCP**: `oco.collect_findings` (composite) |

### MCP Surface Reduction

| Before | After | Rationale |
|--------|-------|-----------|
| `oco_orchestrate` | Removed | Replaced by skills + hooks |
| `oco_status` | Merged into `oco.collect_findings` | Single composite endpoint |
| `oco_trace` | Merged into `oco.collect_findings` | Single composite endpoint |
| `oco_search` | Replaced by `oco.search_codebase` | Symbol-aware, ranked, compact |
| — | Added `oco.trace_error` | New composite error analysis |
| — | Added `oco.verify_patch` | New composite verification |
| **4 tools** | **4 tools** | Same count, much higher value per tool |

## Phase Execution Log

### Phase 1: Inventory and Mapping ✅
- Inspected all 12 Rust crates, Python workspace, TypeScript extension
- Mapped each subsystem to plugin-facing responsibilities
- Produced migration matrix (above)

### Phase 2: Plugin Scaffold ✅
- Created `oco-plugin/` with idiomatic Claude Code structure
- `.claude/settings.json` — plugin config, hooks, MCP server
- `hooks/scripts/` — 4 hook scripts
- `skills/` — 5 skill definitions
- `agents/` — 3 subagent definitions
- `mcp/server/` — bridge server

### Phase 3: Hook Integration ✅
- `UserPromptSubmit` — lightweight task classification via `oco classify`
- `PreToolUse` — destructive command blocking, sensitive file protection, loop detection
- `PostToolUse` — telemetry capture, modified file tracking, loop counter reset
- `Stop` — verification gating (blocks completion without build/test/lint)

### Phase 4: Skill Migration ✅
- `/oco-inspect-repo-area` — structured exploration with OCO search
- `/oco-trace-stack` — evidence-based stack trace analysis
- `/oco-investigate-bug` — root-cause-first debugging
- `/oco-safe-refactor` — impact analysis + staged changes + verification
- `/oco-verify-fix` — structured verification suite

### Phase 5: Subagent Layer ✅
- `@codebase-investigator` — isolated reading, compact summaries (Haiku model)
- `@patch-verifier` — change review with structured verdicts (Sonnet model)
- `@refactor-reviewer` — stale reference detection, impact analysis (Sonnet model)

### Phase 6: MCP Minimization ✅
- Reduced from 4 fine-grained tools to 4 composite tools
- Each tool does substantial local work before returning
- Bridge server handles graceful degradation

### Phase 7: Runtime Bridge ✅
- `bridge.js` — stdio MCP server calling local `oco` CLI
- Stable CLI boundary preferred over HTTP for plugin context
- Telemetry flows through PostToolUse hook → `oco observe`

### Phase 8: Docs and Distribution ✅
- README, MIGRATION, ARCHITECTURE docs produced
- Plugin installable by copying `oco-plugin/` into project

## CLI Extensions Required

The `oco` CLI needs these new subcommands for hook/bridge integration:

| Command | Purpose | Status |
|---------|---------|--------|
| `oco classify <prompt> --workspace <path> --format json` | Task classification for UserPromptSubmit hook | **TODO** |
| `oco gate-check --tool <name> --input <json> --format json` | Advanced policy gate for PreToolUse hook | **TODO** |
| `oco observe --tool <name> --status <ok/error> --format json` | Telemetry capture for PostToolUse hook | **TODO** |

These are thin wrappers around existing crate logic:
- `classify` → `TaskClassifier::classify()` from `oco-policy-engine`
- `gate-check` → `PolicyGate::evaluate()` from `oco-policy-engine`
- `observe` → telemetry recording from `oco-telemetry`
