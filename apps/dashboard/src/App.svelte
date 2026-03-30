<script lang="ts">
  import './app.css'
  import { onMount } from 'svelte'
  import type { DashboardEvent, BudgetSnapshot, StepRow } from './lib/types'
  import { connectSSE, type SSEClient, type SSEStatus } from './lib/sse'
  import { playDemo, type Thought } from './lib/demo'
  import Timeline from './lib/Timeline.svelte'
  import PlanMap from './lib/PlanMap.svelte'
  import PlanExplorer from './lib/PlanExplorer.svelte'
  import DetailPanel from './lib/DetailPanel.svelte'

  // ── Lifecycle phases ────────────────────────────────────────
  type Phase = 'connecting' | 'waiting' | 'classifying' | 'planning' | 'executing' | 'verifying' | 'complete' | 'failed' | 'demo'

  const PHASE_META: Record<Phase, { label: string; color: string; icon: string }> = {
    connecting:  { label: 'Connecting',            color: 'text-amber',  icon: '◌' },
    waiting:     { label: 'Waiting for mission',   color: 'text-text-3', icon: '◎' },
    classifying: { label: 'Classifying task',      color: 'text-cyan',   icon: '◈' },
    planning:    { label: 'Planning execution',    color: 'text-purple', icon: '◇' },
    executing:   { label: 'Executing plan',        color: 'text-blue',   icon: '▸' },
    verifying:   { label: 'Verifying results',     color: 'text-amber',  icon: '◆' },
    complete:    { label: 'Mission accomplished',  color: 'text-green',  icon: '✓' },
    failed:      { label: 'Mission failed',        color: 'text-red',    icon: '✗' },
    demo:        { label: 'Demo mode',             color: 'text-purple', icon: '▶' },
  }

  // ── State ───────────────────────────────────────────────────
  let events = $state<DashboardEvent[]>([])
  let steps = $state<StepRow[]>([])
  let budget = $state<BudgetSnapshot | null>(null)
  let missionRequest = $state('')
  let complexity = $state('')
  let provider = $state('')
  let selectedSeq = $state<number | null>(null)
  let selectedStepId = $state<string | null>(null)
  let thoughts = $state<Thought[]>([])
  let explorationPhase = $state<'idle' | 'generating' | 'comparing' | 'scoring' | 'selecting' | 'done'>('idle')
  let liveSessionId = $state<string | null>(null)
  let sseStatus = $state<SSEStatus>('connecting')
  let phase = $state<Phase>('connecting')

  // ── Derived ─────────────────────────────────────────────────
  let completedSteps = $derived(steps.filter(s => s.status === 'passed' || s.status === 'failed').length)
  let totalSteps = $derived(steps.length)
  let progressPct = $derived(totalSteps > 0 ? Math.round((completedSteps / totalSteps) * 100) : 0)
  let isFinished = $derived(phase === 'complete' || phase === 'failed')
  let isRunning = $derived(!isFinished && phase !== 'connecting' && phase !== 'waiting' && phase !== 'demo')
  let selectedEvent = $derived(selectedSeq != null ? events.find(e => e.seq === selectedSeq) ?? null : null)
  let selectedStep = $derived(selectedStepId != null ? steps.find(s => s.id === selectedStepId) ?? null : null)
  let phaseMeta = $derived(PHASE_META[phase])

  // Ordered phase list for the stepper
  const PHASE_ORDER: Phase[] = ['connecting', 'waiting', 'classifying', 'planning', 'executing', 'verifying', 'complete']
  let phaseIndex = $derived(PHASE_ORDER.indexOf(phase))

  // ── Lifecycle management ────────────────────────────────────
  let cancelDemo: (() => void) | null = null
  let sseClient: SSEClient | null = null

  function resetState() {
    events = []; steps = []; budget = null; thoughts = []; explorationPhase = 'idle'
    selectedSeq = null; selectedStepId = null; missionRequest = ''; complexity = ''; provider = ''
  }

  function startDemo() {
    sseClient?.close(); sseClient = null; liveSessionId = null
    resetState()
    phase = 'demo'
    cancelDemo?.()
    cancelDemo = playDemo(
      handleEvent,
      (t) => { thoughts = [...thoughts, t] },
      (p) => { explorationPhase = p },
    )
  }

  function startLive(sessionId: string) {
    cancelDemo?.(); cancelDemo = null
    resetState()
    liveSessionId = sessionId
    phase = 'connecting'
    const baseUrl = `/api/v1/dashboard/sessions/${sessionId}/stream`
    sseClient = connectSSE(baseUrl)
    sseClient.onEvent(handleEvent)
    sseClient.onStatus((status) => {
      sseStatus = status
      if (status === 'connected' && phase === 'connecting') {
        phase = 'waiting'
      }
    })
  }

  onMount(() => {
    const params = new URLSearchParams(window.location.search)
    const live = params.get('live')
    if (live) {
      startLive(live)
    } else {
      startDemo()
    }
    return () => { sseClient?.close(); cancelDemo?.() }
  })

  // ── Event handler ───────────────────────────────────────────
  function handleEvent(event: DashboardEvent) {
    events = [...events, event]
    const kind = event.kind as Record<string, unknown>
    const type = kind.type as string

    switch (type) {
      case 'run_started':
        missionRequest = kind.request_summary as string
        provider = `${kind.provider}/${kind.model}`
        if (phase !== 'demo') phase = 'classifying'
        break

      case 'plan_exploration':
        if (phase !== 'demo') phase = 'planning'
        explorationPhase = 'generating'
        setTimeout(() => { explorationPhase = 'comparing' }, 500)
        setTimeout(() => { explorationPhase = 'scoring' }, 2000)
        setTimeout(() => { explorationPhase = 'selecting' }, 3500)
        setTimeout(() => { explorationPhase = 'done' }, 5000)
        break

      case 'flat_step_completed': {
        const snap = kind.budget_snapshot as BudgetSnapshot | undefined
        if (snap?.tokens_used !== undefined) budget = snap
        const actionType = kind.action_type as string
        const reason = (kind.reason as string) || ''
        if (phase !== 'demo') {
          // Handle phase events injected by oco.emit_phase (via POST /sessions/{id}/events)
          switch (actionType) {
            case 'classifying':
              phase = 'classifying'; complexity = reason; break
            case 'planning':
              phase = 'planning'; complexity = reason; break
            case 'executing':
              phase = 'executing'; break
            case 'verifying':
              phase = 'verifying'; break
            case 'plan':
              // Rust orchestrator: classification → plan engine routing
              complexity = reason; phase = 'planning'; break
          }
        }
        break
      }

      case 'plan_generated':
        if (phase !== 'demo') phase = 'executing'
        if (explorationPhase === 'idle') explorationPhase = 'done'
        steps = ((kind.steps as Array<Record<string, unknown>>) ?? []).map(s => ({
          id: s.id as string, name: s.name as string, role: s.role as string,
          status: 'pending' as const, duration_ms: null, tokens_used: null,
          execution_mode: s.execution_mode as string, verify_passed: null,
        }))
        break

      case 'step_started':
        if (phase !== 'demo') phase = 'executing'
        steps = steps.map(s => s.id === (kind.step_id as string) ? { ...s, status: 'running' as const } : s)
        break

      case 'step_completed': {
        const id = kind.step_id as string
        steps = steps.map(s => s.id === id ? { ...s,
          status: (kind.success ? 'passed' : 'failed') as StepRow['status'],
          duration_ms: kind.duration_ms as number, tokens_used: kind.tokens_used as number,
        } : s)
        break
      }

      case 'verify_gate_result':
        if (phase !== 'demo') phase = 'verifying'
        steps = steps.map(s => s.id === (kind.step_id as string) ? { ...s, verify_passed: kind.overall_passed as boolean } : s)
        break

      case 'progress': {
        const snap = kind.budget as BudgetSnapshot | undefined
        if (snap?.tokens_used !== undefined) budget = snap
        break
      }

      case 'run_stopped': {
        const reason = kind.reason as Record<string, unknown> | string
        const reasonType = typeof reason === 'string' ? reason : (reason?.type as string) ?? 'unknown'
        if (phase !== 'demo') {
          phase = (reasonType === 'task_complete') ? 'complete' : 'failed'
        }
        break
      }
    }
  }

  function selectTimeline(seq: number) { selectedSeq = seq; selectedStepId = null }
  function selectStep(id: string) { selectedStepId = id; selectedSeq = null }
