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
    return respondStructured(id, {
      summary: "OCO backend unavailable",
      evidence: [],
      risks: ["Search results may be incomplete without OCO indexing"],
      next_step: "Use standard search tools (Grep, Glob) as fallback",
      confidence: 0.0,
    });
  }

  let parsed = [];
  try { parsed = JSON.parse(result.stdout); } catch { /* keep empty */ }
  const results = Array.isArray(parsed) ? parsed : (parsed.results || []);

  return respondStructured(id, {
    summary: `Found ${results.length} result(s) for "${args.query}"`,
    evidence: results.slice(0, limit),
    risks: [],
    next_step: results.length > 0
      ? "Review top results and inspect relevant files"
      : "Broaden search query or try different keywords",
    confidence: results.length > 0 ? 0.8 : 0.2,
  });
}

async function traceError(id, args) {
  const workspace = args.workspace || WORKSPACE;

  // Parse stack trace to extract file paths and line numbers
  const frames = parseStackTrace(args.stacktrace);

  if (frames.length === 0) {
    return respondStructured(id, {
      summary: "Could not parse stack trace",
      evidence: [],
      risks: ["Raw error output may need manual analysis"],
      next_step: "Provide the full raw error output for better parsing",
      confidence: 0.1,
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

  const deepestFrame = frames[frames.length - 1];
  const matchedFiles = results.filter((r) => r.matches && (Array.isArray(r.matches) ? r.matches.length > 0 : true));

  return respondStructured(id, {
    summary: `Parsed ${frames.length} frame(s) across ${fileSet.length} file(s). ${matchedFiles.length} matched in codebase.`,
    evidence: [
      { parsed_frames: frames },
      { codebase_matches: results },
    ],
    risks: matchedFiles.length === 0
      ? ["No stack frames matched local files — error may originate in dependencies"]
      : [],
    next_step: deepestFrame
      ? `Inspect ${deepestFrame.file}:${deepestFrame.line} — deepest application frame`
      : "Review the stack trace manually",
    confidence: matchedFiles.length > 0 ? 0.7 : 0.3,
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
  const failedChecks = Object.entries(verdicts).filter(([, v]) => v.status === "fail").map(([k]) => k);
  const passedChecks = Object.entries(verdicts).filter(([, v]) => v.status === "pass").map(([k]) => k);

  return respondStructured(id, {
    summary: `Verification ${verdict}: ${passedChecks.length} passed, ${failedChecks.length} failed, ${entries.length - passedChecks.length - failedChecks.length} skipped`,
    evidence: [{ verdict, checks: verdicts }],
    risks: hasFail
      ? failedChecks.map((c) => `${c} failed — see output for details`)
      : allSkipped
        ? ["No verification commands detected for this workspace"]
        : [],
    next_step: hasFail
      ? `Fix ${failedChecks[0]} errors first, then re-verify`
      : allSkipped
        ? "Configure build/test/lint commands or verify manually"
        : "All checks passed — safe to proceed",
    confidence: hasFail ? 0.9 : allSkipped ? 0.1 : 1.0,
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
    return respondStructured(id, {
      summary: "No OCO session data available",
      evidence: [],
      risks: ["Session trace unavailable — investigation state unknown"],
      next_step: "Use standard investigation tools to gather evidence",
      confidence: 0.0,
    });
  }

  let trace = [];
  try { trace = JSON.parse(result.stdout); } catch { /* keep empty */ }
  const traceEntries = Array.isArray(trace) ? trace : [trace];
  const errors = traceEntries.filter((t) => t.decision_type === "error" || t.error);
  const decisions = traceEntries.filter((t) => t.reasoning);

  return respondStructured(id, {
    summary: `Session ${sessionId}: ${traceEntries.length} trace entries, ${errors.length} error(s), ${decisions.length} decision(s)`,
    evidence: traceEntries,
    risks: errors.map((e) => e.error || e.reasoning || "Unknown error in trace"),
    next_step: errors.length > 0
      ? "Investigate unresolved errors in the session trace"
      : "Review decisions for correctness and proceed",
    confidence: errors.length === 0 ? 0.8 : 0.5,
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

/**
 * Wrap a structured response in MCP format.
 * All tools return: { summary, evidence, risks, next_step, confidence }
 */
function respondStructured(id, payload) {
  return success(id, {
    content: [{ type: "text", text: JSON.stringify(payload) }],
  });
}

function success(id, result) {
  return { jsonrpc: "2.0", id, result };
}

function error(id, code, message) {
  return { jsonrpc: "2.0", id, error: { code, message } };
}
