<script lang="ts">
  import type { DashboardEvent } from './types'

  let { events, maxVisible = 200 }: { events: DashboardEvent[]; maxVisible?: number } = $props()

  let filter = $state('')
  let autoScroll = $state(true)
  let logEl: HTMLDivElement | undefined = $state()

  const typeColors: Record<string, string> = {
    run_started: 'text-info',
    run_stopped: 'text-warning',
    plan_generated: 'text-accent',
    step_started: 'text-running',
    step_completed: 'text-success',
    flat_step_completed: 'text-success',
    progress: 'text-text-dim',
    verify_gate_result: 'text-warning',
    replan_triggered: 'text-error',
    budget_warning: 'text-error',
    budget_snapshot: 'text-text-dim',
    index_progress: 'text-text-dim',
    heartbeat: 'text-text-dim',
  }

  let filtered = $derived.by(() => {
    const lower = filter.toLowerCase()
    const all = lower
      ? events.filter(e => {
          const kind = e.kind as Record<string, unknown>
          return kind.type?.toString().includes(lower)
            || JSON.stringify(kind).toLowerCase().includes(lower)
        })
      : events
    return all.slice(-maxVisible)
  })

  function summary(kind: Record<string, unknown>): string {
    const type = kind.type as string
    switch (type) {
      case 'step_started': return `${kind.step_name} (${kind.role})`
      case 'step_completed': return `${kind.step_name} ${kind.success ? '✓' : '✗'} ${kind.duration_ms}ms`
      case 'flat_step_completed': return `#${kind.step} ${kind.action_type} ${kind.duration_ms}ms`
      case 'plan_generated': return `${kind.step_count} steps, ${kind.strategy}`
      case 'progress': return `${kind.completed}/${kind.total}`
      case 'verify_gate_result': return `${kind.step_name} ${kind.overall_passed ? 'PASS' : 'FAIL'}`
      case 'replan_triggered': return `${kind.failed_step_name} attempt ${kind.attempt}/${kind.max_attempts}`
      case 'budget_warning': return `${kind.resource} ${Math.round((kind.utilization as number) * 100)}%`
      case 'run_started': return `${kind.model} (${kind.provider})`
      case 'run_stopped': return `${kind.total_steps} steps, ${kind.total_tokens} tok`
      default: return ''
    }
  }

  $effect(() => {
    if (autoScroll && logEl && filtered.length > 0) {
      logEl.scrollTop = logEl.scrollHeight
    }
  })
</script>

<div class="flex flex-col h-full">
  <div class="flex items-center gap-2 px-3 py-2 border-b border-border">
    <span class="text-text-dim text-xs">/</span>
    <input
      type="text"
      bind:value={filter}
      placeholder="Filter events..."
      class="flex-1 bg-transparent text-sm text-text outline-none placeholder:text-text-dim"
    />
    <label class="flex items-center gap-1 text-xs text-text-dim cursor-pointer">
      <input type="checkbox" bind:checked={autoScroll} class="accent-accent" />
      auto-scroll
    </label>
    <span class="text-xs text-text-dim">{events.length} events</span>
  </div>

  <div bind:this={logEl} class="flex-1 overflow-y-auto font-mono text-xs leading-5">
    {#each filtered as event (event.seq)}
      {@const kind = event.kind as Record<string, unknown>}
      {@const type = kind.type as string}
      <div class="flex gap-2 px-3 py-0.5 hover:bg-surface-3/50 border-b border-border/30">
        <span class="text-text-dim w-8 shrink-0 text-right">{event.seq}</span>
        <span class="text-text-dim w-20 shrink-0">{new Date(event.ts).toLocaleTimeString()}</span>
        <span class="{typeColors[type] ?? 'text-text'} w-28 shrink-0 truncate">{type}</span>
        <span class="text-text truncate">{summary(kind)}</span>
      </div>
    {/each}
  </div>
</div>
