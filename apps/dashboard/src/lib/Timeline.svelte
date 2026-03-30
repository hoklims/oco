<script lang="ts">
  import type { DashboardEvent } from './types'

  let { events, selectedSeq, onSelect }: {
    events: DashboardEvent[]
    selectedSeq: number | null
    onSelect: (seq: number) => void
  } = $props()

  let autoScroll = $state(true)
  let el: HTMLDivElement | undefined = $state()

  function narrative(kind: Record<string, unknown>): { label: string; tag: string; color: string } {
    const type = kind.type as string
    switch (type) {
      case 'flat_step_completed': {
        const action = kind.action_type as string
        const ms = kind.duration_ms as number
        if (action.startsWith('retrieve')) return { label: `Scanned codebase`, tag: `${ms}ms`, color: 'text-blue' }
        if (action.startsWith('tool:')) return { label: action.replace('tool:', ''), tag: `${ms}ms`, color: 'text-purple' }
        if (action.startsWith('verify:')) return { label: action.replace('verify:', '') + ' check', tag: `${ms}ms`, color: 'text-cyan' }
        if (action === 'respond') return { label: 'Response formulated', tag: '', color: 'text-text-2' }
        if (action.startsWith('stop:')) return { label: 'Terminated', tag: '', color: 'text-text-3' }
        return { label: action, tag: `${ms}ms`, color: 'text-text-2' }
      }
      case 'step_started': return { label: `${kind.step_name}`, tag: (kind.role as string).toUpperCase(), color: 'text-cyan' }
      case 'step_completed': {
        const ok = kind.success as boolean
        return { label: `${kind.step_name}`, tag: `${((kind.duration_ms as number) / 1000).toFixed(1)}s`, color: ok ? 'text-green' : 'text-red' }
      }
      case 'plan_generated': return { label: `Plan: ${kind.step_count} steps`, tag: kind.strategy as string, color: 'text-blue' }
      case 'progress': return { label: `${kind.completed}/${kind.total} done`, tag: '', color: 'text-text-3' }
      case 'verify_gate_result': {
        const ok = kind.overall_passed as boolean
        return { label: `Gate: ${kind.step_name}`, tag: ok ? 'PASS' : 'FAIL', color: ok ? 'text-green' : 'text-red' }
      }
      case 'replan_triggered': return { label: `Replan #${kind.attempt}`, tag: `+${kind.steps_added} -${kind.steps_removed}`, color: 'text-amber' }
      case 'budget_warning': return { label: `${kind.resource}`, tag: `${Math.round((kind.utilization as number) * 100)}%`, color: 'text-amber' }
      case 'run_started': return { label: `${kind.model}`, tag: kind.provider as string, color: 'text-blue' }
      case 'run_stopped': return { label: `${kind.total_steps} steps`, tag: `${(kind.total_tokens as number).toLocaleString()} tok`, color: 'text-green' }
      default: return { label: type, tag: '', color: 'text-text-3' }
    }
  }

  function pipFor(kind: Record<string, unknown>): string {
    const type = kind.type as string
    if (type === 'step_started') return 'pip-active'
    if (type === 'step_completed') return (kind.success as boolean) ? 'pip-done' : 'pip-fail'
    if (type === 'verify_gate_result') return (kind.overall_passed as boolean) ? 'pip-done' : 'pip-fail'
    if (type === 'replan_triggered' || type === 'budget_warning') return 'pip-warn'
    if (type === 'run_stopped') return 'pip-done'
    return 'pip-idle'
  }

  // Only show meaningful events (skip progress noise)
  let visible = $derived(events.filter(e => {
    const type = (e.kind as Record<string, unknown>).type as string
    return type !== 'progress'
  }))

  $effect(() => {
    if (autoScroll && el && visible.length > 0) {
      el.scrollTop = el.scrollHeight
    }
  })
</script>

<div class="flex flex-col h-full">
  <div class="flex items-center px-4 py-2 border-b border-border bg-surface shrink-0">
    <span class="text-xs font-mono text-text-3 uppercase tracking-widest flex-1">Activity</span>
    <label class="flex items-center gap-1.5 text-xs text-text-3 cursor-pointer">
      <input type="checkbox" bind:checked={autoScroll} class="accent-blue w-3.5 h-3.5" />
      follow
    </label>
  </div>

  <div bind:this={el} class="flex-1 overflow-y-auto">
    {#each visible as event (event.seq)}
      {@const kind = event.kind as Record<string, unknown>}
      {@const info = narrative(kind)}
      <button
        onclick={() => onSelect(event.seq)}
        class="w-full flex items-center gap-3 px-4 py-2.5 border-b border-border/30 hover:bg-surface-hover transition-colors text-left cursor-pointer {selectedSeq === event.seq ? 'selected' : ''}"
      >
        <div class="pip {pipFor(kind)}"></div>
        <span class="text-sm {info.color} truncate flex-1">{info.label}</span>
        {#if info.tag}
          <span class="text-xs font-mono text-text-3 shrink-0 px-2 py-0.5 bg-surface-2 rounded">{info.tag}</span>
        {/if}
      </button>
    {/each}
  </div>
</div>
