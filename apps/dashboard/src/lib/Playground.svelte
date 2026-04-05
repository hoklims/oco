<script lang="ts">
  import { SCENARIOS, type PlaygroundScenario } from './playground-data'
  import {
    SCORECARD_PRESETS, GATE_PRESETS, MISSION_PRESETS, REVIEW_PRESETS,
    type ScorecardPreset, type GatePreset, type MissionPreset, type ReviewPreset,
  } from './playground-presets'
  import type {
    DashboardEvent, StepRow, BudgetSnapshot, ReviewPacket,
    RunScorecard, GateResult, MissionMemory, ScorecardDimension,
    GateVerdict as GateVerdictType, MergeReadiness, TrustVerdict, BaselineFreshness,
  } from './types'
  import type { Thought } from './demo'
  import { createEventPlayer } from './event-player'

  // Components
  import PlanMap from './PlanMap.svelte'
  import PlanExplorer from './PlanExplorer.svelte'
  import ClassifyingScene from './ClassifyingScene.svelte'
  import PostRunPanel from './PostRunPanel.svelte'
  import ScorecardRadar from './ScorecardRadar.svelte'
  import GateVerdict from './GateVerdict.svelte'
  import MissionPanel from './MissionPanel.svelte'
  import ReviewBadge from './ReviewBadge.svelte'
  import Timeline from './Timeline.svelte'

  // ── Tab system ────────────────────────────────────────────────
  type Tab = 'execution' | 'scorecard' | 'gate' | 'mission' | 'review' | 'full'

  const TABS: { id: Tab; label: string; color: string; icon: string }[] = [
    { id: 'execution', label: 'Execution', color: 'cyan', icon: '▸' },
    { id: 'scorecard', label: 'Scorecard', color: 'green', icon: '◎' },
    { id: 'gate', label: 'Gate', color: 'amber', icon: '◆' },
    { id: 'mission', label: 'Mission', color: 'purple', icon: '◇' },
    { id: 'review', label: 'Review', color: 'blue', icon: '✓' },
    { id: 'full', label: 'Full Flow', color: 'green', icon: '▶' },
  ]

  let activeTab = $state<Tab>('execution')

  // ── Execution tab state ───────────────────────────────────────
  type ExecMode = 'idle' | 'planmap' | 'classifying' | 'explorer'
  let execMode = $state<ExecMode>('idle')
  let selectedScenario = $state<PlaygroundScenario>(SCENARIOS[0])
  let events = $state<DashboardEvent[]>([])
  let steps = $state<StepRow[]>([])
  let thoughts = $state<Thought[]>([])
  let explorationPhase = $state<'idle' | 'generating' | 'comparing' | 'scoring' | 'selecting' | 'done'>('idle')
  let selectedStepId = $state<string | null>(null)
  let budget = $state<BudgetSnapshot | null>(null)
  let missionRequest = $state('')
  let complexity = $state('')
  let isPlaying = $state(false)
  let playStartTime = $state<number | null>(null)
  let elapsedMs = $state(0)
  let teamInfo = $state<{ name: string; topology: string; member_count: number } | null>(null)
  let teammateMessages = $state<Array<{ fromStepId: string; toStepId: string; fromName: string; toName: string; summary: string }>>([])
  let messageTimers: ReturnType<typeof setTimeout>[] = []
  let subPlanState = $state<Map<string, { subSteps: Array<{ id: string; name: string; status: 'pending' | 'running' | 'passed' | 'failed' }>; completed: boolean }>>(new Map())
  let timerHandle: ReturnType<typeof setInterval> | null = null
  let stepSummaries = $state<Array<{ id: string; name: string; depends_on: string[]; verify_after: boolean; execution_mode: string }>>([])
  let cancelPlayback: (() => void) | null = null

  // ── Scorecard tab state ───────────────────────────────────────
  let scorecardPreset = $state<ScorecardPreset>(SCORECARD_PRESETS[0])
  let scorecardData = $state<RunScorecard>(structuredClone(SCORECARD_PRESETS[0].data))

  // ── Gate tab state ────────────────────────────────────────────
  let gatePreset = $state<GatePreset>(GATE_PRESETS[0])
  let gateData = $state<GateResult>(structuredClone(GATE_PRESETS[0].data))

  // ── Mission tab state ─────────────────────────────────────────
  let missionPreset = $state<MissionPreset>(MISSION_PRESETS[0])
  let missionData = $state<MissionMemory>(structuredClone(MISSION_PRESETS[0].data))

  // ── Review tab state ──────────────────────────────────────────
  let reviewPreset = $state<ReviewPreset>(REVIEW_PRESETS[0])
  let reviewData = $state<ReviewPacket>(structuredClone(REVIEW_PRESETS[0].data))

  // ── Review overrides ──────────────────────────────────────────
  let reviewMerge = $state<MergeReadiness>('Ready')
  let reviewTrust = $state<TrustVerdict | 'null'>('High')
  let reviewGate = $state<GateVerdictType | 'null'>('Pass')
  let reviewFreshness = $state<BaselineFreshness>('Fresh')

  // ── Cleanup ───────────────────────────────────────────────────
  function cleanup() {
    cancelPlayback?.(); cancelPlayback = null
    if (timerHandle) { clearInterval(timerHandle); timerHandle = null }
    isPlaying = false; playStartTime = null
  }

  function resetExec() {
    cleanup()
    events = []; steps = []; thoughts = []; budget = null
    explorationPhase = 'idle'; selectedStepId = null
    missionRequest = ''; complexity = ''; elapsedMs = 0
    stepSummaries = []; teamInfo = null; teammateMessages = []; subPlanState = new Map()
    messageTimers.forEach(clearTimeout); messageTimers = []
    execMode = 'idle'
  }

  // ── Exec event handler ────────────────────────────────────────
  function handleEvent(event: DashboardEvent) {
    events = [...events, event]
    const kind = event.kind as Record<string, unknown>
    const type = kind.type as string
    switch (type) {
      case 'run_started': missionRequest = kind.request_summary as string; break
      case 'plan_generated': {
        const incomingSteps = ((kind.steps as Array<Record<string, unknown>>) ?? []).map(s => ({
          id: s.id as string, name: s.name as string, depends_on: s.depends_on as string[],
          verify_after: s.verify_after as boolean, execution_mode: s.execution_mode as string,
          role: (s.role as string) ?? 'implementer',
        }))

        const isReplan = event.plan_version > 1 && steps.length > 0

        if (isReplan) {
          // Merge: keep existing completed/failed steps, add new pending ones
          const existingById = new Map(steps.map(s => [s.id, s]))
          const newSummaries = [...stepSummaries]
          const newSteps = [...steps]

          // Mark removed steps (exist in old but not in new plan) as 'failed'
          const incomingIds = new Set(incomingSteps.map(s => s.id))
          for (const s of newSteps) {
            if (!incomingIds.has(s.id) && s.status === 'pending') {
              s.status = 'failed'
            }
          }

          // Add genuinely new steps
          for (const s of incomingSteps) {
            if (!existingById.has(s.id)) {
              newSummaries.push({ id: s.id, name: s.name, depends_on: s.depends_on, verify_after: s.verify_after, execution_mode: s.execution_mode })
              newSteps.push({
                id: s.id, name: s.name, role: s.role,
                status: 'pending' as const, duration_ms: null, tokens_used: null,
                execution_mode: s.execution_mode, verify_passed: null,
              })
            }
          }

          stepSummaries = newSummaries
          steps = newSteps
        } else {
          // First plan: initialize from scratch
          stepSummaries = incomingSteps.map(s => ({ id: s.id, name: s.name, depends_on: s.depends_on, verify_after: s.verify_after, execution_mode: s.execution_mode }))
          steps = incomingSteps.map(s => ({
            id: s.id, name: s.name, role: s.role,
            status: 'pending' as const, duration_ms: null, tokens_used: null,
            execution_mode: s.execution_mode, verify_passed: null,
          }))
        }

        { const t = kind.team as Record<string, unknown> | null
          teamInfo = t ? { name: t.name as string, topology: t.topology as string, member_count: t.member_count as number } : null }
        break
      }
      case 'step_started': steps = steps.map(s => s.id === (kind.step_id as string) ? { ...s, status: 'running' as const } : s); break
      case 'step_completed': { const id = kind.step_id as string; steps = steps.map(s => s.id === id ? { ...s, status: (kind.success ? 'passed' : 'failed') as StepRow['status'], duration_ms: kind.duration_ms as number, tokens_used: kind.tokens_used as number } : s); break }
      case 'verify_gate_result': {
        const vid = kind.step_id as string
        const passed = kind.overall_passed as boolean
        steps = steps.map(s => s.id === vid ? {
          ...s,
          verify_passed: passed,
          // When verify fails, override step status to 'failed' so the node turns red
          ...(passed ? {} : { status: 'failed' as const }),
        } : s)
        break
      }
      case 'progress': { const snap = kind.budget as BudgetSnapshot | undefined; if (snap?.tokens_used !== undefined) budget = snap; break }
      case 'sub_plan_started': { const pid = kind.parent_step_id as string; const subs = ((kind.sub_steps as Array<Record<string, unknown>>) ?? []).map(s => ({ id: s.id as string, name: s.name as string, status: 'pending' as const })); subPlanState = new Map(subPlanState).set(pid, { subSteps: subs, completed: false }); break }
      case 'sub_step_progress': { const pid = kind.parent_step_id as string; const sid = kind.sub_step_id as string; const st = kind.status as 'pending' | 'running' | 'passed' | 'failed'; const entry = subPlanState.get(pid); if (entry) { subPlanState = new Map(subPlanState).set(pid, { ...entry, subSteps: entry.subSteps.map(s => s.id === sid ? { ...s, status: st } : s) }) } break }
      case 'sub_plan_completed': { const pid = kind.parent_step_id as string; const entry = subPlanState.get(pid); if (entry) { subPlanState = new Map(subPlanState).set(pid, { ...entry, completed: true }); messageTimers.push(setTimeout(() => { const n = new Map(subPlanState); n.delete(pid); subPlanState = n }, 800)) } break }
      case 'teammate_message': { const ts = Date.now() + Math.random(); const msg = { fromStepId: kind.from_step_id as string, toStepId: kind.to_step_id as string, fromName: kind.from_name as string, toName: kind.to_name as string, summary: kind.summary as string, _ts: ts }; teammateMessages = [...teammateMessages, msg]; messageTimers.push(setTimeout(() => { teammateMessages = teammateMessages.filter(m => (m as typeof msg)._ts !== ts) }, 3000)); break }
    }
  }

  // ── Exec play modes ───────────────────────────────────────────
  function playPlanMap() {
    resetExec(); execMode = 'planmap'
    stepSummaries = selectedScenario.steps.map(s => ({ id: s.id, name: s.name, depends_on: s.depends_on, verify_after: s.verify_after, execution_mode: s.execution_mode }))
    steps = selectedScenario.steps.map(s => ({ id: s.id, name: s.name, role: s.role, status: 'pending' as const, duration_ms: null, tokens_used: null, execution_mode: s.execution_mode, verify_passed: null }))
    const planEvt = selectedScenario.events.find(e => (e.kind as Record<string, unknown>).type === 'plan_generated')
    if (planEvt) { const t = (planEvt.kind as Record<string, unknown>).team as Record<string, unknown> | null; teamInfo = t ? { name: t.name as string, topology: t.topology as string, member_count: t.member_count as number } : null }
    isPlaying = true; playStartTime = Date.now()
    timerHandle = setInterval(() => { elapsedMs = Date.now() - (playStartTime ?? Date.now()) }, 100)
    // Include replan/progress events; skip first plan_generated (already handled above)
    let skippedFirstPlan = false
    const stepEvents = selectedScenario.events.filter(e => {
      const t = (e.kind as Record<string, unknown>).type as string
      if (t === 'plan_generated' && !skippedFirstPlan) { skippedFirstPlan = true; return false }
      return ['step_started', 'step_completed', 'verify_gate_result', 'plan_generated', 'replan_triggered', 'progress', 'budget_warning', 'teammate_message', 'sub_plan_started', 'sub_step_progress', 'sub_plan_completed'].includes(t)
    })
    const baseTime = stepEvents.length > 0 ? new Date(stepEvents[0].ts).getTime() : Date.now()
    const timeouts: ReturnType<typeof setTimeout>[] = []
    for (const event of stepEvents) { timeouts.push(setTimeout(() => handleEvent(event), new Date(event.ts).getTime() - baseTime)) }
    if (stepEvents.length > 0) { const lastDelay = new Date(stepEvents[stepEvents.length - 1].ts).getTime() - baseTime; timeouts.push(setTimeout(() => { isPlaying = false; if (timerHandle) clearInterval(timerHandle) }, lastDelay + 1000)) }
    cancelPlayback = () => { timeouts.forEach(clearTimeout) }
  }

  function playClassifying() {
    resetExec(); execMode = 'classifying'
    missionRequest = 'Refactor the auth module to use JWT tokens with refresh flow'
    complexity = 'Medium+ (5 steps, 2 parallel groups)'
    isPlaying = true; playStartTime = Date.now()
    timerHandle = setInterval(() => { elapsedMs = Date.now() - (playStartTime ?? Date.now()) }, 100)
    const t = setTimeout(() => { isPlaying = false; if (timerHandle) clearInterval(timerHandle) }, 13000)
    cancelPlayback = () => clearTimeout(t)
  }

  function playExplorer() {
    resetExec(); execMode = 'explorer'
    isPlaying = true; playStartTime = Date.now()
    timerHandle = setInterval(() => { elapsedMs = Date.now() - (playStartTime ?? Date.now()) }, 100)
    explorationPhase = 'generating'
    const timers: ReturnType<typeof setTimeout>[] = []
    timers.push(setTimeout(() => { explorationPhase = 'comparing' }, 3500))
    timers.push(setTimeout(() => { explorationPhase = 'scoring' }, 5500))
    timers.push(setTimeout(() => { explorationPhase = 'selecting' }, 7500))
    timers.push(setTimeout(() => { explorationPhase = 'done' }, 9000))
    timers.push(setTimeout(() => { isPlaying = false; if (timerHandle) clearInterval(timerHandle) }, 10000))
    cancelPlayback = () => timers.forEach(clearTimeout)
  }

  function playFullFlow() {
    resetExec()
    isPlaying = true; playStartTime = Date.now()
    timerHandle = setInterval(() => { elapsedMs = Date.now() - (playStartTime ?? Date.now()) }, 100)
    const player = createEventPlayer({
      onEvent: handleEvent,
      onExploration: (p) => { explorationPhase = p },
      onThought: (t) => { thoughts = [...thoughts, t] },
      onTeammateMessage: (msg) => { const ts = Date.now() + Math.random(); const tagged = { ...msg, _ts: ts }; teammateMessages = [...teammateMessages, tagged]; messageTimers.push(setTimeout(() => { teammateMessages = teammateMessages.filter(m => (m as typeof tagged)._ts !== ts) }, 3000)) },
      onSubPlan: (update) => { const pid = update.parentStepId; if (update.type === 'started' && update.subSteps) { subPlanState = new Map(subPlanState).set(pid, { subSteps: update.subSteps.map(s => ({ ...s, status: 'pending' as const })), completed: false }) } else if (update.type === 'progress' && update.subStepId && update.status) { const e = subPlanState.get(pid); if (e) subPlanState = new Map(subPlanState).set(pid, { ...e, subSteps: e.subSteps.map(s => s.id === update.subStepId ? { ...s, status: update.status! } : s) }) } else if (update.type === 'completed') { const e = subPlanState.get(pid); if (e) { subPlanState = new Map(subPlanState).set(pid, { ...e, completed: true }); messageTimers.push(setTimeout(() => { const n = new Map(subPlanState); n.delete(pid); subPlanState = n }, 800)) } } },
    })
    player.pushBatch(selectedScenario.events)
    const maxDuration = selectedScenario.events.length * 3000
    const t = setTimeout(() => { isPlaying = false; if (timerHandle) clearInterval(timerHandle) }, maxDuration)
    cancelPlayback = () => { player.stop(); clearTimeout(t) }
  }

  // ── Step status cycling ───────────────────────────────────────
  function cycleStepStatus(id: string) {
    const order: StepRow['status'][] = ['pending', 'running', 'passed', 'failed']
    steps = steps.map(s => s.id !== id ? s : { ...s, status: order[(order.indexOf(s.status) + 1) % order.length] })
  }

  let teamSteps = $derived(steps.filter(s => s.execution_mode === 'teammate'))
  let subagentSteps = $derived(steps.filter(s => s.execution_mode === 'subagent'))

  // ── Scorecard dimension editing ───────────────────────────────
  function updateDimension(dim: ScorecardDimension, val: number) {
    scorecardData = {
      ...scorecardData,
      dimensions: scorecardData.dimensions.map(d => d.dimension === dim ? { ...d, score: val } : d),
      overall_score: scorecardData.dimensions.reduce((sum, d) => sum + (d.dimension === dim ? val : d.score), 0) / 7,
    }
  }

  // ── Review override sync ──────────────────────────────────────
  function applyReviewOverrides() {
    reviewData = {
      ...reviewData,
      merge_readiness: reviewMerge,
      trust_verdict: reviewTrust === 'null' ? null : reviewTrust,
      gate_verdict: reviewGate === 'null' ? null : reviewGate,
      baseline_freshness: reviewData.baseline_freshness ? { ...reviewData.baseline_freshness, freshness: reviewFreshness } : null,
    }
  }

  const DIMENSIONS: ScorecardDimension[] = ['Success', 'TrustVerdict', 'VerificationCoverage', 'MissionContinuity', 'CostEfficiency', 'ReplanStability', 'ErrorRate']
  const DIM_SHORT: Record<ScorecardDimension, string> = { Success: 'Success', TrustVerdict: 'Trust', VerificationCoverage: 'Coverage', MissionContinuity: 'Continuity', CostEfficiency: 'Cost', ReplanStability: 'Stability', ErrorRate: 'Errors' }
