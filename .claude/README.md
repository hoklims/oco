# Claude Code Integration

This directory provides project-specific tooling for [Claude Code](https://claude.com/claude-code).

## What's included

| Component | Purpose | Requires OCO binary? |
|---|---|---|
| **hooks/** | Safety gates: blocks destructive commands, protects sensitive files, detects loops, enforces verification before completion | No |
| **skills/** | 5 slash commands: `/oco-inspect-repo-area`, `/oco-investigate-bug`, `/oco-safe-refactor`, `/oco-trace-stack`, `/oco-verify-fix` | No |
| **agents/** | 3 specialized agents: `codebase-investigator`, `patch-verifier`, `refactor-reviewer` | No |
| **mcp/bridge.cjs** | MCP server exposing OCO composite tools (search, trace, verify, findings) | Yes |
| **managed-settings.d/50-oco.json** | Hooks wiring, MCP server config, permissions | No |

## Install

```bash
npx oco-claude-plugin install            # project-level (recommended)
npx oco-claude-plugin install --global   # all projects (~/.claude/)
npx oco-claude-plugin install --force    # overwrite existing files
```

## Diagnostics

```bash
npx oco-claude-plugin doctor             # check installation health
npx oco-claude-plugin repair --dry-run   # preview fixes
npx oco-claude-plugin repair             # restore missing files
```

## Operating modes

| Mode | Condition | What works |
|---|---|---|
| **full** | hooks + settings + bridge + `oco` binary | Everything: hooks, skills, agents, MCP tools |
| **plugin** | hooks + settings + bridge, no `oco` binary | Hooks, skills, agents. MCP tools return task packets. |
| **degraded** | Some components missing | Partial functionality |
| **broken** | Settings or hooks missing | Plugin will not load |

Run `doctor` to see your current mode.

## Source of truth

**`plugin/`** (in repo root) is the canonical source for all plugin files. During `install`, files are copied from `plugin/` to the target `.claude/` directory. Never edit `.claude/hooks/` directly in a project — edit `plugin/` and re-install with `--force`.

## Requirements

- **Node.js 18+** (for hooks — uses `node:*` built-in modules only)
- **Claude Code** (reads `.claude/` automatically)
- **Claude Code >= 2.1.83** (for managed-settings.d support; falls back to settings.json merge)
- **OCO binary** (optional — `cargo install --path apps/dev-cli`)
