<script lang="ts">
  let { run, onSelect }: {
    run: { id: string; request: string; complexity: string; steps: number; duration_ms: number; success: boolean; created_at: string; run_dir: string; tokens_used: number; tokens_max: number }
    onSelect: (runDir: string) => void
  } = $props()

  const complexityRank: Record<string, number> = {
    Trivial: 1, Low: 2, Medium: 3, High: 4, Critical: 5,
  }

  function formatDuration(ms: number): string {
    if (ms < 1000) return `${ms}ms`
    if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
    return `${Math.floor(ms / 60000)}m ${Math.floor((ms % 60000) / 1000)}s`
  }

  function timeAgo(dateStr: string): string {
    if (!dateStr) return ''
    const diff = Date.now() - new Date(dateStr).getTime()
    const mins = Math.floor(diff / 60000)
    if (mins < 1) return 'just now'
    if (mins < 60) return `${mins}m ago`
    const hrs = Math.floor(mins / 60)
    if (hrs < 24) return `${hrs}h ago`
    return `${Math.floor(hrs / 24)}d ago`
  }

  let rank = $derived(complexityRank[run.complexity] ?? 0)
</script>

<button
  onclick={() => onSelect(run.run_dir)}
  class="w-full text-left bg-surface-2 border border-border hover:border-border-bright transition-all group cursor-pointer"
>
  <!-- Status bar top -->
  <div class="h-0.5 {run.success ? 'bg-success' : 'bg-error'}"></div>

  <div class="p-3">
    <!-- Top row: status + time -->
    <div class="flex items-center justify-between mb-2">
      <div class="flex items-center gap-2">
        <div class="pip {run.success ? 'pip-success' : 'pip-error'}"></div>
        <span class="text-[10px] font-mono uppercase tracking-wider {run.success ? 'text-success' : 'text-error'}">
          {run.success ? 'COMPLETE' : 'FAILED'}
        </span>
      </div>
      <span class="text-[10px] font-mono text-text-dim">{timeAgo(run.created_at)}</span>
    </div>

    <!-- Mission name -->
    <p class="text-xs text-text-bright font-medium mb-3 line-clamp-2 leading-relaxed group-hover:text-text-white transition-colors">
      {run.request || '—'}
    </p>

    <!-- Stats row -->
    <div class="flex items-center gap-4 text-[10px] font-mono text-text-dim">
      <div class="flex items-center gap-1.5">
        <span class="text-text-dim">STEPS</span>
        <span class="text-text">{run.steps}</span>
      </div>
      <div class="flex items-center gap-1.5">
        <span class="text-text-dim">TIME</span>
        <span class="text-text">{formatDuration(run.duration_ms)}</span>
      </div>
      <div class="flex items-center gap-1.5">
        <span class="text-text-dim">RANK</span>
        <!-- Complexity pips -->
        <div class="flex gap-0.5">
          {#each Array(5) as _, i}
            <div class="w-1.5 h-3 {i < rank ? (rank >= 4 ? 'bg-warning' : 'bg-accent') : 'bg-surface'}"></div>
          {/each}
        </div>
      </div>
    </div>
  </div>
</button>
