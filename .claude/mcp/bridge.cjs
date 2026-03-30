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

const { spawn, exec } = require("child_process");
const fs = require("fs");
const http = require("http");
const path = require("path");
const readline = require("readline");

const OCO_BIN = process.env.OCO_BIN || "oco";
const WORKSPACE = process.env.OCO_WORKSPACE || process.cwd();

// --- Background Server Manager (singleton) ---

const serverManager = {
  _proc: null,
  _port: null,
  _ready: null,

  /** Lazily start `oco serve --port 0` and return the bound port. */
  async ensureRunning() {
    if (this._port) return this._port;
    if (this._ready) return this._ready;

    this._ready = new Promise((resolve, reject) => {
      const args = ["serve", "--port", "0"];
      // Resolve dashboard dist path relative to this bridge file.
      const dashboardDir = process.env.OCO_DASHBOARD_DIR ||
        path.resolve(__dirname, "../../apps/dashboard/dist");
      const proc = spawn(OCO_BIN, args, {
        stdio: ["ignore", "pipe", "pipe"],
        detached: false,
        env: { ...process.env, OCO_DASHBOARD_DIR: dashboardDir },
      });

      this._proc = proc;
      let resolved = false;

      const tryParsePort = (data) => {
        const line = data.toString();
        const match = line.match(/listening on http:\/\/[^:]+:(\d+)/);
        if (match && !resolved) {
          resolved = true;
          this._port = parseInt(match[1], 10);
          resolve(this._port);
        }
      };

      // Parse both stdout and stderr for "listening on http://HOST:PORT"
      proc.stdout.on("data", tryParsePort);
      proc.stderr.on("data", tryParsePort);

      proc.on("error", (err) => {
        if (!resolved) {
          resolved = true;
          this._ready = null;
          reject(err);
        }
      });

      proc.on("exit", () => {
        this._proc = null;
        this._port = null;
        this._ready = null;
      });

      // Timeout: if server doesn't start in 15s, give up
      setTimeout(() => {
        if (!resolved) {
          resolved = true;
          this._ready = null;
          proc.kill();
          reject(new Error("oco serve did not start within 15s"));
        }
      }, 15000);
    });

    return this._ready;
  },

  /** Stop the background server. */
  stop() {
    if (this._proc) {
      this._proc.kill();
      this._proc = null;
      this._port = null;
      this._ready = null;
    }
  },
};

// Clean up on exit
process.on("exit", () => serverManager.stop());
process.on("SIGTERM", () => { serverManager.stop(); process.exit(0); });
process.on("SIGINT", () => { serverManager.stop(); process.exit(0); });

// --- HTTP helpers for server API ---

function httpRequest(method, port, path, body) {
  return new Promise((resolve, reject) => {
    const options = {
      hostname: "127.0.0.1",
      port,
      path,
      method,
      headers: { "Content-Type": "application/json" },
    };
    const req = http.request(options, (res) => {
      let data = "";
      res.on("data", (chunk) => (data += chunk));
      res.on("end", () => {
        try { resolve({ status: res.statusCode, body: JSON.parse(data) }); }
        catch { resolve({ status: res.statusCode, body: data }); }
      });
    });
    req.on("error", reject);
    if (body) req.write(JSON.stringify(body));
    req.end();
  });
}

