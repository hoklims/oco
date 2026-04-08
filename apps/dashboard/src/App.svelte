<script lang="ts">
  import './app.css'
  import { onMount } from 'svelte'
  import type { DashboardEvent, BudgetSnapshot, StepRow } from './lib/types'
  import { connectSSE, type SSEClient, type SSEStatus } from './lib/sse'
  import { createEventPlayer, type EventPlayer } from './lib/event-player'
  import { playDemo, type Thought, type CompetitivePlan } from './lib/demo'
  import type { ReviewPacket } from './lib/types'
  import Timeline from './lib/Timeline.svelte'
  import PlanMap from './lib/PlanMap.svelte'
  import PlanExplorer from './lib/PlanExplorer.svelte'
  import DetailPanel from './lib/DetailPanel.svelte'
  import ClassifyingScene from './lib/ClassifyingScene.svelte'
  import PostRunPanel from './lib/PostRunPanel.svelte'
  import SeedLauncher from './lib/SeedLauncher.svelte'
  import Playground from './lib/Playground.svelte'

  // ── Playground routing ─────────────────────────────────────
  const isPlayground = new URLSearchParams(window.location.search).has('playground')

  // ── Lifecycle phases ────────────────────────────────────────
  type Phase = 'launcher' | 'connecting' | 'waiting' | 'classifying' | 'planning' | 'executing' | 'verifying' | 'complete' | 'failed' | 'demo'

  const PHASE_META: Record<Phase, { label: string; color: string; icon: string }> = {
    launcher:    { label: 'Ready',                 color: 'text-text-3', icon: '◎' },
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
  let explorationPlans = $state<{ loser: CompetitivePlan; winner: CompetitivePlan } | undefined>(undefined)
  let pendingCandidates = $state<Array<Record<string, unknown>>>([])  // temp storage from plan_exploration until plan_generated arrives
  let stepSummaries = $state<Array<{
    id: string; name: string; depends_on: string[]; verify_after: boolean; execution_mode: string
  }>>([])
  let teamInfo = $state<{ name: string; topology: string; member_count: number } | null>(null)
  let teammateMessages = $state<Array<{ fromStepId: string; toStepId: string; fromName: string; toName: string; summary: string }>>([])
  let subPlanState = $state<Map<string, { subSteps: Array<{ id: string; name: string; status: 'pending' | 'running' | 'passed' | 'failed' }>; completed: boolean }>>(new Map())
  let msgTimers: ReturnType<typeof setTimeout>[] = []
  let reviewPacket = $state<ReviewPacket | null>(null)
  let liveSessionId = $state<string | null>(null)
  let sseStatus = $state<SSEStatus>('connecting')
  let phase = $state<Phase>('launcher')

  // ── Derived ─────────────────────────────────────────────────
  let completedSteps = $derived(steps.filter(s => s.status === 'passed' || s.status === 'failed').length)
  let totalSteps = $derived(steps.length)
  let progressPct = $derived(totalSteps > 0 ? Math.round((completedSteps / totalSteps) * 100) : 0)
  let isFinished = $derived(phase === 'complete' || phase === 'failed')
  let isRunning = $derived(!isFinished && phase !== 'launcher' && phase !== 'connecting' && phase !== 'waiting' && phase !== 'demo')
  let selectedEvent = $derived(selectedSeq != null ? events.find(e => e.seq === selectedSeq) ?? null : null)
  let selectedStep = $derived(selectedStepId != null ? steps.find(s => s.id === selectedStepId) ?? null : null)
  let phaseMeta = $derived(PHASE_META[phase])

  // Transition from 'planning' to 'executing' when exploration animation completes
  $effect(() => {
    if (explorationPhase === 'done' && phase === 'planning') {
      phase = 'executing'
    }
  })

  // Ordered phase list for the stepper
  const PHASE_ORDER: Phase[] = ['connecting', 'waiting', 'classifying', 'planning', 'executing', 'verifying', 'complete']
  let phaseIndex = $derived(PHASE_ORDER.indexOf(phase))

  // ── Lifecycle management ────────────────────────────────────
  let cancelDemo: (() => void) | null = null
  let sseClient: SSEClient | null = null
  let eventPlayer: EventPlayer | null = null

  function resetState() {
    events = []; steps = []; budget = null; thoughts = []; explorationPhase = 'idle'; stepSummaries = []; teamInfo = null; teammateMessages = []; subPlanState = new Map(); msgTimers.forEach(clearTimeout); msgTimers = []
    selectedSeq = null; selectedStepId = null; missionRequest = ''; complexity = ''; provider = ''; reviewPacket = null
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
      (r) => { reviewPacket = r },
    )
  }

  function startLive(sessionId: string) {
    cancelDemo?.(); cancelDemo = null
    eventPlayer?.stop()
    resetState()
    liveSessionId = sessionId
    phase = 'connecting'

    // Create the EventPlayer — it choreographs event playback with proper timing
    eventPlayer = createEventPlayer({
      onEvent: handleEvent,
      onExploration: (p) => { explorationPhase = p },
      onThought: (t) => { thoughts = [...thoughts, t] },
      onTeammateMessage: (msg) => {
        const ts = Date.now() + Math.random()
        const tagged = { ...msg, _ts: ts }
        teammateMessages = [...teammateMessages, tagged]
        msgTimers.push(setTimeout(() => { teammateMessages = teammateMessages.filter(m => (m as typeof tagged)._ts !== ts) }, 3000))
      },
      onSubPlan: (update) => {
        const pid = update.parentStepId
        if (update.type === 'started' && update.subSteps) {
          const subs = update.subSteps.map(s => ({ ...s, status: 'pending' as const }))
          subPlanState = new Map(subPlanState).set(pid, { subSteps: subs, completed: false })
        } else if (update.type === 'progress' && update.subStepId && update.status) {
          const entry = subPlanState.get(pid)
          if (entry) {
            subPlanState = new Map(subPlanState).set(pid, {
              ...entry, subSteps: entry.subSteps.map(s => s.id === update.subStepId ? { ...s, status: update.status! } : s),
            })
          }
        } else if (update.type === 'completed') {
          const entry = subPlanState.get(pid)
          if (entry) {
            subPlanState = new Map(subPlanState).set(pid, { ...entry, completed: true })
            msgTimers.push(setTimeout(() => { const n = new Map(subPlanState); n.delete(pid); subPlanState = n }, 800))
          }
        }
      },
    })

    const baseUrl = `/api/v1/dashboard/sessions/${sessionId}/stream`
    sseClient = connectSSE(baseUrl)
    // SSE events go into the player buffer, NOT directly to handleEvent
    sseClient.onEvent((event) => eventPlayer?.push(event))
    sseClient.onStatus((status) => {
      sseStatus = status
      if (status === 'connected' && phase === 'connecting') {
        phase = 'waiting'
      }
    })
  }

  async function launchMission(seed: string, workspace: string, _provider: string, _model: string) {
    phase = 'connecting'
    try {
      const res = await fetch('/api/v1/sessions', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          user_request: seed,
          workspace_root: workspace || undefined,
        }),
      })
      if (!res.ok) {
        const err = await res.json().catch(() => ({ error: 'Server error' }))
        throw new Error(err.error || `HTTP ${res.status}`)
      }
      const data = await res.json()
      startLive(data.id)
    } catch (e) {
      // Server not reachable — fall back to demo with the seed as mission
      missionRequest = seed
      phase = 'launcher'
      throw e
    }
  }

  function backToLauncher() {
    cancelDemo?.(); cancelDemo = null
    sseClient?.close(); sseClient = null
    eventPlayer?.stop(); eventPlayer = null
    resetState()
    liveSessionId = null
    phase = 'launcher'
  }

  onMount(() => {
    const params = new URLSearchParams(window.location.search)
    const live = params.get('live')
    if (live) {
      startLive(live)
    }
    // Default: stay on launcher (no auto-demo)
    return () => { sseClient?.close(); cancelDemo?.(); eventPlayer?.stop(); msgTimers.forEach(clearTimeout) }
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
        if (kind.complexity) complexity = kind.complexity as string
        if (phase !== 'demo') phase = 'classifying'
        break

      case 'plan_exploration':
        if (phase !== 'demo') phase = 'planning'
        // Store candidates — explorationPlans will be built when plan_generated
        // arrives with real step names. No synthetic data needed.
        pendingCandidates = (kind.candidates as Array<Record<string, unknown>>) ?? []
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

      case 'plan_generated': {
        const rawSteps = (kind.steps as Array<Record<string, unknown>>) ?? []
        stepSummaries = rawSteps.map(s => ({
          id: s.id as string, name: s.name as string,
          depends_on: (s.depends_on as string[]) ?? [],
          verify_after: (s.verify_after as boolean) ?? false,
          execution_mode: (s.execution_mode as string) ?? 'inline',
        }))
        const t = kind.team as Record<string, unknown> | null
        teamInfo = t ? { name: t.name as string, topology: t.topology as string, member_count: t.member_count as number } : null
        steps = rawSteps.map(s => ({
          id: s.id as string, name: s.name as string, role: s.role as string,
          status: 'pending' as const, duration_ms: null, tokens_used: null,
          execution_mode: s.execution_mode as string, verify_passed: null,
        }))

        // Build explorationPlans with REAL winner steps from plan_generated.
        // Loser gets synthetic names (we never receive loser step details).
        if (pendingCandidates.length >= 2) {
          const winnerCandidate = pendingCandidates.find(c => c.winner) ?? pendingCandidates[pendingCandidates.length - 1]
          const loserCandidate = pendingCandidates.find(c => !c.winner) ?? pendingCandidates[0]

          // Winner: real step names from plan_generated.
          // Map UUID depends_on → step names (PlanExplorer matches edges by name).
          const idToName = new Map(rawSteps.map(s => [s.id as string, s.name as string]))
          const winnerSteps = rawSteps.map(s => ({
            name: s.name as string, role: (s.role as string) ?? 'implementer',
            verify: (s.verify_after as boolean) ?? false,
            tokens: (s.estimated_tokens as number) ?? 2000,
            depends_on: ((s.depends_on as string[]) ?? []).map(id => idToName.get(id) ?? id),
          }))

          // Loser: synthetic linear chain (we only have metadata)
          const loserStepCount = (loserCandidate.step_count as number) ?? 3
          const loserTokens = (loserCandidate.estimated_tokens as number) ?? 15000
          const loserSteps = Array.from({ length: loserStepCount }, (_, i) => ({
            name: i === 0 ? 'Analyze & plan' : i === loserStepCount - 1 ? 'Quick verify' : `Implement (${i})`,
            role: i === 0 ? 'architect' : i === loserStepCount - 1 ? 'tester' : 'implementer',
            verify: i === loserStepCount - 1,
            tokens: Math.round(loserTokens / loserStepCount),
            depends_on: i === 0 ? [] : [i === 1 ? 'Analyze & plan' : `Implement (${i - 1})`],
          }))

          // Derive scoring breakdown from event metadata.
          function deriveScores(cand: Record<string, unknown>) {
            const sc = (cand.score as number) ?? 0.5
            const vc = (cand.verify_count as number) ?? 0
            const pg = (cand.parallel_groups as number) ?? 1
            const steps = (cand.step_count as number) ?? 3
            const tokens = (cand.estimated_tokens as number) ?? 20000
            return {
              verify: Math.min(vc / Math.max(steps, 1), 1),
              cost: Math.max(0, 1 - tokens / 60000),
              parallel: Math.min((pg - 1) / 4, 1),
              depth: sc > 0.7 ? sc - 0.3 : sc * 0.5,
            }
          }

          explorationPlans = {
            loser: {
              strategy: (loserCandidate.strategy as string) ?? 'unknown',
              steps: loserSteps,
              score: (loserCandidate.score as number) ?? 0,
              winner: false,
              scores: deriveScores(loserCandidate),
            },
            winner: {
              strategy: (winnerCandidate.strategy as string) ?? 'unknown',
              steps: winnerSteps,
              score: (winnerCandidate.score as number) ?? 0,
              winner: true,
              scores: deriveScores(winnerCandidate),
            },
          }

          // Start exploration animation NOW (after explorationPlans is set).
          // In demo mode or non-live: drive phases from here.
          // In live mode: EventPlayer drives them via onExploration callback.
          if (phase === 'demo' || !liveSessionId) {
            explorationPhase = 'generating'
            setTimeout(() => { explorationPhase = 'comparing' }, 500)
            setTimeout(() => { explorationPhase = 'scoring' }, 2000)
            setTimeout(() => { explorationPhase = 'selecting' }, 3500)
            setTimeout(() => { explorationPhase = 'done' }, 5000)
          }
        }
        pendingCandidates = []

        // Transition to executing after exploration (or immediately if no exploration)
        if (!explorationPlans) {
          if (phase !== 'demo') phase = 'executing'
          if (explorationPhase === 'idle') explorationPhase = 'done'
        }
        // When explorationPlans IS set, phase transitions to 'executing'
        // reactively when explorationPhase reaches 'done' (see $effect below).
        break
      }

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
        if (snap) {
          // Merge: progress events only carry reliable token data.
          // Keep richer fields (tool_calls, retrievals, etc.) from prior flat_step_completed events.
          budget = {
            tokens_used: snap.tokens_used,
            tokens_remaining: snap.tokens_remaining,
            tool_calls_used: snap.tool_calls_used || budget?.tool_calls_used || 0,
            tool_calls_remaining: snap.tool_calls_remaining || budget?.tool_calls_remaining || 0,
            retrievals_used: snap.retrievals_used || budget?.retrievals_used || 0,
            verify_cycles_used: snap.verify_cycles_used || budget?.verify_cycles_used || 0,
            elapsed_secs: snap.elapsed_secs || budget?.elapsed_secs || 0,
          }
        }
        break
      }

      // In live mode, EventPlayer's onSubPlan/onTeammateMessage callbacks handle these.
      // In demo mode (no EventPlayer), handleEvent processes them directly.
      case 'sub_plan_started': {
        if (!liveSessionId) {
          const pid = kind.parent_step_id as string
          const subs = ((kind.sub_steps as Array<Record<string, unknown>>) ?? []).map(s => ({
            id: s.id as string, name: s.name as string, status: 'pending' as const,
          }))
          subPlanState = new Map(subPlanState).set(pid, { subSteps: subs, completed: false })
        }
        break
      }
      case 'sub_step_progress': {
        if (!liveSessionId) {
          const pid = kind.parent_step_id as string
          const sid = kind.sub_step_id as string
          const st = kind.status as 'pending' | 'running' | 'passed' | 'failed'
          const entry = subPlanState.get(pid)
          if (entry) {
            subPlanState = new Map(subPlanState).set(pid, {
              ...entry, subSteps: entry.subSteps.map(s => s.id === sid ? { ...s, status: st } : s),
            })
          }
        }
        break
      }
      case 'sub_plan_completed': {
        if (!liveSessionId) {
          const pid = kind.parent_step_id as string
          const entry = subPlanState.get(pid)
          if (entry) {
            subPlanState = new Map(subPlanState).set(pid, { ...entry, completed: true })
            msgTimers.push(setTimeout(() => { const n = new Map(subPlanState); n.delete(pid); subPlanState = n }, 800))
          }
        }
        break
      }
      case 'teammate_message': {
        if (!liveSessionId) {
          const msg = {
            fromStepId: kind.from_step_id as string, toStepId: kind.to_step_id as string,
            fromName: kind.from_name as string, toName: kind.to_name as string,
            summary: kind.summary as string,
          }
          teammateMessages = [...teammateMessages, msg]
          msgTimers.push(setTimeout(() => { teammateMessages = teammateMessages.filter(m => m !== msg) }, 3000))
        }
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

{#if isPlayground}
  <Playground />
{:else}
<div class="h-screen flex flex-col bg-bg">
  <!-- Phase stepper bar -->
  {#if liveSessionId && phase !== 'launcher'}
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

  <!-- Header (hidden in launcher) -->
  {#if phase !== 'launcher'}
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
    <button onclick={backToLauncher} class="px-3 py-1.5 text-xs text-text-3 hover:text-text-1 bg-surface-2 hover:bg-surface-3 rounded transition-colors">
      New
    </button>
  </header>
  {/if}

  <!-- Main content -->
  {#if phase === 'launcher'}
    <!-- Seed launcher — default view -->
    <div class="flex-1 overflow-y-auto">
      <SeedLauncher onLaunch={launchMission} onDemo={startDemo} />
    </div>
  {:else if phase === 'connecting' || phase === 'waiting'}
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
    {#if isFinished && reviewPacket}
      <!-- Post-run intelligence view -->
      <div class="flex-1 flex overflow-hidden min-h-0">
        <!-- Left: Post-run panel -->
        <div class="w-1/2 border-r border-border flex flex-col min-w-0">
          <PostRunPanel review={reviewPacket} />
        </div>
        <!-- Right: Timeline + Detail -->
        <div class="w-1/2 flex flex-col min-w-0">
          <div class="h-1/2 border-b border-border">
            <Timeline {events} {selectedSeq} onSelect={selectTimeline} />
          </div>
          <div class="h-1/2">
            <DetailPanel event={selectedEvent} {budget} {selectedStep} {thoughts} />
          </div>
        </div>
      </div>
    {:else}
      <!-- Plan — top zone -->
      <div class="h-[55%] border-b border-border shrink-0 relative">
        {#if explorationPlans}
          <PlanExplorer phase={explorationPhase} plans={explorationPlans} />
        {/if}
        {#if explorationPhase === 'done' || (steps.length > 0 && !explorationPlans)}
          <div class="absolute inset-0 z-10 plan-map-enter">
            <PlanMap {steps} selectedId={selectedStepId} onSelect={selectStep} {thoughts} {stepSummaries} {teamInfo} {teammateMessages} {subPlanState} />
          </div>
        {:else if phase === 'classifying'}
          <ClassifyingScene mission={missionRequest} {complexity} />
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
  {/if}

  <!-- Footer (hidden in launcher) -->
  {#if phase !== 'launcher'}
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
  {/if}
</div>
{/if}
