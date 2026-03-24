#!/usr/bin/env node
/**
 * OCO MCP Bridge Server
 *
 * Minimal MCP server that bridges Claude Code to the local OCO runtime.
 * Exposes only composite, high-value tools.
 *
 * Transport: stdio (Claude Code spawns this process)
 * Backend: calls local `oco` CLI binary
 */

const { spawn } = require("child_process");
const readline = require("readline");

const OCO_BIN = process.env.OCO_BIN || "oco";
const WORKSPACE = process.env.OCO_WORKSPACE || process.cwd();

// --- MCP Protocol Handler ---

const rl = readline.createInterface({ input: process.stdin });
let buffer = "";

rl.on("line", (line) => {
  try {
    const request = JSON.parse(line);
    handleRequest(request).then((response) => {
      process.stdout.write(JSON.stringify(response) + "\n");
    });
  } catch {
    // Ignore malformed lines
  }
});

async function handleRequest(request) {
  const { id, method, params } = request;

  switch (method) {
    case "initialize":
      return success(id, {
        protocolVersion: "2024-11-05",
        serverInfo: { name: "oco-bridge", version: "0.1.0" },
        capabilities: {
          tools: { listChanged: false },
        },
      });

    case "tools/list":
      return success(id, { tools: TOOLS });

    case "tools/call":
      return handleToolCall(id, params.name, params.arguments || {});

    default:
      return error(id, -32601, `Method not found: ${method}`);
  }
}

// --- Tool Definitions ---

const TOOLS = [
  {
    name: "oco.search_codebase",
    description:
      "Composite codebase search: lexical + structural ranking with symbol-aware narrowing. Returns compact ranked results.",
    inputSchema: {
      type: "object",
      properties: {
        query: {
          type: "string",
          description: "Search query (natural language or symbol name)",
        },
        workspace: {
          type: "string",
          description: "Workspace root path (defaults to cwd)",
        },
        limit: {
          type: "integer",
          description: "Max results (default: 10)",
          default: 10,
        },
      },
      required: ["query"],
    },
  },
  {
    name: "oco.trace_error",
    description:
      "Composite error analysis: maps stack trace to codebase, identifies likely root cause regions, suggests next verification step.",
    inputSchema: {
      type: "object",
      properties: {
        stacktrace: {
          type: "string",
          description: "The stack trace or error output to analyze",
        },
        workspace: {
          type: "string",
          description: "Workspace root path",
        },
      },
      required: ["stacktrace"],
    },
  },
  {
    name: "oco.verify_patch",
    description:
      "Composite verification: detects project type, runs build/test/lint/typecheck, returns structured verdict.",
    inputSchema: {
      type: "object",
      properties: {
        workspace: {
          type: "string",
          description: "Workspace root path",
        },
        checks: {
          type: "array",
          items: { type: "string" },
          description:
            "Specific checks to run (build, test, lint, typecheck). Defaults to all available.",
        },
      },
    },
  },
  {
    name: "oco.collect_findings",
    description:
      "Composite state extraction: current evidence, open questions, unresolved risks, suggested next action from the OCO session.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "OCO session ID (optional, uses latest if omitted)",
        },
      },
    },
  },
];

// --- Tool Handlers ---

async function handleToolCall(id, toolName, args) {
  try {
    switch (toolName) {
      case "oco.search_codebase":
        return await searchCodebase(id, args);
      case "oco.trace_error":
        return await traceError(id, args);
      case "oco.verify_patch":
        return await verifyPatch(id, args);
      case "oco.collect_findings":
        return await collectFindings(id, args);
      default:
        return error(id, -32601, `Unknown tool: ${toolName}`);
    }
  } catch (e) {
    return success(id, {
      content: [{ type: "text", text: `Error: ${e.message}` }],
      isError: true,
    });
  }
}

async function searchCodebase(id, args) {
  const workspace = args.workspace || WORKSPACE;
  const limit = args.limit || 10;

  const result = await runOco([
    "search",
    args.query,
    "--workspace",
    workspace,
    "--limit",
    String(limit),
    "--format",
    "json",
  ]);

  if (result.error) {
    // Graceful degradation: return empty results
    return success(id, {
      content: [
        {
          type: "text",
          text: JSON.stringify({ results: [], note: "OCO backend unavailable, use standard search tools" }),
        },
      ],
    });
  }

  return success(id, {
    content: [{ type: "text", text: result.stdout }],
  });
}

async function traceError(id, args) {
  const workspace = args.workspace || WORKSPACE;

  // Parse stack trace to extract file paths and line numbers
  const frames = parseStackTrace(args.stacktrace);

  if (frames.length === 0) {
    return success(id, {
      content: [
        {
          type: "text",
          text: JSON.stringify({
            frames: [],
            note: "Could not parse stack trace. Provide the raw error output.",
          }),
        },
      ],
    });
  }

  // Search for each unique file in the stack trace
  const fileSet = [...new Set(frames.map((f) => f.file))];
  const results = [];

  for (const file of fileSet.slice(0, 5)) {
    const search = await runOco([
      "search",
      file,
      "--workspace",
      workspace,
      "--limit",
      "3",
      "--format",
      "json",
    ]);
    if (!search.error && search.stdout) {
      try {
        const parsed = JSON.parse(search.stdout);
        results.push({ file, matches: parsed });
      } catch {
        // skip
      }
    }
  }

  return success(id, {
    content: [
      {
        type: "text",
        text: JSON.stringify({
          parsed_frames: frames,
          codebase_matches: results,
          suggestion: "Inspect the deepest application frame first. Check for null access, type errors, or missing validation.",
        }),
      },
    ],
  });
}

