<script lang="ts">
  import type { DashboardEvent, BudgetSnapshot, StepRow } from './types'
  import BudgetGauge from './BudgetGauge.svelte'
  import StepTable from './StepTable.svelte'

  let {
    events, steps, budget, request,
    onDisconnect, onPause, onResume, onSpeed, speed, status,
  }: {
    events: DashboardEvent[]; steps: StepRow[]; budget: BudgetSnapshot | null
    request: string; onDisconnect: () => void; onPause: () => void
    onResume: () => void; onSpeed: (s: number) => void; speed: number; status: string
  } = $props()

  let filter = $state('')
  let autoScroll = $state(true)
  let logEl: HTMLDivElement | undefined = $state()

  // Tactical narrative — no emojis, military-style log
  function narrative(kind: Record<string, unknown>): string {
    const type = kind.type as string
    switch (type) {
      case 'flat_step_completed': {
        const action = kind.action_type as string
        const ms = kind.duration_ms as number
        if (action.startsWith('retrieve')) return `RECON — scanned codebase [${ms}ms]`
        if (action.startsWith('tool:')) return `EXEC — ${action.replace('tool:', '')} [${ms}ms]`
        if (action.startsWith('verify:')) return `CHECK — ${action.replace('verify:', '')} [${ms}ms]`
        if (action === 'respond') return 'REPORT — response formulated'
        if (action.startsWith('stop:')) return 'END — mission terminated'
        return `${action.toUpperCase()} [${ms}ms]`
      }
      case 'step_started': return `DEPLOY — ${kind.step_name} [${(kind.role as string).toUpperCase()}]`
      case 'step_completed': {
        const ok = kind.success as boolean
        return `${ok ? 'DONE' : 'FAIL'} — ${kind.step_name} [${kind.duration_ms}ms / ${(kind.tokens_used as number).toLocaleString()} tok]`
      }
      case 'plan_generated': return `PLAN — ${kind.step_count} objectives, strategy: ${kind.strategy}`
      case 'progress': return `SITREP — ${kind.completed}/${kind.total} objectives complete`
      case 'verify_gate_result': {
        const ok = kind.overall_passed as boolean
        return `GATE ${ok ? 'CLEAR' : 'BLOCKED'} — ${kind.step_name}${kind.replan_triggered ? ' > REPLAN' : ''}`
      }
      case 'replan_triggered': return `REPLAN #${kind.attempt}/${kind.max_attempts} — +${kind.steps_added} -${kind.steps_removed} objectives`
      case 'budget_warning': return `WARNING — ${kind.resource} at ${Math.round((kind.utilization as number) * 100)}%`
      case 'run_started': return `INIT — ${kind.model} / ${kind.provider}`
      case 'run_stopped': return `DEBRIEF — ${kind.total_steps} steps, ${(kind.total_tokens as number).toLocaleString()} tokens`
      default: return type.toUpperCase()
    }
  }

  function logClass(kind: Record<string, unknown>): string {
    const type = kind.type as string
    switch (type) {
      case 'step_completed': return (kind.success as boolean) ? 'text-success' : 'text-error'
      case 'verify_gate_result': return (kind.overall_passed as boolean) ? 'text-success' : 'text-error'
      case 'replan_triggered': return 'text-warning'
      case 'budget_warning': return 'text-warning'
      case 'run_stopped': return 'text-accent'
      case 'flat_step_completed': {
        const action = kind.action_type as string
        if (action.startsWith('verify:')) return 'text-accent'
        return 'text-text'
      }
      default: return 'text-text'
    }
  }

  function pipClass(kind: Record<string, unknown>): string {
    const type = kind.type as string
    switch (type) {
      case 'step_completed': return (kind.success as boolean) ? 'pip-success' : 'pip-error'
      case 'verify_gate_result': return (kind.overall_passed as boolean) ? 'pip-success' : 'pip-error'
      case 'replan_triggered': return 'pip-warning'
      case 'budget_warning': return 'pip-warning'
      case 'run_stopped': return 'pip-success'
      case 'step_started': return 'pip-running'
      default: return 'pip-pending'
    }
  }

  let filtered = $derived.by(() => {
    if (!filter) return events
    const lower = filter.toLowerCase()
    return events.filter(e => {
      const kind = e.kind as Record<string, unknown>
      return narrative(kind).toLowerCase().includes(lower)
    })
  })

  let completedSteps = $derived(steps.filter(s => s.status === 'passed' || s.status === 'failed').length)
  let totalSteps = $derived(steps.length)
  let progressPct = $derived(totalSteps > 0 ? Math.round((completedSteps / totalSteps) * 100) : (events.length > 0 ? Math.min(events.length * 11, 100) : 0))
  let isFinished = $derived(events.some(e => (e.kind as Record<string, unknown>).type === 'run_stopped'))
  let missionStatus = $derived(isFinished ? 'COMPLETE' : status === 'connected' ? 'IN PROGRESS' : 'CONNECTING')
  let missionPip = $derived(isFinished ? 'pip-success' : status === 'connected' ? 'pip-running' : 'pip-warning')

  $effect(() => {
    if (autoScroll && logEl && filtered.length > 0) {
      logEl.scrollTop = logEl.scrollHeight
    }
  })
