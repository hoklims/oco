<script lang="ts">
  /**
   * Lobby — mission select screen. Shows recent runs as a clean list,
   * not cards. Click to replay and enter the mission view.
   */
  let { runs, speed, onSelect, onSpeedChange }: {
    runs: Array<{
      id: string; request: string; complexity: string; steps: number
      duration_ms: number; success: boolean; created_at: string; run_dir: string
    }>
    speed: number
    onSelect: (runDir: string) => void
    onSpeedChange: (s: number) => void
  } = $props()

  const complexityPips: Record<string, number> = {
    Trivial: 1, Low: 2, Medium: 3, High: 4, Critical: 5,
  }

  function timeAgo(dateStr: string): string {
    if (!dateStr) return ''
    const diff = Date.now() - new Date(dateStr).getTime()
    const mins = Math.floor(diff / 60000)
    if (mins < 1) return 'now'
    if (mins < 60) return `${mins}m`
    const hrs = Math.floor(mins / 60)
    if (hrs < 24) return `${hrs}h`
    return `${Math.floor(hrs / 24)}d`
  }

  function fmtDuration(ms: number): string {
    if (ms < 1000) return `${ms}ms`
    if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
    return `${Math.floor(ms / 60000)}m${Math.floor((ms % 60000) / 1000)}s`
  }
</script>

<div class="min-h-screen bg-bg">
  <!-- Header -->
  <header class="border-b border-border bg-surface sticky top-0 z-10">
    <div class="max-w-4xl mx-auto px-6 py-4 flex items-center justify-between">
      <div>
        <div class="text-[10px] font-mono text-text-3 uppercase tracking-[0.25em]">OCO</div>
        <div class="text-sm text-text-1 font-medium mt-0.5">Mission Control</div>
      </div>
      <div class="flex items-center gap-3">
        <span class="text-[10px] font-mono text-text-3 uppercase">Replay speed</span>
        {#each [1, 5, 10, 50] as s}
          <button
            onclick={() => onSpeedChange(s)}
            class="text-[10px] font-mono px-2 py-1 transition-colors {speed === s ? 'bg-blue text-white' : 'text-text-3 bg-surface-2 hover:bg-surface-3'}"
          >{s}x</button>
        {/each}
      </div>
    </div>
  </header>

  <!-- Run list -->
  <main class="max-w-4xl mx-auto px-6 py-2">
    <!-- Column headers -->
    <div class="flex items-center px-3 py-2 text-[10px] font-mono text-text-3 uppercase tracking-wider border-b border-border">
      <span class="w-8"></span>
      <span class="flex-1">Mission</span>
      <span class="w-16 text-center">Rank</span>
      <span class="w-14 text-right">Steps</span>
      <span class="w-16 text-right">Time</span>
      <span class="w-10 text-right">Age</span>
    </div>

    {#each runs as run (run.id)}
      {@const rank = complexityPips[run.complexity] ?? 0}
      <button
        onclick={() => onSelect(run.run_dir)}
        class="w-full flex items-center px-3 py-2.5 border-b border-border/40 hover:bg-surface-hover transition-colors text-left cursor-pointer group"
      >
        <!-- Status pip -->
        <div class="w-8 shrink-0">
          <div class="pip {run.success ? 'pip-done' : 'pip-fail'}"></div>
        </div>

        <!-- Request -->
        <div class="flex-1 min-w-0 pr-4">
          <div class="text-xs text-text-2 truncate group-hover:text-text-1 transition-colors">{run.request || '—'}</div>
        </div>

        <!-- Complexity pips -->
        <div class="w-16 flex justify-center gap-0.5 shrink-0">
          {#each Array(5) as _, i}
            <div class="w-1 h-3 {i < rank ? (rank >= 4 ? 'bg-amber' : 'bg-blue') : 'bg-surface-2'}"></div>
          {/each}
        </div>

        <!-- Steps -->
        <div class="w-14 text-right shrink-0">
          <span class="text-xs font-mono text-text-2">{run.steps}</span>
        </div>

        <!-- Duration -->
        <div class="w-16 text-right shrink-0">
          <span class="text-[10px] font-mono text-text-3">{fmtDuration(run.duration_ms)}</span>
        </div>

        <!-- Age -->
        <div class="w-10 text-right shrink-0">
          <span class="text-[10px] font-mono text-text-3">{timeAgo(run.created_at)}</span>
        </div>
      </button>
    {/each}

    {#if runs.length === 0}
      <div class="py-16 text-center">
        <div class="text-text-3 text-xs mb-1">No missions recorded</div>
        <div class="text-[10px] font-mono text-text-3">
          Run <span class="text-blue">oco run "task"</span> to begin
        </div>
      </div>
    {/if}
  </main>
</div>
