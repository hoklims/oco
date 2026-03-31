#!/usr/bin/env node
/**
 * OCO SDK Runner — Agent SDK wrapper for SdkFallback mode.
 *
 * When Claude Code interactive client is unavailable (CI/CD, headless),
 * OCO uses this script to execute plan steps via the Agent SDK.
 *
 * Input:  JSON on stdin (step config: task, tools, constraints)
 * Output: JSONL on stdout (OrchestrationEvent-compatible events)
 *
 * Requires: @anthropic-ai/claude-agent-sdk (npm) and ANTHROPIC_API_KEY env var.
 *
 * Usage:
 *   echo '{"task":"fix the bug","max_tokens":50000}' | node sdk-runner.mjs
 */

import { createInterface } from "node:readline";

// Read step config from stdin
function readStdin() {
  return new Promise((resolve) => {
    let data = "";
    const rl = createInterface({ input: process.stdin });
    rl.on("line", (line) => { data += line; });
    rl.on("close", () => {
      try { resolve(JSON.parse(data)); }
      catch { resolve(null); }
    });
    setTimeout(() => { rl.close(); }, 5000);
  });
}

function emit(event) {
  process.stdout.write(JSON.stringify(event) + "\n");
}

async function main() {
  const config = await readStdin();
  if (!config || !config.task) {
    emit({ event: "error", message: "No task provided on stdin" });
    process.exit(1);
  }

  // Check for API key
  if (!process.env.ANTHROPIC_API_KEY) {
    emit({ event: "error", message: "ANTHROPIC_API_KEY not set. SDK mode requires an API key." });
    process.exit(1);
  }

  // Try to load Agent SDK
  let sdk;
  try {
    sdk = await import("@anthropic-ai/claude-agent-sdk");
  } catch {
    emit({ event: "error", message: "Agent SDK not installed. Run: npm install @anthropic-ai/claude-agent-sdk" });
    process.exit(1);
  }

  emit({ event: "sdk_started", task: config.task, mode: "sdk_fallback" });

  try {
    const agent = new sdk.Agent({
      model: config.model || "claude-sonnet-4-20250514",
      maxTokens: config.max_tokens || 50000,
      tools: config.tools || [],
    });

    const result = await agent.query(config.task);

    emit({
      event: "step_completed",
      success: true,
      output: typeof result === "string" ? result : JSON.stringify(result),
      tokens_used: result?.usage?.total_tokens || 0,
    });
  } catch (err) {
    emit({
      event: "step_completed",
      success: false,
      error: err.message || String(err),
    });
    process.exit(1);
  }
}

main().catch((err) => {
  emit({ event: "error", message: err.message || String(err) });
  process.exit(1);
});
