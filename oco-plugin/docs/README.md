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
- Rust toolchain (for building OCO backend)

### Install

```bash
# Build OCO backend
cd /path/to/supertools
cargo build --release

# Add oco binary to PATH
export PATH="$PATH:/path/to/supertools/target/release"

# Install plugin (copy to your project or link globally)
cp -r oco-plugin/.claude/ ~/.claude/  # Global install
# OR
cp -r oco-plugin/.claude/ .claude/    # Project-level install
```

### Verify

```bash
# Check OCO binary is available
oco --help

# Start Claude Code — hooks and skills are now active
claude
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

## Graceful Degradation

All plugin components degrade gracefully if the OCO backend is unavailable:

- **Hooks**: Skip classification/gating, fall through silently
- **Skills**: Work with standard Claude Code tools instead of OCO-backed search
- **MCP tools**: Return empty results with a note to use standard tools
- **Subagents**: Always work (they use standard Claude Code tools)

## Architecture

See [ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full design.

## License

Apache 2.0
