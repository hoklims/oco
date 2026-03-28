# OCO — Claude Code Plugin

**Open Context Orchestrator** as a Claude Code plugin. Deterministic code intelligence, policy enforcement, and structured workflows — without requiring a separate API key.

## What OCO Does

OCO is a local coprocessor that makes Claude Code smarter about your codebase:

- **Hooks** enforce safety rules (block destructive commands, require verification)
- **Skills** provide structured workflows (bug investigation, refactoring, verification)
- **Subagents** isolate heavy reading tasks (codebase investigation, change review)
- **MCP tools** expose composite code intelligence (search, error tracing, verification)

## Quick Start

### Prerequisites

- [Claude Code](https://claude.ai/claude-code) installed
- Node.js >= 18

### Install the plugin

```bash
npx oco-claude-plugin install          # project-level
npx oco-claude-plugin install --global # all projects
```

This installs hooks, skills, agents, and the MCP bridge into `.claude/`.
**It does not install the OCO runtime binary.**

### What works immediately (plugin-only mode)

- Safety hooks (destructive command blocking, verification enforcement)
- 5 structured skills (`/oco-inspect-repo-area`, `/oco-verify-fix`, etc.)
- 3 specialized agents (`codebase-investigator`, `patch-verifier`, `refactor-reviewer`)
- MCP `verify_patch` (runs cargo/npm directly, no binary needed)
- MCP `working_memory` (local file persistence, no binary needed)

### Optional: install the runtime for full mode

The OCO runtime enables indexed codebase search, stack trace mapping,
task delegation, and session trace collection via MCP tools.

```bash
# Requires Rust toolchain (https://rustup.rs) and access to the OCO source repo
cd /path/to/oco
cargo install --path apps/dev-cli

# Verify
oco --version
```

### Check installation

```bash
npx oco-claude-plugin doctor
```

## Usage

### Skills (invoke with `/`)

| Skill | When to Use |
|-------|-------------|
| `/oco-inspect-repo-area` | Explore and understand codebase areas |
| `/oco-trace-stack` | Analyze stack traces and runtime errors |
| `/oco-investigate-bug` | Systematic bug investigation |
| `/oco-safe-refactor` | Safe refactoring with impact analysis |
| `/oco-verify-fix` | Verify changes (build, test, lint, types) |

### Subagents (invoked by skills automatically)

| Agent | Role |
|-------|------|
| `@codebase-investigator` | Read many files, return compact summary |
| `@patch-verifier` | Review changes for correctness |
| `@refactor-reviewer` | Check refactor completeness |

### Hooks (automatic, no user action needed)

| Hook | What It Does |
|------|-------------|
| UserPromptSubmit | Classifies task, injects minimal guidance |
| PreToolUse | Blocks destructive/risky operations |
| PostToolUse | Captures telemetry, tracks modifications |
| Stop | Requires verification before completion |

### MCP Tools (used by skills, also callable directly)

| Tool | Purpose |
|------|---------|
| `oco.search_codebase` | Symbol-aware codebase search |
| `oco.trace_error` | Stack trace analysis |
| `oco.verify_patch` | Run project verification suite |
| `oco.collect_findings` | Summarize investigation state |

## Configuration

Edit `.claude/settings.json` to customize:

```jsonc
{
  "hooks": {
    // Modify hook behavior
  },
  "mcpServers": {
    "oco": {
      // Configure MCP bridge
      "env": {
        "OCO_BIN": "/custom/path/to/oco"
      }
    }
  }
}
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OCO_BIN` | `oco` | Path to OCO binary |
| `OCO_WORKSPACE` | `$PWD` | Default workspace root |

## Plugin-only vs Full Mode

| Feature | Plugin-only | Full (with runtime) |
|---------|:-----------:|:-------------------:|
| Safety hooks | ✓ | ✓ |
| Skills (/oco-*) | ✓ | ✓ |
| Agents (@codebase-investigator, etc.) | ✓ | ✓ |
| MCP verify_patch | ✓ | ✓ |
| MCP working_memory | ✓ | ✓ |
| MCP search_codebase (indexed search) | fallback | ✓ |
| MCP trace_error (stack mapping) | fallback | ✓ |
| MCP begin_task (task delegation) | fallback | ✓ |
| MCP collect_findings (session traces) | fallback | ✓ |

**Fallback** means the tool returns a structured response explaining the runtime
is not installed, with a suggestion to use standard Claude Code tools instead.

## Architecture

See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full design.

## License

Apache 2.0
