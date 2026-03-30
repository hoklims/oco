<script lang="ts">
  import './app.css'
  import { onMount } from 'svelte'
  import type { DashboardEvent, BudgetSnapshot, StepRow } from './lib/types'
  import { playDemo, type Thought } from './lib/demo'
  import Timeline from './lib/Timeline.svelte'
  import PlanMap from './lib/PlanMap.svelte'
  import PlanExplorer from './lib/PlanExplorer.svelte'
  import DetailPanel from './lib/DetailPanel.svelte'

  let events = $state<DashboardEvent[]>([])
  let steps = $state<StepRow[]>([])
  let budget = $state<BudgetSnapshot | null>(null)
  let missionRequest = $state('')
  let selectedSeq = $state<number | null>(null)
  let selectedStepId = $state<string | null>(null)
  let thoughts = $state<Thought[]>([])
  let explorationPhase = $state<'idle' | 'generating' | 'comparing' | 'scoring' | 'selecting' | 'done'>('idle')

  let completedSteps = $derived(steps.filter(s => s.status === 'passed' || s.status === 'failed').length)
  let totalSteps = $derived(steps.length)
  let progressPct = $derived(totalSteps > 0 ? Math.round((completedSteps / totalSteps) * 100) : 0)
  let isFinished = $derived(events.some(e => (e.kind as Record<string, unknown>).type === 'run_stopped'))
  let isRunning = $derived(events.length > 0 && !isFinished)
  let selectedEvent = $derived(selectedSeq != null ? events.find(e => e.seq === selectedSeq) ?? null : null)
  let selectedStep = $derived(selectedStepId != null ? steps.find(s => s.id === selectedStepId) ?? null : null)

  let cancelDemo: (() => void) | null = null
  function startDemo() {
    events = []; steps = []; budget = null; thoughts = []; explorationPhase = 'idle'
    selectedSeq = null; selectedStepId = null; missionRequest = ''
    cancelDemo?.()
    cancelDemo = playDemo(
      handleEvent,
      (t) => { thoughts = [...thoughts, t] },
      (phase) => { explorationPhase = phase },
    )
  }
  onMount(() => { startDemo() })

  function handleEvent(event: DashboardEvent) {
    events = [...events, event]
    const kind = event.kind as Record<string, unknown>
    const type = kind.type as string
    switch (type) {
      case 'run_started': missionRequest = kind.request_summary as string; break
      case 'plan_generated':
        steps = ((kind.steps as Array<Record<string, unknown>>) ?? []).map(s => ({
          id: s.id as string, name: s.name as string, role: s.role as string,
          status: 'pending' as const, duration_ms: null, tokens_used: null,
          execution_mode: s.execution_mode as string, verify_passed: null,
        })); break
      case 'step_started': steps = steps.map(s => s.id === (kind.step_id as string) ? { ...s, status: 'running' as const } : s); break
      case 'step_completed': {
        const id = kind.step_id as string
        steps = steps.map(s => s.id === id ? { ...s,
          status: (kind.success ? 'passed' : 'failed') as StepRow['status'],
          duration_ms: kind.duration_ms as number, tokens_used: kind.tokens_used as number,
        } : s); break
      }
      case 'verify_gate_result': steps = steps.map(s => s.id === (kind.step_id as string) ? { ...s, verify_passed: kind.overall_passed as boolean } : s); break
      case 'progress': { const snap = kind.budget as BudgetSnapshot | undefined; if (snap?.tokens_used !== undefined) budget = snap; break }
      case 'flat_step_completed': { const snap = kind.budget_snapshot as BudgetSnapshot | undefined; if (snap?.tokens_used !== undefined) budget = snap; break }
    }
  }

  function selectTimeline(seq: number) { selectedSeq = seq; selectedStepId = null }
  function selectStep(id: string) { selectedStepId = id; selectedSeq = null }
</script>

<div class="h-screen flex flex-col bg-bg">
  <!-- Header -->
  <header class="flex items-center gap-4 px-5 py-3 border-b border-border bg-surface shrink-0">
    <div class="pip {isFinished ? 'pip-done' : isRunning ? 'pip-active' : 'pip-idle'}"></div>
    <div class="flex-1 min-w-0">
      <div class="text-[15px] text-text-1 font-medium truncate">{missionRequest || 'Waiting...'}</div>
    </div>
    <!-- Budget quick stats in header -->
    {#if budget}
      <div class="flex items-center gap-4 text-xs font-mono text-text-3 shrink-0">
        <span>{budget.tokens_used.toLocaleString()} tok</span>
        <span>{budget.tool_calls_used} actions</span>
        <span>{budget.elapsed_secs}s</span>
      </div>
    {/if}
    <div class="w-36 shrink-0">
      <div class="flex items-center gap-2">
        <div class="rail flex-1"><div class="rail-fill {isFinished ? 'bg-green' : 'bg-blue'}" style="width: {progressPct}%"></div></div>
        <span class="text-xs font-mono text-text-3">{progressPct}%</span>
      </div>
    </div>
    <button onclick={startDemo} class="px-3 py-1.5 text-xs text-text-3 hover:text-text-1 bg-surface-2 hover:bg-surface-3 rounded transition-colors">
      Replay
    </button>
  </header>

  <!-- Plan — top zone, takes ~55% height, the star of the show -->
  <div class="h-[55%] border-b border-border shrink-0 relative">
    <PlanExplorer phase={explorationPhase} />
    {#if explorationPhase === 'done' || steps.length > 0}
      <PlanMap {steps} selectedId={selectedStepId} onSelect={selectStep} {thoughts} />
    {:else if explorationPhase !== 'idle'}
      <!-- Empty plan area during exploration — just the SVG overlay -->
      <div class="h-full"></div>
    {/if}
  </div>

  <!-- Bottom: Activity + Detail side by side -->
  <div class="flex-1 flex overflow-hidden min-h-0">
    <!-- Activity feed -->
    <div class="w-1/2 border-r border-border flex flex-col min-w-0">
      <Timeline {events} {selectedSeq} onSelect={selectTimeline} />
    </div>
    <!-- Detail panel -->
    <div class="w-1/2 flex flex-col min-w-0">
      <DetailPanel event={selectedEvent} {budget} {selectedStep} {thoughts} />
    </div>
  </div>

  <!-- Footer -->
  <footer class="flex items-center gap-3 px-5 py-1.5 border-t border-border bg-surface text-xs font-mono shrink-0">
    <span class="{isFinished ? 'text-green' : isRunning ? 'text-cyan' : 'text-text-3'} uppercase tracking-wider">
      {isFinished ? 'Complete' : isRunning ? 'Running' : 'Idle'}
    </span>
    {#if isFinished}
      <span class="text-text-3">|</span>
      <span class="text-green">Mission accomplished</span>
    {/if}
    <span class="ml-auto text-text-3">{events.length} events</span>
  </footer>
</div>
