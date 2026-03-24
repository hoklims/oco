# Claude Code Integration

This directory provides project-specific tooling for [Claude Code](https://claude.com/claude-code).

## What's included

| Component | Purpose | Requires OCO binary? |
|---|---|---|
| **hooks/** | Safety gates: blocks destructive commands, protects sensitive files, detects loops, enforces verification before completion | No |
| **skills/** | 5 slash commands: `/oco-inspect-repo-area`, `/oco-investigate-bug`, `/oco-safe-refactor`, `/oco-trace-stack`, `/oco-verify-fix` | No |
| **agents/** | 3 specialized agents: `codebase-investigator`, `patch-verifier`, `refactor-reviewer` | No |
| **mcp/bridge.js** | MCP server exposing OCO composite tools (search, trace, verify, findings) | Yes |
| **settings.json** | Hooks wiring, MCP server config, permissions | No |

## Setup

**Zero config required.** Clone the repo and open it with Claude Code — hooks, skills, and agents activate automatically.

The MCP bridge provides enhanced capabilities when the `oco` binary is available but degrades gracefully without it. All safety hooks work with Node.js only (no external dependencies).

### Optional: full OCO integration

```bash
# Build and install the OCO CLI
cargo install --path apps/dev-cli

# Index the workspace (enables MCP search/trace tools)
oco index .
```

## Requirements

- **Node.js 20+** (for hooks — uses `node:*` built-in modules only)
- **Claude Code** (reads `.claude/` automatically)
- **OCO binary** (optional — for MCP bridge features)