</script>

<div class="h-screen flex flex-col bg-bg text-text-2">
  <!-- Tab bar -->
  <header class="flex items-center gap-1 px-3 py-2 border-b border-border bg-surface shrink-0">
    <h1 class="text-sm font-mono text-text-1 font-semibold tracking-wide mr-3">DEVTOOL</h1>
    {#each TABS as tab}
      <button
        onclick={() => { activeTab = tab.id; if (tab.id !== 'execution' && tab.id !== 'full') resetExec() }}
        class="px-3 py-1.5 text-xs font-mono rounded transition-all
          {activeTab === tab.id
            ? `bg-${tab.color}/15 text-${tab.color} border border-${tab.color}/30 ring-1 ring-${tab.color}/10`
            : 'text-text-3 hover:text-text-1 border border-transparent hover:bg-surface-2'}"
      >
        <span class="mr-1 text-[10px]">{tab.icon}</span>{tab.label}
      </button>
    {/each}

    <div class="flex-1"></div>

    {#if isPlaying}
      <span class="text-xs font-mono text-cyan flex items-center gap-1.5">
        <span class="pip pip-active"></span>
        {(elapsedMs / 1000).toFixed(1)}s
      </span>
    {/if}
  </header>

  <!-- Main area -->
  <div class="flex-1 flex overflow-hidden min-h-0">

    <!-- ═══════════════════════════════════════════════════════ -->
    <!-- EXECUTION TAB                                          -->
    <!-- ═══════════════════════════════════════════════════════ -->
    {#if activeTab === 'execution'}
      <!-- Viz area -->
      <div class="flex-1 flex flex-col min-w-0">
        {#if execMode === 'idle'}
          <div class="flex-1 flex items-center justify-center">
            <div class="text-center space-y-3 max-w-sm">
              <div class="text-3xl opacity-20">▸</div>
              <p class="text-sm text-text-3">Select a mode to test execution components</p>
            </div>
          </div>
        {:else if execMode === 'planmap'}
          <div class="flex-1 relative">
            <PlanMap {steps} selectedId={selectedStepId} onSelect={(id) => selectedStepId = id} {thoughts} {stepSummaries} {teamInfo} {teammateMessages} {subPlanState} />
          </div>
        {:else if execMode === 'classifying'}
          <div class="flex-1 relative">
            <ClassifyingScene mission={missionRequest} {complexity} />
          </div>
        {:else if execMode === 'explorer'}
          <div class="flex-1 relative">
            <PlanExplorer phase={explorationPhase} />
          </div>
        {/if}
      </div>

      <!-- Exec sidebar -->
      <div class="w-72 border-l border-border bg-surface flex flex-col shrink-0 overflow-y-auto">
        <div class="px-3 py-2 border-b border-border bg-surface-2">
          <h3 class="text-[10px] font-mono text-text-3 uppercase tracking-widest">Execution Controls</h3>
        </div>
        <div class="p-3 space-y-4 text-xs font-mono">
          <!-- Scenario -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Scenario</h4>
            <select class="w-full bg-surface-2 border border-border text-text-2 text-xs font-mono rounded px-2 py-1.5 outline-none"
              onchange={(e) => { const s = SCENARIOS.find(sc => sc.id === (e.target as HTMLSelectElement).value); if (s) { selectedScenario = s; resetExec() } }}>
              {#each SCENARIOS as s (s.id)}
                <option value={s.id} selected={s.id === selectedScenario.id}>{s.name}</option>
              {/each}
            </select>
            <div class="text-text-3 text-[10px] mt-1">{selectedScenario.description}</div>
          </section>

          <!-- Modes -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Mode</h4>
            <div class="grid grid-cols-2 gap-1.5">
              <button onclick={playPlanMap} class="px-2 py-1.5 rounded text-[10px] transition-colors {execMode === 'planmap' ? 'bg-cyan/15 text-cyan border border-cyan/30' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}">PlanMap</button>
              <button onclick={playClassifying} class="px-2 py-1.5 rounded text-[10px] transition-colors {execMode === 'classifying' ? 'bg-cyan/15 text-cyan border border-cyan/30' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}">Classifying</button>
              <button onclick={playExplorer} class="px-2 py-1.5 rounded text-[10px] transition-colors {execMode === 'explorer' ? 'bg-purple/15 text-purple border border-purple/30' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}">Explorer</button>
              <button onclick={resetExec} class="px-2 py-1.5 rounded text-[10px] bg-surface-2 text-text-3 border border-border hover:text-red hover:border-red/30 transition-colors">Reset</button>
            </div>
          </section>

          <!-- Steps -->
          {#if steps.length > 0}
            <section>
              <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Steps ({steps.length})</h4>
              <div class="space-y-1">
                {#each steps as step (step.id)}
                  <button onclick={() => cycleStepStatus(step.id)}
                    class="w-full text-left flex items-center gap-1.5 px-2 py-1 rounded bg-surface-2 hover:bg-surface-3 border border-border transition-colors {selectedStepId === step.id ? 'border-blue/40' : ''}">
                    <span class="pip {step.status === 'running' ? 'pip-active' : step.status === 'passed' ? 'pip-done' : step.status === 'failed' ? 'pip-fail' : 'pip-idle'}"></span>
                    <span class="text-text-2 truncate flex-1 text-[10px]">{step.name}</span>
                    <span class="text-text-3 text-[9px]">{step.status}</span>
                  </button>
                {/each}
              </div>
              <p class="text-text-3 mt-1 text-[9px] opacity-60">Click to cycle status</p>
            </section>
          {/if}

          <!-- Events -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Events ({events.length})</h4>
            <div class="max-h-32 overflow-y-auto space-y-0.5">
              {#each events.slice(-8) as event (event.seq)}
                <div class="text-text-3 text-[9px] truncate">#{event.seq} {(event.kind as Record<string, unknown>).type}</div>
              {/each}
            </div>
          </section>
        </div>
      </div>

    <!-- ═══════════════════════════════════════════════════════ -->
    <!-- SCORECARD TAB                                          -->
    <!-- ═══════════════════════════════════════════════════════ -->
    {:else if activeTab === 'scorecard'}
      <div class="flex-1 flex items-start justify-center p-6 overflow-y-auto">
        <div class="w-full max-w-md">
          <ScorecardRadar scorecard={scorecardData} />
        </div>
      </div>

      <div class="w-80 border-l border-border bg-surface flex flex-col shrink-0 overflow-y-auto">
        <div class="px-3 py-2 border-b border-border bg-surface-2">
          <h3 class="text-[10px] font-mono text-text-3 uppercase tracking-widest">Scorecard Controls</h3>
        </div>
        <div class="p-3 space-y-4 text-xs font-mono">
          <!-- Presets -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Preset</h4>
            <div class="space-y-1">
              {#each SCORECARD_PRESETS as p (p.id)}
                <button
                  onclick={() => { scorecardPreset = p; scorecardData = structuredClone(p.data) }}
                  class="w-full text-left px-2.5 py-1.5 rounded transition-colors
                    {scorecardPreset.id === p.id ? 'bg-green/10 text-green border border-green/20' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                >
                  <div class="text-[10px] font-medium">{p.name}</div>
                  <div class="text-[9px] opacity-60 mt-0.5">{p.description}</div>
                </button>
              {/each}
            </div>
          </section>

          <!-- Dimension sliders -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Dimensions</h4>
            <div class="space-y-2.5">
              {#each DIMENSIONS as dim}
                {@const score = scorecardData.dimensions.find(d => d.dimension === dim)?.score ?? 0}
                <div>
                  <div class="flex justify-between mb-0.5">
                    <span class="text-text-3 text-[10px]">{DIM_SHORT[dim]}</span>
                    <span class="text-text-2 text-[10px]">{(score * 100).toFixed(0)}%</span>
                  </div>
                  <input type="range" min="0" max="100" value={Math.round(score * 100)}
                    oninput={(e) => updateDimension(dim, parseInt((e.target as HTMLInputElement).value) / 100)}
                    class="w-full h-1 bg-surface-3 rounded-full appearance-none cursor-pointer accent-green"
                  />
                </div>
              {/each}
            </div>
          </section>

          <!-- Overall -->
          <section>
            <div class="flex justify-between px-2 py-2 rounded bg-surface-2 border border-border">
              <span class="text-text-3">Overall</span>
              <span class="text-text-1 font-bold">{(scorecardData.overall_score * 100).toFixed(0)}%</span>
            </div>
          </section>
        </div>
      </div>

    <!-- ═══════════════════════════════════════════════════════ -->
    <!-- GATE TAB                                               -->
    <!-- ═══════════════════════════════════════════════════════ -->
    {:else if activeTab === 'gate'}
      <div class="flex-1 p-6 overflow-y-auto">
        <div class="max-w-lg mx-auto">
          <GateVerdict result={gateData} />
        </div>
      </div>

      <div class="w-80 border-l border-border bg-surface flex flex-col shrink-0 overflow-y-auto">
        <div class="px-3 py-2 border-b border-border bg-surface-2">
          <h3 class="text-[10px] font-mono text-text-3 uppercase tracking-widest">Gate Controls</h3>
        </div>
        <div class="p-3 space-y-4 text-xs font-mono">
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Preset</h4>
            <div class="space-y-1">
              {#each GATE_PRESETS as p (p.id)}
                <button
                  onclick={() => { gatePreset = p; gateData = structuredClone(p.data) }}
                  class="w-full text-left px-2.5 py-1.5 rounded transition-colors
                    {gatePreset.id === p.id ? 'bg-amber/10 text-amber border border-amber/20' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                >
                  <div class="text-[10px] font-medium">{p.name}</div>
                  <div class="text-[9px] opacity-60 mt-0.5">{p.description}</div>
                </button>
              {/each}
            </div>
          </section>

          <!-- Override verdict -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Override Verdict</h4>
            <div class="grid grid-cols-3 gap-1">
              {#each ['Pass', 'Warn', 'Fail'] as v}
                <button
                  onclick={() => { gateData = { ...gateData, verdict: v as GateVerdictType } }}
                  class="px-2 py-1.5 rounded text-[10px] transition-colors
                    {gateData.verdict === v
                      ? (v === 'Pass' ? 'bg-green/15 text-green border border-green/30' : v === 'Warn' ? 'bg-amber/15 text-amber border border-amber/30' : 'bg-red/15 text-red border border-red/30')
                      : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                >{v}</button>
              {/each}
            </div>
          </section>

          <!-- Dimension overrides -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Per-Dimension Verdict</h4>
            <div class="space-y-1">
              {#each gateData.dimension_checks as check, i}
                <div class="flex items-center gap-2 px-2 py-1 rounded bg-surface-2 border border-border">
                  <span class="text-[9px] text-text-3 w-16 truncate">{DIM_SHORT[check.dimension]}</span>
                  {#each ['Pass', 'Warn', 'Fail'] as v}
                    <button
                      onclick={() => { const checks = [...gateData.dimension_checks]; checks[i] = { ...checks[i], verdict: v as GateVerdictType }; gateData = { ...gateData, dimension_checks: checks } }}
                      class="w-5 h-5 rounded text-[8px] flex items-center justify-center transition-colors
                        {check.verdict === v
                          ? (v === 'Pass' ? 'bg-green text-bg' : v === 'Warn' ? 'bg-amber text-bg' : 'bg-red text-bg')
                          : 'bg-surface-3 text-text-3 hover:bg-surface-hover'}"
                    >{v[0]}</button>
                  {/each}
                </div>
              {/each}
            </div>
          </section>
        </div>
      </div>

    <!-- ═══════════════════════════════════════════════════════ -->
    <!-- MISSION TAB                                            -->
    <!-- ═══════════════════════════════════════════════════════ -->
    {:else if activeTab === 'mission'}
      <div class="flex-1 min-w-0">
        <MissionPanel mission={missionData} />
      </div>

      <div class="w-72 border-l border-border bg-surface flex flex-col shrink-0 overflow-y-auto">
        <div class="px-3 py-2 border-b border-border bg-surface-2">
          <h3 class="text-[10px] font-mono text-text-3 uppercase tracking-widest">Mission Controls</h3>
        </div>
        <div class="p-3 space-y-4 text-xs font-mono">
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Preset</h4>
            <div class="space-y-1">
              {#each MISSION_PRESETS as p (p.id)}
                <button
                  onclick={() => { missionPreset = p; missionData = structuredClone(p.data) }}
                  class="w-full text-left px-2.5 py-1.5 rounded transition-colors
                    {missionPreset.id === p.id ? 'bg-purple/10 text-purple border border-purple/20' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                >
                  <div class="text-[10px] font-medium">{p.name}</div>
                  <div class="text-[9px] opacity-60 mt-0.5">{p.description}</div>
                </button>
              {/each}
            </div>
          </section>

          <!-- Quick actions -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Quick Actions</h4>
            <div class="space-y-1.5">
              <button onclick={() => { missionData = { ...missionData, facts: [...missionData.facts, { content: `New fact #${missionData.facts.length + 1}`, source: 'manual', established_at: new Date().toISOString() }] } }}
                class="w-full px-2 py-1.5 rounded text-[10px] bg-green/10 text-green border border-green/20 hover:bg-green/20 transition-colors">+ Add Fact</button>
              <button onclick={() => { missionData = { ...missionData, hypotheses: [...missionData.hypotheses, { content: `Hypothesis #${missionData.hypotheses.length + 1}`, confidence_pct: 50, supporting_evidence: [] }] } }}
                class="w-full px-2 py-1.5 rounded text-[10px] bg-amber/10 text-amber border border-amber/20 hover:bg-amber/20 transition-colors">+ Add Hypothesis</button>
              <button onclick={() => { missionData = { ...missionData, open_questions: [...missionData.open_questions, `Question #${missionData.open_questions.length + 1}?`] } }}
                class="w-full px-2 py-1.5 rounded text-[10px] bg-cyan/10 text-cyan border border-cyan/20 hover:bg-cyan/20 transition-colors">+ Add Question</button>
              <button onclick={() => { missionData = { ...missionData, risks: [...missionData.risks, `Risk #${missionData.risks.length + 1}`] } }}
                class="w-full px-2 py-1.5 rounded text-[10px] bg-red/10 text-red border border-red/20 hover:bg-red/20 transition-colors">+ Add Risk</button>
            </div>
          </section>

          <!-- Phase override -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Plan Phase</h4>
            <div class="grid grid-cols-2 gap-1">
              {#each ['planning', 'executing', 'verifying', 'complete'] as ph}
                <button
                  onclick={() => { missionData = { ...missionData, plan: { ...missionData.plan, phase: ph } } }}
                  class="px-2 py-1.5 rounded text-[10px] transition-colors
                    {missionData.plan.phase === ph ? 'bg-purple/15 text-purple border border-purple/30' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                >{ph}</button>
              {/each}
            </div>
          </section>

          <!-- Stats -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Stats</h4>
            <div class="space-y-0.5 text-[10px] text-text-3">
              <div>Facts: <span class="text-green">{missionData.facts.length}</span></div>
              <div>Hypotheses: <span class="text-amber">{missionData.hypotheses.length}</span></div>
              <div>Questions: <span class="text-cyan">{missionData.open_questions.length}</span></div>
              <div>Risks: <span class="text-red">{missionData.risks.length}</span></div>
              <div>Modified files: <span class="text-text-2">{missionData.modified_files.length}</span></div>
              <div>Decisions: <span class="text-purple">{missionData.key_decisions.length}</span></div>
            </div>
          </section>
        </div>
      </div>

    <!-- ═══════════════════════════════════════════════════════ -->
    <!-- REVIEW TAB                                             -->
    <!-- ═══════════════════════════════════════════════════════ -->
    {:else if activeTab === 'review'}
      <div class="flex-1 min-w-0">
        <PostRunPanel review={reviewData} />
      </div>

      <div class="w-80 border-l border-border bg-surface flex flex-col shrink-0 overflow-y-auto">
        <div class="px-3 py-2 border-b border-border bg-surface-2">
          <h3 class="text-[10px] font-mono text-text-3 uppercase tracking-widest">Review Controls</h3>
        </div>
        <div class="p-3 space-y-4 text-xs font-mono">
          <!-- Presets -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Preset</h4>
            <div class="space-y-1">
              {#each REVIEW_PRESETS as p (p.id)}
                <button
                  onclick={() => {
                    reviewPreset = p; reviewData = structuredClone(p.data)
                    reviewMerge = p.data.merge_readiness
                    reviewTrust = p.data.trust_verdict ?? 'null'
                    reviewGate = p.data.gate_verdict ?? 'null'
                    reviewFreshness = p.data.baseline_freshness?.freshness ?? 'Unknown'
                  }}
                  class="w-full text-left px-2.5 py-1.5 rounded transition-colors
                    {reviewPreset.id === p.id ? 'bg-blue/10 text-blue border border-blue/20' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                >
                  <div class="text-[10px] font-medium">{p.name}</div>
                  <div class="text-[9px] opacity-60 mt-0.5">{p.description}</div>
                </button>
              {/each}
            </div>
          </section>

          <!-- Override controls -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Overrides</h4>
            <div class="space-y-2.5">
              <!-- Merge Readiness -->
              <div>
                <div class="text-[10px] text-text-3 mb-1">Merge Readiness</div>
                <div class="grid grid-cols-2 gap-1">
                  {#each ['Ready', 'ConditionallyReady', 'NotReady', 'Unknown'] as v}
                    <button onclick={() => { reviewMerge = v as MergeReadiness; applyReviewOverrides() }}
                      class="px-1.5 py-1 rounded text-[9px] transition-colors
                        {reviewMerge === v ? 'bg-blue/15 text-blue border border-blue/30' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                    >{v === 'ConditionallyReady' ? 'Conditional' : v}</button>
                  {/each}
                </div>
              </div>

              <!-- Trust Verdict -->
              <div>
                <div class="text-[10px] text-text-3 mb-1">Trust Verdict</div>
                <div class="grid grid-cols-5 gap-1">
                  {#each ['High', 'Medium', 'Low', 'None', 'null'] as v}
                    <button onclick={() => { reviewTrust = v as TrustVerdict | 'null'; applyReviewOverrides() }}
                      class="px-1 py-1 rounded text-[9px] transition-colors
                        {reviewTrust === v ? 'bg-blue/15 text-blue border border-blue/30' : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                    >{v === 'null' ? '---' : v}</button>
                  {/each}
                </div>
              </div>

              <!-- Gate Verdict -->
              <div>
                <div class="text-[10px] text-text-3 mb-1">Gate Verdict</div>
                <div class="grid grid-cols-4 gap-1">
                  {#each ['Pass', 'Warn', 'Fail', 'null'] as v}
                    <button onclick={() => { reviewGate = v as GateVerdictType | 'null'; applyReviewOverrides() }}
                      class="px-1.5 py-1 rounded text-[9px] transition-colors
                        {reviewGate === v ? (v === 'Pass' ? 'bg-green/15 text-green border border-green/30' : v === 'Warn' ? 'bg-amber/15 text-amber border border-amber/30' : v === 'Fail' ? 'bg-red/15 text-red border border-red/30' : 'bg-blue/15 text-blue border border-blue/30') : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                    >{v === 'null' ? '---' : v}</button>
                  {/each}
                </div>
              </div>

              <!-- Baseline Freshness -->
              <div>
                <div class="text-[10px] text-text-3 mb-1">Baseline Freshness</div>
                <div class="grid grid-cols-4 gap-1">
                  {#each ['Fresh', 'Aging', 'Stale', 'Unknown'] as v}
                    <button onclick={() => { reviewFreshness = v as BaselineFreshness; applyReviewOverrides() }}
                      class="px-1.5 py-1 rounded text-[9px] transition-colors
                        {reviewFreshness === v ? (v === 'Fresh' ? 'bg-green/15 text-green border border-green/30' : v === 'Aging' ? 'bg-amber/15 text-amber border border-amber/30' : v === 'Stale' ? 'bg-red/15 text-red border border-red/30' : 'bg-surface-3 text-text-3 border border-border') : 'bg-surface-2 text-text-3 border border-border hover:text-text-1'}"
                    >{v}</button>
                  {/each}
                </div>
              </div>
            </div>
          </section>
        </div>
      </div>

    <!-- ═══════════════════════════════════════════════════════ -->
    <!-- FULL FLOW TAB                                          -->
    <!-- ═══════════════════════════════════════════════════════ -->
    {:else if activeTab === 'full'}
      <div class="flex-1 flex flex-col min-w-0">
        <div class="h-[60%] border-b border-border relative">
          <PlanExplorer phase={explorationPhase} />
          {#if explorationPhase === 'done' || steps.length > 0}
            <PlanMap {steps} selectedId={selectedStepId} onSelect={(id) => selectedStepId = id} {thoughts} {stepSummaries} {teamInfo} {teammateMessages} {subPlanState} />
          {:else if explorationPhase === 'idle' && missionRequest}
            <ClassifyingScene mission={missionRequest} {complexity} />
          {/if}
        </div>
        <div class="flex-1 overflow-hidden">
          <Timeline events={events} selectedSeq={null} onSelect={() => {}} />
        </div>
      </div>

      <div class="w-72 border-l border-border bg-surface flex flex-col shrink-0 overflow-y-auto">
        <div class="px-3 py-2 border-b border-border bg-surface-2">
          <h3 class="text-[10px] font-mono text-text-3 uppercase tracking-widest">Full Flow</h3>
        </div>
        <div class="p-3 space-y-4 text-xs font-mono">
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Scenario</h4>
            <select class="w-full bg-surface-2 border border-border text-text-2 text-xs font-mono rounded px-2 py-1.5 outline-none"
              onchange={(e) => { const s = SCENARIOS.find(sc => sc.id === (e.target as HTMLSelectElement).value); if (s) { selectedScenario = s; resetExec() } }}>
              {#each SCENARIOS as s (s.id)}
                <option value={s.id} selected={s.id === selectedScenario.id}>{s.name}</option>
              {/each}
            </select>
          </section>
          <section>
            <div class="space-y-1.5">
              <button onclick={playFullFlow}
                class="w-full px-3 py-2 rounded text-[10px] bg-green/10 text-green border border-green/20 hover:bg-green/20 transition-colors font-medium">
                {isPlaying ? 'Playing...' : 'Play Full Flow'}
              </button>
              <button onclick={resetExec}
                class="w-full px-3 py-1.5 rounded text-[10px] bg-surface-2 text-text-3 border border-border hover:text-red hover:border-red/30 transition-colors">
                Reset
              </button>
            </div>
          </section>
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">State</h4>
            <div class="space-y-0.5 text-[10px] text-text-3">
              <div>Events: <span class="text-text-2">{events.length}</span></div>
              <div>Steps: <span class="text-text-2">{steps.filter(s => s.status === 'passed').length}/{steps.length}</span></div>
              <div>Exploration: <span class="text-purple">{explorationPhase}</span></div>
            </div>
          </section>
        </div>
      </div>
    {/if}
  </div>

  <!-- Footer -->
  <footer class="flex items-center gap-3 px-4 py-1.5 border-t border-border bg-surface text-[10px] font-mono text-text-3 shrink-0">
    <span class="uppercase tracking-wider">{activeTab}</span>
    {#if activeTab === 'scorecard'}
      <span>·</span><span>Preset: {scorecardPreset.name}</span>
      <span>·</span><span>Overall: <span class="text-text-2">{(scorecardData.overall_score * 100).toFixed(0)}%</span></span>
    {:else if activeTab === 'gate'}
      <span>·</span><span>Preset: {gatePreset.name}</span>
      <span>·</span><span class="{gateData.verdict === 'Pass' ? 'text-green' : gateData.verdict === 'Warn' ? 'text-amber' : 'text-red'}">{gateData.verdict}</span>
    {:else if activeTab === 'mission'}
      <span>·</span><span>Preset: {missionPreset.name}</span>
      <span>·</span><span>Phase: {missionData.plan.phase}</span>
    {:else if activeTab === 'review'}
      <span>·</span><span>Preset: {reviewPreset.name}</span>
      <span>·</span><span>{reviewData.merge_readiness}</span>
    {:else if activeTab === 'execution'}
      <span>·</span><span>Scenario: {selectedScenario.id}</span>
      <span>·</span><span>Mode: {execMode}</span>
    {:else if activeTab === 'full'}
      <span>·</span><span>{steps.filter(s => s.status === 'passed').length}/{steps.length} steps</span>
    {/if}
    <span class="ml-auto">localhost:5174/?playground</span>
  </footer>
</div>