async function verifyPatch(id, args) {
  const workspace = args.workspace || WORKSPACE;
  const checks = args.checks || ["build", "test", "lint", "typecheck"];

  const verdicts = {};

  for (const check of checks) {
    const cmd = getCheckCommand(workspace, check);
    if (!cmd) {
      verdicts[check] = { status: "skip", reason: "not available" };
      continue;
    }

    const result = await runShell(cmd.command, cmd.args, { cwd: workspace });
    const passed = result.exitCode === 0;
    verdicts[check] = {
      status: passed ? "pass" : "fail",
      // Only include output on failure to avoid leaking noisy stderr warnings
      ...(passed ? {} : { output: truncate((result.stderr + "\n" + result.stdout).trim(), 500) }),
    };

    // Stop on first failure
    if (result.exitCode !== 0) {
      break;
    }
  }

  const entries = Object.values(verdicts);
  const allSkipped = entries.every((v) => v.status === "skip");
  const hasFail = entries.some((v) => v.status === "fail");
  const verdict = hasFail ? "FAIL" : allSkipped ? "SKIP" : "PASS";

  return success(id, {
    content: [
      {
        type: "text",
        text: JSON.stringify({
          verdict,
          checks: verdicts,
          ...(allSkipped && { note: "No verification commands available for this workspace. Manual review required." }),
        }),
      },
    ],
  });
}

async function collectFindings(id, args) {
  const sessionId = args.session_id || "latest";

  const result = await runOco([
    "trace",
    sessionId,
    "--format",
    "json",
  ]);

  if (result.error) {
    return success(id, {
      content: [
        {
          type: "text",
          text: JSON.stringify({
            evidence: [],
            open_questions: [],
            risks: [],
            next_action: "No OCO session data available. Use standard investigation.",
          }),
        },
      ],
    });
  }

  return success(id, {
    content: [{ type: "text", text: result.stdout }],
  });
}

// --- Helpers ---

function parseStackTrace(text) {
  const frames = [];
  // Common patterns: file:line, file(line), at file:line:col
  const patterns = [
    /at\s+(?:\w+\s+\()?([^:(\s]+):(\d+)/g, // JS/TS: at func (file:line:col)
    /File "([^"]+)", line (\d+)/g, // Python: File "path", line N
    /([^\s]+\.rs):(\d+)/g, // Rust: file.rs:line
    /([^\s]+\.[a-z]+):(\d+)/g, // Generic: file.ext:line
  ];

  for (const pattern of patterns) {
    let match;
    while ((match = pattern.exec(text)) !== null) {
      frames.push({ file: match[1], line: parseInt(match[2], 10) });
    }
  }

  return frames;
}

function getCheckCommand(workspace, check) {
  const fs = require("fs");
  const path = require("path");

  const hasFile = (name) =>
    fs.existsSync(path.join(workspace, name));

  switch (check) {
    case "build":
      if (hasFile("Cargo.toml"))
        return { command: "cargo", args: ["build"] };
      if (hasFile("package.json"))
        return { command: "npm", args: ["run", "build"] };
      return null;
    case "test":
      if (hasFile("Cargo.toml"))
        return { command: "cargo", args: ["test"] };
      if (hasFile("package.json"))
        return { command: "npm", args: ["test"] };
      if (hasFile("pyproject.toml"))
        return { command: "pytest", args: [] };
      return null;
    case "lint":
      if (hasFile("Cargo.toml"))
        return { command: "cargo", args: ["clippy", "--", "-D", "warnings"] };
      if (hasFile("package.json"))
        return { command: "npm", args: ["run", "lint"] };
      return null;
    case "typecheck":
      if (hasFile("Cargo.toml"))
        return { command: "cargo", args: ["check"] };
      if (hasFile("tsconfig.json"))
        return { command: "npx", args: ["tsc", "--noEmit"] };
      if (hasFile("pyproject.toml"))
        return { command: "mypy", args: ["."] };
      return null;
    default:
      return null;
  }
}

function runOco(args) {
  return runShell(OCO_BIN, args, {});
}

function runShell(command, args, options) {
  return new Promise((resolve) => {
    const proc = spawn(command, args, {
      ...options,
      timeout: 30000,
      stdio: ["ignore", "pipe", "pipe"],
    });

    let stdout = "";
    let stderr = "";

    proc.stdout.on("data", (d) => (stdout += d.toString()));
    proc.stderr.on("data", (d) => (stderr += d.toString()));

    proc.on("close", (code) => {
      resolve({ exitCode: code, stdout, stderr, error: null });
    });

    proc.on("error", (err) => {
      resolve({ exitCode: -1, stdout: "", stderr: "", error: err.message });
    });
  });
}

function truncate(str, maxLen) {
  if (!str || str.length <= maxLen) return str || "";
  return str.slice(0, maxLen) + "\n... (truncated)";
}

function success(id, result) {
  return { jsonrpc: "2.0", id, result };
}

function error(id, code, message) {
  return { jsonrpc: "2.0", id, error: { code, message } };
}
