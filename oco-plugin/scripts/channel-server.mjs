#!/usr/bin/env node
/**
 * OCO Channel Server — Push orchestration events into active Claude sessions.
 *
 * Declares the `claude/channel` capability so OCO can proactively inject
 * plan progress, verify results, and alerts into Claude's context.
 *
 * Requires: Claude Code >= 2.1.80, authenticated via claude.ai.
 *
 * Events pushed:
 *   - PlanStepCompleted (success/failure)
 *   - VerifyGateResult (pass/fail)
 *   - BudgetWarning
 *   - ReplanTriggered
 *   - Stopped (task complete)
 *
 * Transport: stdio MCP (Claude Code spawns this process)
 * Backend: connects to oco serve SSE stream for live events
 */

import { createInterface } from "node:readline";
import http from "node:http";

const OCO_SERVER = process.env.OCO_SERVER_URL || "http://localhost:3000";
const CHANNEL_NAME = "oco-events";

// MCP protocol handler
const rl = createInterface({ input: process.stdin });

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
  const { id, method } = request;

  switch (method) {
    case "initialize":
      return success(id, {
        protocolVersion: "2024-11-05",
        serverInfo: { name: CHANNEL_NAME, version: "0.13.0" },
        capabilities: {
          experimental: {
            "claude/channel": {},
          },
          tools: { listChanged: false },
        },
        instructions:
          "OCO orchestration events. Alerts indicate failures or budget warnings that need attention.",
      });

    case "tools/list":
      return success(id, { tools: [] });

    case "notifications/initialized":
      // Start listening for OCO events and pushing to channel
      startEventStream();
      return null; // notifications don't get responses

    default:
      return success(id, {});
  }
}

/**
 * Connect to oco serve SSE stream and push relevant events to the channel.
 */
function startEventStream() {
  const url = `${OCO_SERVER}/api/v1/events`;

  const req = http.get(url, (res) => {
    if (res.statusCode !== 200) {
      // oco serve not running — silently degrade
      return;
    }

    let buffer = "";
    res.on("data", (chunk) => {
      buffer += chunk.toString();
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";

      for (const line of lines) {
        if (!line.startsWith("data: ")) continue;
        try {
          const event = JSON.parse(line.slice(6));
          const message = formatChannelMessage(event);
          if (message) {
            pushToChannel(message);
          }
        } catch {
          // skip malformed events
        }
      }
    });
  });

  req.on("error", () => {
    // oco serve not running — silently degrade
  });
}

/**
 * Map OrchestrationEvent to a channel message string.
 * Returns null for events that shouldn't be pushed.
 */
function formatChannelMessage(event) {
  switch (event.type) {
    case "step_completed":
    case "plan_step_completed":
      if (event.success === false) {
        return `[OCO] Step "${event.step_name || "unknown"}" FAILED: ${event.detail || "see trace"}`;
      }
      if (event.step_name) {
        return `[OCO] Step "${event.step_name}" completed (${event.tokens_used || 0} tokens)`;
      }
      return null;

    case "verify_gate_result":
      if (!event.overall_passed) {
        const failed = (event.checks || [])
          .filter((c) => !c.passed)
          .map((c) => c.check_type)
          .join(", ");
        return `[OCO] Verify gate FAILED: ${failed}`;
      }
      return `[OCO] Verify gate passed`;

    case "budget_warning":
      return `[OCO] Budget warning: ${event.resource} at ${Math.round((event.utilization || 0) * 100)}%`;

    case "replan_triggered":
      return `[OCO] Replanning after "${event.failed_step_name}" (attempt ${event.attempt}/${event.max_attempts})`;

    case "run_stopped":
      return `[OCO] Task ${event.reason === "task_complete" ? "completed" : "stopped"}: ${event.total_steps} steps, ${event.total_tokens} tokens`;

    default:
      return null;
  }
}

/**
 * Push a message to the Claude channel.
 */
function pushToChannel(message) {
  const notification = {
    jsonrpc: "2.0",
    method: "notifications/claude/channel/message",
    params: {
      channel: CHANNEL_NAME,
      message: { type: "text", text: message },
    },
  };
  process.stdout.write(JSON.stringify(notification) + "\n");
}

function success(id, result) {
  return { jsonrpc: "2.0", id, result };
}