</script>

<div class="h-screen flex flex-col bg-bg">
  <!-- Phase stepper bar -->
  {#if liveSessionId}
    <div class="flex items-center gap-0 px-5 py-2 border-b border-border bg-surface-2 shrink-0 overflow-x-auto">
      {#each PHASE_ORDER as p, i}
        {@const isActive = p === phase}
        {@const isPast = phaseIndex > i}
        {@const isFailed = phase === 'failed' && i === PHASE_ORDER.length - 1}
        <div class="flex items-center gap-2 shrink-0">
          <div class="flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-mono transition-all duration-500
            {isActive ? 'bg-surface-3 ' + PHASE_META[p].color + ' ring-1 ring-border-2' :
             isPast ? 'text-text-3 line-through opacity-50' :
             'text-text-3 opacity-30'}">
            <span class="text-[10px] {isActive ? 'animate-pulse' : ''}">{PHASE_META[p].icon}</span>
            <span>{PHASE_META[p].label}</span>
          </div>
          {#if i < PHASE_ORDER.length - 1}
            <div class="w-6 h-px mx-1 transition-colors duration-500
              {isPast ? 'bg-text-3' : 'bg-border'}"></div>
          {/if}
        </div>
      {/each}
      {#if phase === 'failed'}
        <div class="flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-mono text-red bg-red-dim ring-1 ring-red/20 ml-2">
          <span class="text-[10px]">✗</span>
          <span>Failed</span>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Header -->
  <header class="flex items-center gap-4 px-5 py-3 border-b border-border bg-surface shrink-0">
    <div class="pip {isFinished ? (phase === 'failed' ? 'pip-fail' : 'pip-done') : isRunning ? 'pip-active' : 'pip-idle'}"></div>
    <div class="flex-1 min-w-0">
      {#if missionRequest}
        <div class="text-[15px] text-text-1 font-medium truncate">{missionRequest}</div>
        {#if complexity || provider}
          <div class="text-xs text-text-3 font-mono mt-0.5">{complexity}{complexity && provider ? ' · ' : ''}{provider}</div>
        {/if}
      {:else if phase === 'connecting'}
        <div class="text-[15px] text-amber font-medium flex items-center gap-2">
          <span class="inline-block w-2 h-2 rounded-full bg-amber animate-pulse"></span>
          Connecting to orchestrator...
        </div>
      {:else if phase === 'waiting'}
        <div class="text-[15px] text-text-3 font-medium flex items-center gap-2">
          <span class="inline-block w-2 h-2 rounded-full bg-cyan animate-pulse"></span>
          Waiting for mission...
        </div>
      {:else}
        <div class="text-[15px] text-text-2 font-medium">OCO Dashboard</div>
      {/if}
    </div>
    {#if budget}
      <div class="flex items-center gap-4 text-xs font-mono text-text-3 shrink-0">
        <span>{budget.tokens_used.toLocaleString()} tok</span>
        <span>{budget.tool_calls_used} actions</span>
        <span>{budget.elapsed_secs}s</span>
      </div>
    {/if}
    <div class="w-36 shrink-0">
      <div class="flex items-center gap-2">
        <div class="rail flex-1"><div class="rail-fill {isFinished ? (phase === 'failed' ? 'bg-red' : 'bg-green') : 'bg-blue'}" style="width: {progressPct}%"></div></div>
        <span class="text-xs font-mono text-text-3">{progressPct}%</span>
      </div>
    </div>
    {#if liveSessionId}
      <span class="text-xs font-mono {sseStatus === 'connected' ? 'text-green' : 'text-amber'}">{sseStatus}</span>
    {/if}
    <button onclick={startDemo} class="px-3 py-1.5 text-xs text-text-3 hover:text-text-1 bg-surface-2 hover:bg-surface-3 rounded transition-colors">
      Demo
    </button>
  </header>

  <!-- Main content -->
  {#if phase === 'connecting' || phase === 'waiting'}
    <!-- Pre-mission: full-screen waiting state -->
    <div class="flex-1 flex items-center justify-center">
      <div class="text-center space-y-6">
        <div class="relative">
          <div class="w-20 h-20 mx-auto rounded-2xl bg-surface-2 border border-border flex items-center justify-center">
            {#if phase === 'connecting'}
              <div class="space-y-1.5">
                <div class="w-8 h-1 bg-amber/30 rounded animate-pulse"></div>
                <div class="w-6 h-1 bg-amber/20 rounded animate-pulse" style="animation-delay: 0.2s"></div>
                <div class="w-7 h-1 bg-amber/25 rounded animate-pulse" style="animation-delay: 0.4s"></div>
              </div>
            {:else}
              <div class="w-3 h-3 rounded-full bg-cyan animate-pulse"></div>
            {/if}
          </div>
          {#if phase === 'connecting'}
            <div class="absolute inset-0 rounded-2xl border border-amber/20 animate-ping" style="animation-duration: 2s"></div>
          {/if}
        </div>
        <div>
          <p class="text-lg {phase === 'connecting' ? 'text-amber' : 'text-text-2'} font-medium">
            {phase === 'connecting' ? 'Connecting to orchestrator...' : 'Ready — waiting for mission'}
          </p>
          <p class="text-sm text-text-3 mt-1 font-mono">
            {#if phase === 'connecting'}
              Establishing SSE connection
            {:else}
              The orchestration loop will start any moment
            {/if}
          </p>
        </div>
      </div>
    </div>
  {:else}
    <!-- Active mission: normal layout -->
    <!-- Plan — top zone -->
    <div class="h-[55%] border-b border-border shrink-0 relative">
      <PlanExplorer phase={explorationPhase} />
      {#if explorationPhase === 'done' || steps.length > 0}
        <PlanMap {steps} selectedId={selectedStepId} onSelect={selectStep} {thoughts} />
      {:else if phase === 'classifying'}
        <!-- Classification phase — visual feedback -->
        <div class="h-full flex items-center justify-center">
          <div class="text-center space-y-4">
            <div class="w-16 h-16 mx-auto rounded-xl bg-surface-2 border border-cyan/20 flex items-center justify-center">
              <div class="w-8 h-8 border-2 border-cyan/40 border-t-cyan rounded-full animate-spin"></div>
            </div>
            <div>
              <p class="text-sm text-cyan font-mono">Analyzing task complexity...</p>
              {#if missionRequest}
                <p class="text-xs text-text-3 mt-1 max-w-md truncate">{missionRequest}</p>
              {/if}
            </div>
          </div>
        </div>
      {:else if phase === 'planning' && explorationPhase === 'idle'}
        <!-- Planning without exploration — simple planning indicator -->
        <div class="h-full flex items-center justify-center">
          <div class="text-center space-y-4">
            <div class="w-16 h-16 mx-auto rounded-xl bg-surface-2 border border-purple/20 flex items-center justify-center">
              <div class="grid grid-cols-2 gap-1">
                <div class="w-3 h-3 bg-purple/30 rounded-sm animate-pulse"></div>
                <div class="w-3 h-3 bg-purple/50 rounded-sm animate-pulse" style="animation-delay: 0.15s"></div>
                <div class="w-3 h-3 bg-purple/40 rounded-sm animate-pulse" style="animation-delay: 0.3s"></div>
                <div class="w-3 h-3 bg-purple/20 rounded-sm animate-pulse" style="animation-delay: 0.45s"></div>
              </div>
            </div>
            <div>
              <p class="text-sm text-purple font-mono">Generating execution plan...</p>
              {#if complexity}
                <p class="text-xs text-text-3 mt-1">{complexity}</p>
              {/if}
            </div>
          </div>
        </div>
      {:else if explorationPhase !== 'idle'}
        <div class="h-full"></div>
      {/if}
    </div>

    <!-- Bottom: Activity + Detail -->
    <div class="flex-1 flex overflow-hidden min-h-0">
      <div class="w-1/2 border-r border-border flex flex-col min-w-0">
        <Timeline {events} {selectedSeq} onSelect={selectTimeline} />
      </div>
      <div class="w-1/2 flex flex-col min-w-0">
        <DetailPanel event={selectedEvent} {budget} {selectedStep} {thoughts} />
      </div>
    </div>
  {/if}

  <!-- Footer -->
  <footer class="flex items-center gap-3 px-5 py-1.5 border-t border-border bg-surface text-xs font-mono shrink-0">
    <span class="{phaseMeta.color} uppercase tracking-wider flex items-center gap-1.5">
      <span>{phaseMeta.icon}</span>
      {phaseMeta.label}
    </span>
    {#if liveSessionId}
      <span class="text-text-3">·</span>
      <span class="text-text-3 truncate max-w-48" title={liveSessionId}>{liveSessionId.slice(0, 8)}</span>
    {/if}
    <span class="ml-auto text-text-3">{events.length} events</span>
  </footer>
</div>