function openBrowser(url) {
  const platform = process.platform;
  const cmd = platform === "win32" ? "start" :
              platform === "darwin" ? "open" : "xdg-open";
  // On Windows, 'start' needs empty title for URLs with special chars
  const args = platform === "win32" ? ['""', url] : [url];
  exec(`${cmd} ${args.join(" ")}`, () => {});
}

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
  {
    name: "oco.working_memory",
    description:
      "Query or update OCO's working memory — hypotheses, verified facts, inspected areas, planner state. " +
      "Survives context compaction. Use to persist investigation state across long sessions.",
    inputSchema: {
      type: "object",
      properties: {
        action: {
          type: "string",
          enum: ["get", "add_hypothesis", "add_fact", "record_inspection", "update_plan"],
          description: "What to do with working memory",
          default: "get",
        },
        content: {
          type: "string",
          description: "Content for add_hypothesis/add_fact/record_inspection",
        },
        confidence: {
          type: "number",
          description: "Confidence level 0.0-1.0 (for add_hypothesis)",
          default: 0.5,
        },
        path: {
          type: "string",
          description: "File path (for record_inspection)",
        },
        steps: {
          type: "array",
          items: { type: "string" },
          description: "Plan steps (for update_plan)",
        },
      },
    },
  },
  {
    name: "oco.begin_task",
    description:
      "High-level delegation: hand a task to OCO for structured execution. " +
      "OCO plans, executes within constraints, verifies, and returns structured results " +
      "(patch summary + trace + verification). Use instead of calling individual tools.",
    inputSchema: {
      type: "object",
      properties: {
        task: {
          type: "string",
          description: "What needs to be done (natural language intent)",
        },
        mode: {
          type: "string",
          enum: ["delegated", "plan_only"],
          description: "delegated = plan + execute + verify; plan_only = return plan for review",
          default: "delegated",
        },
        max_steps: {
          type: "integer",
          description: "Maximum plan steps (default: 8)",
          default: 8,
        },
        verify_required: {
          type: "boolean",
          description: "Require verification before completion (default: true)",
          default: true,
        },
        workspace: {
          type: "string",
          description: "Workspace root path (defaults to cwd)",
        },
      },
      required: ["task"],
    },
  },
  {
    name: "oco.open_dashboard",
    description:
      "Open the live dashboard in the browser. Starts the OCO server if needed, " +
      "creates a session, and opens the dashboard URL. Returns immediately (non-blocking). " +
      "Call this at the START of any /oco workflow so the user sees progress live.",
    inputSchema: {
      type: "object",
      properties: {
        task: {
          type: "string",
          description: "Task description (shown in dashboard header)",
        },
        workspace: {
          type: "string",
          description: "Workspace root path (defaults to cwd)",
        },
      },
      required: ["task"],
    },
  },
  {
    name: "oco.emit_phase",
    description:
      "Push a lifecycle phase event to the live dashboard. " +
      "Call at each transition: classifying, planning, executing, verifying, complete, failed. " +
      "Requires a session_id from a prior oco.open_dashboard call.",
    inputSchema: {
      type: "object",
      properties: {
        session_id: {
          type: "string",
          description: "Session ID from oco.open_dashboard",
        },
        phase: {
          type: "string",
          enum: ["run_started", "classifying", "planning", "executing", "verifying", "complete", "failed"],
          description: "Lifecycle phase",
        },
        detail: {
          type: "string",
          description: "Optional detail (e.g. complexity, step name, error message)",
        },
      },
      required: ["session_id", "phase"],
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
      case "oco.begin_task":
        return await beginTask(id, args);
      case "oco.open_dashboard":
        return await openDashboard(id, args);
      case "oco.emit_phase":
        return await emitPhase(id, args);
      case "oco.working_memory":
        return await workingMemory(id, args);
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
    "--quiet",
  ]);

  if (result.error) {
    return respondStructured(id, {
      summary: "oco runtime not installed — indexed search unavailable",
      evidence: [],
      risks: ["The oco binary is not on PATH. Install from OCO source: cd /path/to/oco && cargo install --path apps/dev-cli"],
      next_step: "Use Grep/Glob for text search as fallback",
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
      "--quiet",
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
  const allSearchesFailed = results.length === 0 && fileSet.length > 0;

  const risks = [];
  if (allSearchesFailed) {
    risks.push("oco runtime not installed — codebase matching unavailable. Use Grep to locate frames manually.");
  } else if (matchedFiles.length === 0) {
    risks.push("No stack frames matched local files — error may originate in dependencies");
  }

  return respondStructured(id, {
    summary: `Parsed ${frames.length} frame(s) across ${fileSet.length} file(s). ${matchedFiles.length} matched in codebase.`,
    evidence: [
      { parsed_frames: frames },
      { codebase_matches: results },
    ],
    risks,
    next_step: deepestFrame
      ? `Inspect ${deepestFrame.file}:${deepestFrame.line} — deepest application frame`
      : "Review the stack trace manually",
    confidence: matchedFiles.length > 0 ? 0.7 : allSearchesFailed ? 0.2 : 0.3,
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
    "--quiet",
  ]);

  if (result.error) {
    return respondStructured(id, {
      summary: "oco runtime not installed — session traces unavailable",
      evidence: [],
      risks: ["The oco binary is not on PATH. Install from OCO source: cd /path/to/oco && cargo install --path apps/dev-cli"],
      next_step: "Use standard investigation tools to gather evidence",
      confidence: 0.0,
    });
  }

  let trace = [];
  try { trace = JSON.parse(result.stdout); } catch { /* keep empty */ }
  // Unwrap { traces: [...] } envelope if present.
  const traceEntries = Array.isArray(trace)
    ? trace
    : Array.isArray(trace?.traces)
      ? trace.traces
      : [trace];
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

async function workingMemory(id, args) {
  const action = args.action || "get";

  // Resolve memory path: explicit state dir > session fallback
  const stateDir = process.env.OCO_STATE_DIR || path.join(WORKSPACE, ".oco");
  const memoryPath = path.join(stateDir, "memory.json");

  // Load existing memory
  let memory = { hypotheses: [], verified_facts: [], inspected_areas: [], open_questions: [], plan: [] };
  try {
    if (fs.existsSync(memoryPath)) {
      memory = JSON.parse(fs.readFileSync(memoryPath, "utf8"));
    }
  } catch { /* start fresh */ }

  const now = new Date().toISOString();

  switch (action) {
    case "get": {
      return respondStructured(id, {
        summary: `Working memory: ${(memory.hypotheses || []).length} hypotheses, ${(memory.verified_facts || []).length} facts, ${(memory.inspected_areas || []).length} areas inspected`,
        evidence: [memory],
        risks: [],
        next_step: (memory.hypotheses || []).length > 0
          ? "Verify or invalidate active hypotheses"
          : "Begin investigation — add hypotheses as you explore",
        confidence: 0.9,
      });
    }

    case "add_hypothesis": {
      if (!args.content) return error(id, -32602, "content is required for add_hypothesis");
      memory.hypotheses = memory.hypotheses || [];
      memory.hypotheses.push({
        id: crypto.randomUUID ? crypto.randomUUID() : `h-${Date.now()}`,
        text: args.content,
        confidence: `${Math.round((args.confidence || 0.5) * 100)}%`,
        status: "active",
        created_at: now,
      });
      break;
    }

    case "add_fact": {
      if (!args.content) return error(id, -32602, "content is required for add_fact");
      memory.verified_facts = memory.verified_facts || [];
      memory.verified_facts.push(args.content);
      break;
    }

    case "record_inspection": {
      if (!args.path) return error(id, -32602, "path is required for record_inspection");
      memory.inspected_areas = memory.inspected_areas || [];
      const existing = memory.inspected_areas.find((a) => a === args.path || a.path === args.path);
      if (!existing) {
        memory.inspected_areas.push(args.path);
      }
      break;
    }

    case "update_plan": {
      if (!args.steps) return error(id, -32602, "steps is required for update_plan");
      memory.plan = args.steps;
      break;
    }

    default:
      return error(id, -32602, `Unknown action: ${action}`);
  }

  // Persist atomically: write to temp file then rename to avoid partial writes
  try {
    const dir = path.dirname(memoryPath);
    if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
    const tmpPath = memoryPath + `.tmp.${process.pid}`;
    fs.writeFileSync(tmpPath, JSON.stringify(memory, null, 2));
    fs.renameSync(tmpPath, memoryPath);
  } catch (err) {
    return error(id, -32000, `Failed to persist working memory: ${err.message}`);
  }

  return respondStructured(id, {
    summary: `Working memory updated (${action}): ${(memory.hypotheses || []).length} hypotheses, ${(memory.verified_facts || []).length} facts`,
    evidence: [memory],
    risks: [],
    next_step: "Continue investigation with updated working memory",
    confidence: 0.9,
  });
}

async function openDashboard(id, args) {
  const workspace = args.workspace || WORKSPACE;

  let port;
  try {
    port = await serverManager.ensureRunning();
  } catch (e) {
    return respondStructured(id, {
      summary: "Could not start OCO server for dashboard",
      evidence: [{ error: e.message }],
      risks: ["Dashboard unavailable — oco binary may not be installed"],
      next_step: "Proceed without dashboard",
      confidence: 0.0,
    });
  }

  // Create a session for tracking
  let sessionId;
  try {
    const res = await httpRequest("POST", port, "/api/v1/sessions", {
      user_request: args.task || "OCO session",
      workspace_root: workspace,
    });
    if (res.status !== 201 || !res.body?.id) {
      throw new Error(res.body?.error || "session creation failed");
    }
    sessionId = res.body.id;
  } catch (e) {
    return respondStructured(id, {
      summary: "Server running but session creation failed",
      evidence: [{ error: e.message, port }],
      risks: ["Dashboard opened without live session"],
      next_step: "Proceed without dashboard tracking",
      confidence: 0.2,
    });
  }

  // Open dashboard in browser
  const dashboardUrl = `http://127.0.0.1:${port}/dashboard?live=${sessionId}`;
  openBrowser(dashboardUrl);

  return respondStructured(id, {
    summary: `Dashboard opened at ${dashboardUrl}`,
    evidence: [{
      session_id: sessionId,
      port,
      dashboard_url: dashboardUrl,
    }],
    risks: [],
    next_step: "Use oco.emit_phase to push lifecycle updates to the dashboard",
    confidence: 1.0,
  });
}

async function emitPhase(id, args) {
  const { session_id, phase, detail } = args;

  if (!session_id) {
    return error(id, -32602, "session_id is required");
  }

  let port;
  try {
    port = await serverManager.ensureRunning();
  } catch {
    // Server not running — silently succeed (dashboard is optional)
    return respondStructured(id, {
      summary: `Phase ${phase} noted (dashboard offline)`,
      evidence: [],
      risks: [],
      next_step: "Continue",
      confidence: 1.0,
    });
  }

  // Map phase to event type
  const eventPayload = { type: phase };
  switch (phase) {
    case "run_started":
      eventPayload.type = "run_started";
      eventPayload.request_summary = detail || "";
      eventPayload.provider = "claude-code";
      eventPayload.model = "opus";
      break;
    case "classifying":
      eventPayload.type = "classifying";
      eventPayload.reason = detail || "Analyzing task complexity";
      break;
    case "planning":
      eventPayload.type = "planning";
      eventPayload.reason = detail || "Generating execution plan";
      break;
    case "executing":
      eventPayload.type = "executing";
      eventPayload.reason = detail || "Implementing changes";
      break;
    case "verifying":
      eventPayload.type = "verifying";
      eventPayload.reason = detail || "Running verification";
      break;
    case "complete":
      eventPayload.type = "run_stopped";
      eventPayload.reason = "task_complete";
      eventPayload.total_steps = 0;
      eventPayload.total_tokens = 0;
      break;
    case "failed":
      eventPayload.type = "run_stopped";
      eventPayload.reason = "error";
      eventPayload.message = detail || "Task failed";
      break;
  }

  try {
    await httpRequest("POST", port, `/api/v1/dashboard/sessions/${session_id}/events`, eventPayload);
  } catch {
    // Non-blocking — dashboard updates are best-effort
  }

  return respondStructured(id, {
    summary: `Phase: ${phase}`,
    evidence: [{ phase, detail }],
    risks: [],
    next_step: "Continue",
    confidence: 1.0,
  });
}

async function beginTask(id, args) {
  const workspace = args.workspace || WORKSPACE;
  const mode = args.mode || "delegated";
  const maxSteps = args.max_steps || 8;
  const verifyRequired = args.verify_required !== false;

  // --- Try live dashboard flow via oco serve ---
  let port;
  try {
    port = await serverManager.ensureRunning();
  } catch {
    // Server failed to start — fall back to CLI-based execution
    return beginTaskCli(id, args);
  }

  // Create session via HTTP API
  let sessionId;
  try {
    const res = await httpRequest("POST", port, "/api/v1/sessions", {
      user_request: args.task,
      workspace_root: workspace,
    });
    if (res.status !== 201 || !res.body?.id) {
      throw new Error(res.body?.error || "session creation failed");
    }
    sessionId = res.body.id;
  } catch {
    return beginTaskCli(id, args);
  }

  // Open dashboard in browser with live session
  const dashboardUrl = `http://127.0.0.1:${port}/dashboard?live=${sessionId}`;
  openBrowser(dashboardUrl);

  // Poll session until completion (check every 2s, max 120s)
  const maxPolls = 60;
  let sessionInfo = null;
  for (let i = 0; i < maxPolls; i++) {
    await new Promise((r) => setTimeout(r, 2000));
    try {
      const res = await httpRequest("GET", port, `/api/v1/sessions/${sessionId}`, null);
      sessionInfo = res.body;
      if (sessionInfo?.status && sessionInfo.status !== "Active") break;
    } catch {
      // Retry on transient errors
    }
  }

  // Fetch trace for detailed results
  let traceEntries = [];
  try {
    const res = await httpRequest("GET", port, `/api/v1/sessions/${sessionId}/trace`, null);
    if (Array.isArray(res.body)) traceEntries = res.body;
  } catch { /* ok, trace is optional */ }

  const totalTokens = sessionInfo?.tokens_used || 0;
  const totalSteps = sessionInfo?.steps || 0;
  const status = sessionInfo?.status || "Unknown";
  const isCompleted = status === "Completed";
  const isFailed = status === "Failed";

  return respondStructured(id, {
    summary: `Task ${isCompleted ? "completed" : isFailed ? "failed" : status}: ${totalSteps} step(s), ${totalTokens} tokens`,
    evidence: [
      { session: { id: sessionId, status, complexity: sessionInfo?.complexity } },
      ...(traceEntries.length > 0 ? [{ trace: traceEntries }] : []),
      { dashboard_url: dashboardUrl },
    ],
    risks: [
      ...(isFailed ? ["Session failed — check trace for details"] : []),
      ...(verifyRequired && !isCompleted ? ["Task did not complete successfully"] : []),
    ],
    next_step: isCompleted
      ? "Task completed — review dashboard for full trace"
      : isFailed
        ? "Investigate failure via dashboard trace view"
        : "Session still running — check dashboard",
    confidence: isCompleted ? 0.9 : isFailed ? 0.3 : 0.5,
  });
}

/** Fallback: run task via CLI when server is unavailable. */
async function beginTaskCli(id, args) {
  const workspace = args.workspace || WORKSPACE;
  const mode = args.mode || "delegated";
  const maxSteps = args.max_steps || 8;
  const verifyRequired = args.verify_required !== false;

  const ocoArgs = [
    "run",
    args.task,
    "--workspace", workspace,
    "--format", "jsonl",
    "--quiet",
  ];

  if (maxSteps) {
    ocoArgs.push("--max-steps", String(maxSteps));
  }

  const result = await runOco(ocoArgs);

  if (result.error) {
    return respondStructured(id, {
      summary: "oco runtime not installed — returning task plan for manual execution",
      evidence: [{
        task_packet: {
          intent: args.task,
          mode,
          constraints: { max_steps: maxSteps, verify_required: verifyRequired },
          recommended_steps: [
            "1. Investigate: search for relevant code and understand the context",
            "2. Plan: identify files to modify and potential risks",
            "3. Implement: make the changes",
            ...(verifyRequired ? ["4. Verify: run build/test/lint to confirm correctness"] : []),
          ],
        },
      }],
      risks: ["The oco binary is not on PATH. Install from OCO source: cd /path/to/oco && cargo install --path apps/dev-cli"],
      next_step: "Execute the task steps manually using standard tools",
      confidence: 0.3,
    });
  }

  const events = result.stdout
    .split("\n")
    .filter(Boolean)
    .map((line) => { try { return JSON.parse(line); } catch { return null; } })
    .filter(Boolean);

  const stepEvents = events.filter((e) => e.event === "plan_step_completed");
  const stopped = events.find((e) => e.event === "stopped");
  const planGenerated = events.find((e) => e.event === "plan_generated");
  const verifyResults = events.filter((e) => e.event === "verify_gate_result");
  const budgetWarnings = events.filter((e) => e.event === "budget_warning");

  const outputs = stepEvents
    .filter((e) => e.success)
    .map((e) => e.output || e.step_name)
    .filter(Boolean);

  const failures = stepEvents
    .filter((e) => !e.success)
    .map((e) => `${e.step_name}: failed`);

  const totalTokens = stopped?.total_tokens || 0;
  const totalSteps = stopped?.total_steps || stepEvents.length;
  const hasFailures = failures.length > 0;
  const verified = verifyResults.some((v) => v.overall_passed);

  const trace = stepEvents.map((e) => ({
    step: e.step_name,
    success: e.success,
    tokens: e.tokens_used || 0,
    duration_ms: e.duration_ms || 0,
  }));

  return respondStructured(id, {
    summary: `Task ${hasFailures ? "partially completed" : "completed"}: ${totalSteps} step(s), ${totalTokens} tokens${verified ? ", verified ✓" : verifyRequired ? ", NOT verified ✗" : ""}`,
    evidence: [
      ...(planGenerated ? [{ plan: { steps: planGenerated.steps, strategy: planGenerated.strategy } }] : []),
      { execution: { trace, outputs } },
      ...(verifyResults.length > 0 ? [{ verification: verifyResults }] : []),
      ...(failures.length > 0 ? [{ failures }] : []),
    ],
    risks: [
      ...failures,
      ...budgetWarnings.map((w) => `Budget: ${w.resource}`),
      ...(verifyRequired && !verified ? ["Verification not passed — results may be incorrect"] : []),
    ],
    next_step: hasFailures
      ? `Address failures: ${failures[0]}`
      : verifyRequired && !verified
        ? "Run verification: build/test/lint before considering complete"
        : "Task completed and verified — safe to proceed",
    confidence: hasFailures ? 0.4 : verified ? 0.95 : 0.7,
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