</script>

<div class="h-screen flex flex-col bg-surface scanlines">
  <!-- Mission header bar -->
  <header class="border-b border-border bg-surface-2">
    <!-- Top info row -->
    <div class="flex items-center justify-between px-4 py-2 border-b border-border">
      <div class="flex items-center gap-3">
        <div class="pip {missionPip}"></div>
        <span class="text-[10px] font-mono uppercase tracking-widest {isFinished ? 'text-success' : 'text-running'}">{missionStatus}</span>
        <span class="text-border-bright">|</span>
        <span class="text-xs text-text-bright">{request || 'Mission'}</span>
      </div>
      <div class="flex items-center gap-1">
        <button onclick={onPause} class="text-[10px] font-mono px-2 py-1 text-text-dim hover:text-text bg-surface-3 hover:bg-border-bright transition-colors uppercase tracking-wider">Pause</button>
        <button onclick={onResume} class="text-[10px] font-mono px-2 py-1 text-text-dim hover:text-text bg-surface-3 hover:bg-border-bright transition-colors uppercase tracking-wider">Play</button>
        <div class="w-px h-4 bg-border mx-1"></div>
        {#each [1, 5, 10, 50] as s}
          <button
            onclick={() => onSpeed(s)}
            class="text-[10px] font-mono px-1.5 py-1 transition-colors uppercase {speed === s ? 'bg-accent text-white' : 'text-text-dim hover:text-text bg-surface-3 hover:bg-border-bright'}"
          >{s}x</button>
        {/each}
        <div class="w-px h-4 bg-border mx-1"></div>
        <button onclick={onDisconnect} class="text-[10px] font-mono px-2 py-1 text-error/70 hover:text-error bg-surface-3 hover:bg-error-dim transition-colors uppercase tracking-wider">Abort</button>
      </div>
    </div>

    <!-- Progress bar -->
    <div class="h-1 bg-surface">
      <div
        class="h-full transition-all duration-700 {isFinished ? 'bg-success' : 'bg-accent'}"
        style="width: {progressPct}%"
      ></div>
    </div>
  </header>

  <!-- Main content -->
  <div class="flex-1 flex overflow-hidden">
    <!-- Left panel: Tactical log -->
    <div class="w-3/5 border-r border-border flex flex-col">
      <!-- Log header -->
      <div class="flex items-center gap-2 px-3 py-1.5 border-b border-border bg-surface-2">
        <span class="text-[10px] font-mono text-text-dim uppercase tracking-widest">Operations log</span>
        <div class="flex-1"></div>
        <input
          type="text"
          bind:value={filter}
          placeholder="filter"
          class="w-32 bg-surface-3 border border-border text-[10px] font-mono text-text px-2 py-0.5 outline-none focus:border-accent placeholder:text-text-dim"
        />
        <label class="flex items-center gap-1 text-[10px] font-mono text-text-dim cursor-pointer">
          <input type="checkbox" bind:checked={autoScroll} class="accent-accent w-3 h-3" />
          follow
        </label>
        <span class="text-[10px] font-mono text-text-dim">{events.length}</span>
      </div>

      <!-- Log entries -->
      <div bind:this={logEl} class="flex-1 overflow-y-auto">
        {#each filtered as event (event.seq)}
          {@const kind = event.kind as Record<string, unknown>}
          <div class="flex items-center gap-2 px-3 py-1 border-b border-border/30 hover:bg-surface-3/30 transition-colors">
            <span class="text-[10px] font-mono text-text-dim w-6 text-right shrink-0">{event.seq}</span>
            <div class="pip {pipClass(kind)} shrink-0"></div>
            <span class="text-[10px] font-mono text-text-dim w-16 shrink-0">
              {new Date(event.ts).toLocaleTimeString('en', { hour12: false })}
            </span>
            <span class="text-xs font-mono {logClass(kind)} truncate">{narrative(kind)}</span>
          </div>
        {/each}

        {#if isFinished}
          <div class="mx-3 my-2 py-2 px-3 border border-success/30 bg-success-dim">
            <span class="text-[10px] font-mono uppercase tracking-widest text-success">Mission complete</span>
          </div>
        {/if}
      </div>
    </div>

    <!-- Right panel: Status -->
    <div class="w-2/5 flex flex-col overflow-y-auto bg-surface-2/50">
      <!-- Resources section -->
      <div class="p-3 border-b border-border">
        <div class="text-[10px] font-mono text-text-dim uppercase tracking-widest mb-2">Resources</div>
        <BudgetGauge {budget} />
      </div>

      <!-- Objectives section (plan steps) -->
      <div class="flex-1 flex flex-col">
        <div class="flex items-center justify-between px-3 py-1.5 border-b border-border">
          <span class="text-[10px] font-mono text-text-dim uppercase tracking-widest">Objectives</span>
          {#if totalSteps > 0}
            <span class="text-[10px] font-mono text-text">{completedSteps}/{totalSteps}</span>
          {/if}
        </div>
        <div class="flex-1 overflow-y-auto">
          <StepTable {steps} />
        </div>
      </div>
    </div>
  </div>
</div>
