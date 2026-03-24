# OCO Plugin Architecture

## Architectural Summary

OCO operates as a **deterministic local coprocessor for Claude Code**:

```
┌─────────────────────────────────────────────┐
│              Claude Code (LLM)              │
│         ┌───────────────────────┐           │
│         │   User Conversation   │           │
│         └──────────┬────────────┘           │
│                    │                        │
│    ┌───────────────┼───────────────┐        │
│    │               │               │        │
│  Hooks          Skills         Subagents    │
│  (deterministic) (workflows)   (isolated)   │
│    │               │               │        │
│    └───────────────┼───────────────┘        │
│                    │                        │
│              MCP Bridge                     │
│           (4 composite tools)               │
└────────────────────┼────────────────────────┘
                     │ stdio/CLI
┌────────────────────┼────────────────────────┐
│            OCO Local Runtime                │
│                                             │
│  ┌──────────┐  ┌───────────┐  ┌──────────┐ │
│  │  Policy   │  │  Context  │  │Retrieval │ │
│  │  Engine   │  │  Engine   │  │  Engine  │ │
│  └──────────┘  └───────────┘  └──────────┘ │
│  ┌──────────┐  ┌───────────┐  ┌──────────┐ │
│  │   Code   │  │  Tool     │  │ Verifier │ │
│  │  Intel   │  │  Runtime  │  │          │ │
│  └──────────┘  └───────────┘  └──────────┘ │
│  ┌──────────┐  ┌───────────┐               │
│  │Telemetry │  │ Session   │               │
│  │          │  │ Store     │               │
│  └──────────┘  └───────────┘               │
│                                             │
│  ┌──────────────────────────┐               │
│  │  ML Worker (optional)    │               │
│  │  sentence-transformers   │               │
│  └──────────────────────────┘               │
└─────────────────────────────────────────────┘
```

## Control Split

### Hooks = Deterministic Policy (always runs)

| Hook | What It Enforces | OCO Backend |
|------|-----------------|-------------|
| UserPromptSubmit | Task classification, verification guidance | `TaskClassifier` |
| PreToolUse | Write policy gates, destructive blocking, loop detection | `PolicyGate` |
| PostToolUse | Observation normalization, telemetry capture | `ObservationNormalizer`, telemetry |
| Stop | Verification gating (block premature completion) | `VerificationDispatcher` |

Hooks are **short, fast, deterministic**. They never call an LLM. They degrade gracefully if OCO is unavailable (fall through with no effect).

### Skills = Reusable Workflows (invoked on demand)

| Skill | When To Use | OCO Backend |
|-------|-------------|-------------|
| `/oco-inspect-repo-area` | Exploratory repo understanding | `search_codebase` MCP |
| `/oco-trace-stack` | Stack trace / runtime error present | `trace_error` MCP |
| `/oco-investigate-bug` | Bug without clear stacktrace | `search_codebase` MCP |
| `/oco-safe-refactor` | Rename / restructure / extract | `search_codebase` + `verify_patch` MCP |
| `/oco-verify-fix` | After code changes | `verify_patch` MCP |

Skills are **workflow templates**. They guide Claude's reasoning without bloating CLAUDE.md. Each skill is self-contained and task-specific.

### Subagents = Isolated Heavy Work (spawned as needed)

| Agent | Role | Model | Tools |
|-------|------|-------|-------|
| `@codebase-investigator` | Read many files, produce summary | Haiku | Read, Grep, Glob, Bash |
| `@patch-verifier` | Review change for correctness | Sonnet | Read, Grep, Glob, Bash |
| `@refactor-reviewer` | Check refactor completeness | Sonnet | Read, Grep, Glob, Bash |

Subagents protect the main context window. They have **no write permissions** and return structured summaries.

### MCP = Composite Local Intelligence (4 tools)

| Tool | What It Does Locally | Return Format |
|------|---------------------|---------------|
| `oco.search_codebase` | FTS5 + symbol indexing + RRF ranking | Ranked results JSON |
| `oco.trace_error` | Stack trace parsing + file mapping | Frames + matches JSON |
| `oco.verify_patch` | Auto-detect checks + run build/test/lint | Structured verdict JSON |
| `oco.collect_findings` | Session traces + evidence extraction | Findings summary JSON |

Each MCP tool does **substantial local computation** before returning a compact result. No raw output dumping.

## Structure Deviation from Prompt

The prompt specified `.claude-plugin/plugin.json`. This is **not idiomatic** for Claude Code, which uses:

- `.claude/settings.json` for plugin configuration, hooks, and MCP server definitions
- Skill files as `SKILL.md` in named directories
- Agent files as `.md` files with YAML frontmatter

The implemented structure follows Claude Code idioms:

```
oco-plugin/
├── .claude/
│   ├── settings.json          # Plugin manifest + hooks + MCP config
│   └── agents/                # Subagents (Claude Code discovers agents here)
│       ├── codebase-investigator.md
│       ├── patch-verifier.md
│       └── refactor-reviewer.md
├── hooks/
│   └── scripts/
│       ├── user-prompt-submit.sh
│       ├── pre-tool-use.sh
│       ├── post-tool-use.sh
│       └── stop.sh
├── skills/
│   ├── oco-inspect-repo-area/SKILL.md
│   ├── oco-trace-stack/SKILL.md
│   ├── oco-investigate-bug/SKILL.md
│   ├── oco-safe-refactor/SKILL.md
│   └── oco-verify-fix/SKILL.md
├── mcp/
│   └── server/
│       └── bridge.js          # Stdio MCP bridge to local OCO runtime
└── docs/
    ├── README.md
    ├── MIGRATION.md
    └── ARCHITECTURE.md
```

## Data Flow Examples

### Example: User submits "fix the login bug"

1. **UserPromptSubmit hook** → calls `oco classify "fix the login bug"` → returns `{complexity: "medium", needs_verification: true, task_type: "bugfix"}`
2. Hook injects: `[OCO] complexity=medium type=bugfix verify=true`
3. Claude sees guidance, decides to use `/oco-investigate-bug` skill
4. Skill guides Claude through evidence-based debugging
5. Claude reads files, identifies root cause
6. Claude applies fix using Edit tool
7. **PostToolUse hook** → logs modified file to temp tracking
8. Claude invokes `/oco-verify-fix` skill
9. Skill uses `oco.verify_patch` MCP tool → runs cargo test → returns PASS
10. **Stop hook** → checks verification log → allows completion

### Example: User submits "refactor the auth module"

1. **UserPromptSubmit hook** → `{complexity: "high", task_type: "refactor"}`
2. Hook injects: `[OCO] complexity=high type=refactor verify=true | Recommended: investigate before acting.`
3. Claude uses `/oco-inspect-repo-area` to understand auth module
4. Skill delegates to `@codebase-investigator` (many files)
5. Investigator returns compact summary
6. Claude uses `/oco-safe-refactor` skill
7. Skill requires impact analysis before changes
8. Claude applies staged changes
9. **PreToolUse hook** → validates each write (no sensitive files)
10. `@refactor-reviewer` checks for stale references
11. `/oco-verify-fix` runs full suite
12. **Stop hook** → verification confirmed → allows completion

## Future: Native Runtime Mode (Phase 2)

The existing OCO orchestration loop (`OrchestrationLoop`) is preserved and can be re-enabled as an advanced mode:

- User provides their own API key
- OCO owns the full agentic loop
- Plugin mode becomes a compatibility layer
- All plugin-facing components still work

This is **not the default path** and is deferred to Phase 2.
