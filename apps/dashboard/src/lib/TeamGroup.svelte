<script lang="ts">
  /**
   * TeamGroup — background container node wrapping teammate nodes.
   * Shows team name + topology, and a mini-feed of recent messages.
   */
  let { data }: { data: Record<string, unknown> } = $props()

  let label = $derived(data.label as string)
  let groupWidth = $derived((data.width as number) ?? 300)
  let groupHeight = $derived((data.height as number) ?? 200)
  let messages = $derived((data.messages as Array<{ from: string; to: string; summary: string }>) ?? [])
  let messageCount = $derived((data.messageCount as number) ?? 0)
</script>

<div
  class="team-group"
  style="width: {groupWidth}px; height: {groupHeight}px;"
>
  <!-- Header: team name + message count -->
  <div class="team-header">
    <span class="team-label">{label}</span>
    {#if messageCount > 0}
      <span class="team-msg-count">{messageCount} msg{messageCount > 1 ? 's' : ''}</span>
    {/if}
  </div>

  <!-- Mini message feed (last 3 messages) — anchored at bottom -->
  {#if messages.length > 0}
    <div class="team-feed">
      {#each messages.slice(-3) as msg, i (i)}
        <div class="team-msg">
          <span class="team-msg-from">{msg.from}</span>
          <span class="team-msg-arrow">→</span>
          <span class="team-msg-to">{msg.to}</span>
          <span class="team-msg-text">{msg.summary}</span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .team-group {
    border: 1px dashed #a78bfa35;
    border-radius: 14px;
    background: rgba(167, 139, 250, 0.02);
    position: relative;
    overflow: hidden;
  }

  .team-header {
    position: absolute;
    top: 8px;
    left: 12px;
    right: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .team-label {
    font-family: ui-monospace, monospace;
    font-size: 10px;
    letter-spacing: 0.08em;
    color: #a78bfa;
    opacity: 0.45;
    text-transform: uppercase;
  }
  .team-msg-count {
    font-family: ui-monospace, monospace;
    font-size: 9px;
    color: #a78bfa;
    opacity: 0.3;
    margin-left: auto;
  }

  .team-feed {
    position: absolute;
    left: 10px;
    right: 10px;
    bottom: 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    pointer-events: none;
    background: rgba(13, 15, 20, 0.5);
    border-radius: 6px;
    padding: 4px 6px;
  }
  .team-msg {
    display: flex;
    align-items: center;
    gap: 4px;
    font-family: ui-monospace, monospace;
    font-size: 8px;
    color: #5c6378;
    opacity: 0.5;
    white-space: nowrap;
    overflow: hidden;
    animation: msgSlideIn 0.5s cubic-bezier(0.4, 0, 0.2, 1) forwards;
  }
  @keyframes msgSlideIn {
    0% { opacity: 0; transform: translateY(4px); }
    100% { opacity: 0.5; transform: translateY(0); }
  }
  .team-msg-from {
    color: #a78bfa;
    opacity: 0.6;
    flex-shrink: 0;
    max-width: 60px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .team-msg-arrow {
    color: #5c6378;
    opacity: 0.3;
    flex-shrink: 0;
  }
  .team-msg-to {
    color: #a78bfa;
    opacity: 0.6;
    flex-shrink: 0;
    max-width: 60px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .team-msg-text {
    color: #5c6378;
    opacity: 0.4;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  :global(.team-group .svelte-flow__handle) {
    display: none !important;
  }
</style>
