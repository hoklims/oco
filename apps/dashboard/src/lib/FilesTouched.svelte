<script lang="ts">
  /**
   * FilesTouched — compact panel of workspace changes.
   *
   * Aggregates `file_changed` events into one row per path, keeping the
   * latest change_type and accumulating lines added/removed. Ordered
   * by most recent change.
   */

  import type { FileChange } from './event-player'

  let { changes = [] }: { changes?: FileChange[] } = $props()

  interface AggRow {
    path: string
    changeType: string
    linesAdded: number
    linesRemoved: number
    touches: number
    lastSeq: number
  }

  // Aggregate per path
  let rows = $derived.by(() => {
    const map = new Map<string, AggRow>()
    for (let i = 0; i < changes.length; i++) {
      const c = changes[i]
      const existing = map.get(c.path)
      if (existing) {
        existing.linesAdded += c.linesAdded
        existing.linesRemoved += c.linesRemoved
        existing.touches += 1
        existing.lastSeq = i
        if (c.changeType === 'deleted') existing.changeType = 'deleted'
        else if (existing.changeType !== 'deleted') existing.changeType = c.changeType
      } else {
        map.set(c.path, {
          path: c.path,
          changeType: c.changeType,
          linesAdded: c.linesAdded,
          linesRemoved: c.linesRemoved,
          touches: 1,
          lastSeq: i,
        })
      }
    }
    return Array.from(map.values()).sort((a, b) => b.lastSeq - a.lastSeq)
  })

  let totalAdded = $derived(rows.reduce((s, r) => s + r.linesAdded, 0))
  let totalRemoved = $derived(rows.reduce((s, r) => s + r.linesRemoved, 0))

  function icon(t: string): string {
    return t === 'created' ? '+' : t === 'deleted' ? '−' : '~'
  }
  function changeColor(t: string): string {
    return t === 'created' ? '#34d399' : t === 'deleted' ? '#f87171' : '#fbbf24'
  }
</script>

<div class="panel">
  <div class="panel-header">
    <span class="panel-title">FILES TOUCHED</span>
    <span class="panel-count">{rows.length}</span>
  </div>

  {#if rows.length > 0}
    <div class="summary">
      <span class="added">+{totalAdded}</span>
      <span class="sep">/</span>
      <span class="removed">−{totalRemoved}</span>
    </div>
  {/if}

  <div class="panel-body">
    {#if rows.length === 0}
      <div class="empty">No files touched yet.</div>
    {:else}
      {#each rows as row (row.path)}
        <div class="file-row">
          <span class="change-icon" style="color:{changeColor(row.changeType)}">{icon(row.changeType)}</span>
          <span class="path" title={row.path}>{row.path}</span>
          <span class="diff">
            {#if row.linesAdded > 0}<span class="plus">+{row.linesAdded}</span>{/if}
            {#if row.linesRemoved > 0}<span class="minus">−{row.linesRemoved}</span>{/if}
          </span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: rgba(13, 15, 20, 0.4);
    border-radius: 6px;
    border: 1px solid #1c203040;
    font-family: ui-monospace, monospace;
    overflow: hidden;
  }

  .panel-header {
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
  .panel-title { font-weight: 600; }
  .panel-count { color: #5c6378; }

  .summary {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid #1c203040;
    font-size: 10px;
    font-weight: 600;
  }
  .added   { color: #34d399; }
  .removed { color: #f87171; }
  .sep     { color: #2a3546; }

  .panel-body {
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

  .file-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 10px;
    animation: row-in 0.3s ease-out both;
    transition: background 0.2s;
  }
  .file-row:hover { background: rgba(28,32,48,0.4); }
  @keyframes row-in {
    from { opacity: 0; transform: translateX(-4px); }
    to   { opacity: 1; transform: translateX(0); }
  }

  .change-icon {
    font-weight: 700;
    font-size: 12px;
    line-height: 1;
    width: 10px;
    text-align: center;
    flex-shrink: 0;
  }

  .path {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: #a4aabb;
    direction: rtl;
    text-align: left;
  }

  .diff {
    display: inline-flex;
    gap: 4px;
    flex-shrink: 0;
    font-size: 9px;
    font-weight: 600;
  }
  .plus  { color: #34d399; }
  .minus { color: #f87171; }
</style>
