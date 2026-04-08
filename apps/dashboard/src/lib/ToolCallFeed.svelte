<script lang="ts">
  /**
   * ToolCallFeed — live stream of tool invocations from Claude Code.
   *
   * Each `tool_call_started` event opens a row. The matching
   * `tool_call_completed` (keyed by stepId + toolName order) closes it
   * with success/fail badge + duration + output summary.
   *
   * Limited to the MAX_ROWS most recent entries so long runs don't
   * drown the panel.
   */

  import type { ToolCallRow } from './event-player'

  let { calls = [] }: { calls?: ToolCallRow[] } = $props()

  // Tool category coloring. Keep palette muted & readable on dark bg.
  const TOOL_COLORS: Record<string, { fg: string; bg: string; icon: string }> = {
    Read:     { fg: '#22d3ee', bg: 'rgba(34,211,238,0.10)',  icon: '▤' },
    Grep:     { fg: '#a78bfa', bg: 'rgba(167,139,250,0.10)', icon: '⌕' },
    Glob:     { fg: '#a78bfa', bg: 'rgba(167,139,250,0.10)', icon: '✱' },
    Edit:     { fg: '#fbbf24', bg: 'rgba(251,191,36,0.10)',  icon: '✎' },
    Write:    { fg: '#fbbf24', bg: 'rgba(251,191,36,0.10)',  icon: '✎' },
    Bash:     { fg: '#34d399', bg: 'rgba(52,211,153,0.10)',  icon: '❯' },
    WebFetch: { fg: '#60a5fa', bg: 'rgba(96,165,250,0.10)',  icon: '↗' },
    WebSearch:{ fg: '#60a5fa', bg: 'rgba(96,165,250,0.10)',  icon: '↗' },
    Task:     { fg: '#f472b6', bg: 'rgba(244,114,182,0.10)', icon: '◈' },
  }
  function toolStyle(name: string) {
    return TOOL_COLORS[name] ?? { fg: '#8890a4', bg: 'rgba(136,144,164,0.08)', icon: '⬢' }
  }
</script>

<div class="feed">
  <div class="feed-header">
    <span class="feed-title">TOOL CALLS</span>
    <span class="feed-count">{calls.length}</span>
  </div>
  <div class="feed-body">
    {#if calls.length === 0}
      <div class="empty">No tool calls yet.</div>
    {:else}
      {#each calls as call (call.id)}
        {@const style = toolStyle(call.toolName)}
        <div class="row" class:row-running={call.status === 'running'} class:row-failed={call.status === 'failed'}>
          <span class="badge" style="color:{style.fg};background:{style.bg};border-color:{style.fg}40">
            <span class="icon">{style.icon}</span>{call.toolName}
          </span>
          <span class="args" title={call.argsSummary}>{call.argsSummary}</span>
          {#if call.status === 'running'}
            <span class="pip"></span>
          {:else if call.durationMs !== undefined}
            <span class="meta">
              {#if call.outputSummary}
                <span class="output">{call.outputSummary}</span>
              {/if}
              <span class="duration {call.status === 'failed' ? 'duration-fail' : ''}">
                {call.durationMs < 1000 ? `${call.durationMs}ms` : `${(call.durationMs / 1000).toFixed(1)}s`}
              </span>
              {#if call.status === 'failed'}
                <span class="x">✗</span>
              {/if}
            </span>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .feed {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: rgba(13, 15, 20, 0.4);
    border-radius: 6px;
    border: 1px solid #1c203040;
    font-family: ui-monospace, monospace;
    overflow: hidden;
  }

  .feed-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 10px;
    border-bottom: 1px solid #1c203040;
    font-size: 9px;
    letter-spacing: 0.15em;
    color: #8890a4;
    text-transform: uppercase;
  }
  .feed-title { font-weight: 600; }
  .feed-count { color: #5c6378; }

  .feed-body {
    flex: 1;
    overflow-y: auto;
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .empty {
    padding: 12px 10px;
    font-size: 10px;
    color: #5c6378;
    text-align: center;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 10px;
    color: #a4aabb;
    animation: row-in 0.35s ease-out both;
    transition: background 0.2s;
  }
  .row:hover { background: rgba(28,32,48,0.4); }
  @keyframes row-in {
    from { opacity: 0; transform: translateX(-4px); }
    to   { opacity: 1; transform: translateX(0); }
  }

  .row-running { background: rgba(34,211,238,0.03); }
  .row-failed  { background: rgba(248,113,113,0.05); }

  .badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 2px 7px;
    border-radius: 3px;
    border: 1px solid;
    font-size: 9px;
    font-weight: 600;
    letter-spacing: 0.05em;
    flex-shrink: 0;
  }
  .icon { font-size: 10px; line-height: 1; }

  .args {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #8890a4;
  }

  .pip {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: #22d3ee;
    box-shadow: 0 0 4px rgba(34,211,238,0.6);
    flex-shrink: 0;
    animation: pip-pulse 0.9s ease-in-out infinite;
  }
  @keyframes pip-pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50%      { opacity: 0.3; transform: scale(0.7); }
  }

  .meta {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }
  .output {
    color: #5c6378;
    font-size: 9px;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .duration {
    color: #34d399;
    font-size: 9px;
    font-weight: 600;
  }
  .duration-fail { color: #f87171; }
  .x { color: #f87171; font-weight: 700; font-size: 10px; }
</style>
