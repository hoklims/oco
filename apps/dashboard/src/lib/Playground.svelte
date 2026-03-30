<script lang="ts">
  import { SCENARIOS, type PlaygroundScenario } from './playground-data'
  import type { DashboardEvent, StepRow, BudgetSnapshot } from './types'
  import type { Thought } from './demo'
  import { createEventPlayer } from './event-player'
  import PlanMap from './PlanMap.svelte'
  import PlanExplorer from './PlanExplorer.svelte'
  import ClassifyingScene from './ClassifyingScene.svelte'
  import Timeline from './Timeline.svelte'

  // ── State ──────────────────────────────────────────────────
  let selectedScenario = $state<PlaygroundScenario>(SCENARIOS[0])
  let mode = $state<'idle' | 'planmap' | 'classifying' | 'explorer' | 'full'>('idle')
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

  // Sub-plan state driven by events (Map: parentStepId → SubPlanEntry)
  let subPlanState = $state<Map<string, { subSteps: Array<{ id: string; name: string; status: 'pending' | 'running' | 'passed' | 'failed' }>; completed: boolean }>>(new Map())

  // Debug panel
  let showDebug = $state(true)

  // Timer for elapsed display
  let timerHandle: ReturnType<typeof setInterval> | null = null

  // Step summaries for DAG layout (with depends_on)
  let stepSummaries = $state<Array<{
    id: string; name: string; depends_on: string[]; verify_after: boolean; execution_mode: string
  }>>([])

  // ── Cleanup ────────────────────────────────────────────────
  let cancelPlayback: (() => void) | null = null

  function cleanup() {
    cancelPlayback?.()
    cancelPlayback = null
    if (timerHandle) { clearInterval(timerHandle); timerHandle = null }
    isPlaying = false
    playStartTime = null
  }

  function resetState() {
    cleanup()
    events = []; steps = []; thoughts = []; budget = null
    explorationPhase = 'idle'; selectedStepId = null
    missionRequest = ''; complexity = ''; elapsedMs = 0
    stepSummaries = []; teamInfo = null; teammateMessages = []; subPlanState = new Map()
    messageTimers.forEach(clearTimeout); messageTimers = []
    mode = 'idle'
  }

  // ── Event handler (shared with full flow) ──────────────────
  function handleEvent(event: DashboardEvent) {
    events = [...events, event]
    const kind = event.kind as Record<string, unknown>
    const type = kind.type as string

    switch (type) {
      case 'run_started':
        missionRequest = kind.request_summary as string
        break
      case 'plan_generated':
        stepSummaries = ((kind.steps as Array<Record<string, unknown>>) ?? []).map(s => ({
          id: s.id as string,
          name: s.name as string,
          depends_on: s.depends_on as string[],
          verify_after: s.verify_after as boolean,
          execution_mode: s.execution_mode as string,
        }))
        steps = stepSummaries.map(s => ({
          id: s.id, name: s.name, role: ((kind.steps as Array<Record<string, unknown>>)?.find(ss => ss.id === s.id)?.role as string) ?? 'implementer',
          status: 'pending' as const, duration_ms: null, tokens_used: null,
          execution_mode: s.execution_mode, verify_passed: null,
        }))
        { const t = kind.team as Record<string, unknown> | null
          teamInfo = t ? { name: t.name as string, topology: t.topology as string, member_count: t.member_count as number } : null }
        break
      case 'step_started':
        steps = steps.map(s => s.id === (kind.step_id as string) ? { ...s, status: 'running' as const } : s)
        break
      case 'step_completed': {
        const id = kind.step_id as string
        steps = steps.map(s => s.id === id ? { ...s,
          status: (kind.success ? 'passed' : 'failed') as StepRow['status'],
          duration_ms: kind.duration_ms as number,
          tokens_used: kind.tokens_used as number,
        } : s)
        break
      }
      case 'verify_gate_result':
        steps = steps.map(s => s.id === (kind.step_id as string) ? { ...s, verify_passed: kind.overall_passed as boolean } : s)
        break
      case 'progress': {
        const snap = kind.budget as BudgetSnapshot | undefined
        if (snap?.tokens_used !== undefined) budget = snap
        break
      }
      case 'sub_plan_started': {
        const parentId = kind.parent_step_id as string
        const subSteps = ((kind.sub_steps as Array<Record<string, unknown>>) ?? []).map(s => ({
          id: s.id as string, name: s.name as string, status: 'pending' as const,
        }))
        subPlanState = new Map(subPlanState).set(parentId, { subSteps, completed: false })
        break
      }
      case 'sub_step_progress': {
        const parentId = kind.parent_step_id as string
        const subStepId = kind.sub_step_id as string
        const status = kind.status as 'pending' | 'running' | 'passed' | 'failed'
        const entry = subPlanState.get(parentId)
        if (entry) {
          const updated = entry.subSteps.map(s => s.id === subStepId ? { ...s, status } : s)
          subPlanState = new Map(subPlanState).set(parentId, { ...entry, subSteps: updated })
        }
        break
      }
      case 'sub_plan_completed': {
        const parentId = kind.parent_step_id as string
        const entry = subPlanState.get(parentId)
        if (entry) {
          // Mark completed, will be removed after collapse animation
          subPlanState = new Map(subPlanState).set(parentId, { ...entry, completed: true })
          // Remove after 800ms collapse animation
          messageTimers.push(setTimeout(() => {
            const next = new Map(subPlanState); next.delete(parentId); subPlanState = next
          }, 800))
        }
        break
      }
      case 'teammate_message': {
        const ts = Date.now() + Math.random()
        const msg = {
          fromStepId: kind.from_step_id as string,
          toStepId: kind.to_step_id as string,
          fromName: kind.from_name as string,
          toName: kind.to_name as string,
          summary: kind.summary as string,
          _ts: ts,
        }
        teammateMessages = [...teammateMessages, msg]
        // Auto-clear after 3s so the flash disappears
        messageTimers.push(setTimeout(() => {
          teammateMessages = teammateMessages.filter(m => (m as typeof msg)._ts !== ts)
        }, 3000))
        break
      }
    }
  }

  // ── Play modes ─────────────────────────────────────────────

  function playPlanMap() {
    resetState()
    mode = 'planmap'

    // Load steps from scenario directly (instant, no animation delay)
    stepSummaries = selectedScenario.steps.map(s => ({
      id: s.id, name: s.name, depends_on: s.depends_on,
      verify_after: s.verify_after, execution_mode: s.execution_mode,
    }))
    steps = selectedScenario.steps.map(s => ({
      id: s.id, name: s.name, role: s.role,
      status: 'pending' as const, duration_ms: null, tokens_used: null,
      execution_mode: s.execution_mode, verify_passed: null,
    }))
    // Extract team info from plan_generated event if present
    const planEvt = selectedScenario.events.find(e => (e.kind as Record<string, unknown>).type === 'plan_generated')
    if (planEvt) {
      const t = (planEvt.kind as Record<string, unknown>).team as Record<string, unknown> | null
      teamInfo = t ? { name: t.name as string, topology: t.topology as string, member_count: t.member_count as number } : null
    }

    // Then play step events to animate status changes
    isPlaying = true
    playStartTime = Date.now()
    timerHandle = setInterval(() => { elapsedMs = Date.now() - (playStartTime ?? Date.now()) }, 100)

    const stepEvents = selectedScenario.events.filter(e => {
      const t = (e.kind as Record<string, unknown>).type as string
      return ['step_started', 'step_completed', 'verify_gate_result',
        'teammate_message', 'sub_plan_started', 'sub_step_progress', 'sub_plan_completed'].includes(t)
    })

    const baseTime = stepEvents.length > 0 ? new Date(stepEvents[0].ts).getTime() : Date.now()
    const timeouts: ReturnType<typeof setTimeout>[] = []

    for (const event of stepEvents) {
      const delay = new Date(event.ts).getTime() - baseTime
      timeouts.push(setTimeout(() => handleEvent(event), delay))
    }

    // Auto-stop
    if (stepEvents.length > 0) {
      const lastDelay = new Date(stepEvents[stepEvents.length - 1].ts).getTime() - baseTime
      timeouts.push(setTimeout(() => { isPlaying = false; if (timerHandle) clearInterval(timerHandle) }, lastDelay + 1000))
    }

    cancelPlayback = () => { timeouts.forEach(clearTimeout) }
  }

  function playClassifying() {
    resetState()
    mode = 'classifying'
    missionRequest = 'Refactor the auth module to use JWT tokens with refresh flow'
    complexity = 'Medium+ (5 steps, 2 parallel groups)'
    isPlaying = true
    playStartTime = Date.now()
    timerHandle = setInterval(() => { elapsedMs = Date.now() - (playStartTime ?? Date.now()) }, 100)

    const t = setTimeout(() => { isPlaying = false; if (timerHandle) clearInterval(timerHandle) }, 13000)
    cancelPlayback = () => clearTimeout(t)
  }

  function playExplorer() {
    resetState()
    mode = 'explorer'
    isPlaying = true
    playStartTime = Date.now()
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
    resetState()
    mode = 'full'
    isPlaying = true
    playStartTime = Date.now()
    timerHandle = setInterval(() => { elapsedMs = Date.now() - (playStartTime ?? Date.now()) }, 100)

    const player = createEventPlayer({
      onEvent: handleEvent,
      onExploration: (p) => { explorationPhase = p },
      onThought: (t) => { thoughts = [...thoughts, t] },
      onTeammateMessage: (msg) => {
        const ts = Date.now() + Math.random()
        const tagged = { ...msg, _ts: ts }
        teammateMessages = [...teammateMessages, tagged]
        messageTimers.push(setTimeout(() => { teammateMessages = teammateMessages.filter(m => (m as typeof tagged)._ts !== ts) }, 3000))
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
            messageTimers.push(setTimeout(() => { const n = new Map(subPlanState); n.delete(pid); subPlanState = n }, 800))
          }
        }
      },
    })

    player.pushBatch(selectedScenario.events)

    // Stop after reasonable timeout based on event count
    const maxDuration = selectedScenario.events.length * 3000
    const t = setTimeout(() => { isPlaying = false; if (timerHandle) clearInterval(timerHandle) }, maxDuration)

    cancelPlayback = () => { player.stop(); clearTimeout(t) }
  }

  // ── Step status override (manual testing) ──────────────────
  function cycleStepStatus(id: string) {
    const order: StepRow['status'][] = ['pending', 'running', 'passed', 'failed']
    steps = steps.map(s => {
      if (s.id !== id) return s
      const idx = order.indexOf(s.status)
      return { ...s, status: order[(idx + 1) % order.length] }
    })
  }

  // ── Manual communication controls ─────────────────────────
  let teamSteps = $derived(steps.filter(s => s.execution_mode === 'teammate'))
  let subagentSteps = $derived(steps.filter(s => s.execution_mode === 'subagent'))

  function sendManualMessage() {
    if (teamSteps.length < 2) return
    const from = teamSteps[0]
    const to = teamSteps[1]
    const ts = Date.now() + Math.random()
    const msg = {
      fromStepId: from.id, toStepId: to.id,
      fromName: from.name.split(' ').slice(0, 2).join(' '),
      toName: to.name.split(' ').slice(0, 2).join(' '),
      summary: `Sync on shared interface (manual #${teammateMessages.length + 1})`,
      _ts: ts,
    }
    teammateMessages = [...teammateMessages, msg]
    messageTimers.push(setTimeout(() => { teammateMessages = teammateMessages.filter(m => (m as typeof msg)._ts !== ts) }, 3000))
  }

  function triggerSubPlan(stepId: string) {
    if (subPlanState.has(stepId)) return
    const subs = [
      { id: `${stepId}-sub-1`, name: 'Analyze scope' },
      { id: `${stepId}-sub-2`, name: 'Implement changes' },
      { id: `${stepId}-sub-3`, name: 'Verify results' },
    ]
    subPlanState = new Map(subPlanState).set(stepId, {
      subSteps: subs.map(s => ({ ...s, status: 'pending' as const })),
      completed: false,
    })
  }

  function progressNextSubStep(stepId: string) {
    const entry = subPlanState.get(stepId)
    if (!entry) return
    // Find first pending → running, or first running → passed
    const pendingIdx = entry.subSteps.findIndex(s => s.status === 'pending')
    const runningIdx = entry.subSteps.findIndex(s => s.status === 'running')
    if (runningIdx >= 0) {
      subPlanState = new Map(subPlanState).set(stepId, {
        ...entry, subSteps: entry.subSteps.map((s, i) => i === runningIdx ? { ...s, status: 'passed' } : s),
      })
    } else if (pendingIdx >= 0) {
      subPlanState = new Map(subPlanState).set(stepId, {
        ...entry, subSteps: entry.subSteps.map((s, i) => i === pendingIdx ? { ...s, status: 'running' } : s),
      })
    }
  }

  function completeSubPlan(stepId: string) {
    const entry = subPlanState.get(stepId)
    if (!entry) return
    subPlanState = new Map(subPlanState).set(stepId, {
      ...entry, subSteps: entry.subSteps.map(s => ({ ...s, status: 'passed' as const })), completed: true,
    })
    messageTimers.push(setTimeout(() => { const n = new Map(subPlanState); n.delete(stepId); subPlanState = n }, 800))
  }
</script>

<div class="h-screen flex flex-col bg-bg text-text-2">
  <!-- Toolbar -->
  <header class="flex items-center gap-3 px-4 py-2.5 border-b border-border bg-surface shrink-0">
    <h1 class="text-sm font-mono text-text-1 font-semibold tracking-wide">PLAYGROUND</h1>
    <div class="h-4 w-px bg-border"></div>

    <!-- Scenario selector -->
    <select
      class="bg-surface-2 border border-border text-text-2 text-xs font-mono rounded px-2 py-1 outline-none focus:border-blue/50"
      onchange={(e) => {
        const target = e.target as HTMLSelectElement
        const s = SCENARIOS.find(sc => sc.id === target.value)
        if (s) { selectedScenario = s; resetState() }
      }}
    >
      {#each SCENARIOS as s (s.id)}
        <option value={s.id} selected={s.id === selectedScenario.id}>{s.name}</option>
      {/each}
    </select>

    <div class="h-4 w-px bg-border"></div>

    <!-- Play buttons -->
    <button onclick={playPlanMap} class="px-2.5 py-1 text-xs font-mono rounded transition-colors
      {mode === 'planmap' ? 'bg-cyan/15 text-cyan border border-cyan/30' : 'bg-surface-2 text-text-3 hover:text-text-1 border border-border hover:border-border-2'}">
      PlanMap
    </button>
    <button onclick={playClassifying} class="px-2.5 py-1 text-xs font-mono rounded transition-colors
      {mode === 'classifying' ? 'bg-cyan/15 text-cyan border border-cyan/30' : 'bg-surface-2 text-text-3 hover:text-text-1 border border-border hover:border-border-2'}">
      Classifying
    </button>
    <button onclick={playExplorer} class="px-2.5 py-1 text-xs font-mono rounded transition-colors
      {mode === 'explorer' ? 'bg-purple/15 text-purple border border-purple/30' : 'bg-surface-2 text-text-3 hover:text-text-1 border border-border hover:border-border-2'}">
      Explorer
    </button>
    <button onclick={playFullFlow} class="px-2.5 py-1 text-xs font-mono rounded transition-colors
      {mode === 'full' ? 'bg-green/15 text-green border border-green/30' : 'bg-surface-2 text-text-3 hover:text-text-1 border border-border hover:border-border-2'}">
      Full Flow
    </button>

    <div class="h-4 w-px bg-border"></div>
    <button onclick={resetState} class="px-2.5 py-1 text-xs font-mono bg-surface-2 text-text-3 hover:text-red border border-border hover:border-red/30 rounded transition-colors">
      Reset
    </button>

    <div class="flex-1"></div>

    <!-- Status -->
    {#if isPlaying}
      <span class="text-xs font-mono text-cyan flex items-center gap-1.5">
        <span class="pip pip-active"></span>
        Playing {(elapsedMs / 1000).toFixed(1)}s
      </span>
    {:else if mode !== 'idle'}
      <span class="text-xs font-mono text-text-3">Stopped</span>
    {/if}

    <button
      onclick={() => showDebug = !showDebug}
      class="px-2 py-1 text-xs font-mono text-text-3 hover:text-text-1 bg-surface-2 border border-border rounded transition-colors"
    >
      {showDebug ? 'Hide' : 'Show'} Debug
    </button>
  </header>

  <!-- Main area -->
  <div class="flex-1 flex overflow-hidden min-h-0">
    <!-- Visualization area -->
    <div class="flex-1 flex flex-col min-w-0 relative">
      {#if mode === 'idle'}
        <div class="flex-1 flex items-center justify-center">
          <div class="text-center space-y-4 max-w-md">
            <div class="text-4xl opacity-20">◇</div>
            <h2 class="text-lg text-text-1 font-medium">OCO Dashboard Playground</h2>
            <p class="text-sm text-text-3 leading-relaxed">
              Select a scenario and click a play button to test individual components or the full flow.
            </p>
            <div class="text-xs text-text-3 font-mono space-y-1">
              <p><strong class="text-cyan">PlanMap</strong> — DAG visualization with node status animation</p>
              <p><strong class="text-cyan">Classifying</strong> — Abstract analysis scene (~6.5s)</p>
              <p><strong class="text-purple">Explorer</strong> — Plan exploration with two branches (~10s)</p>
              <p><strong class="text-green">Full Flow</strong> — Complete event playback via EventPlayer</p>
            </div>
          </div>
        </div>

      {:else if mode === 'planmap'}
        <div class="flex-1 relative">
          <PlanMap {steps} selectedId={selectedStepId} onSelect={(id) => selectedStepId = id} {thoughts} {stepSummaries} {teamInfo} {teammateMessages} {subPlanState} />
        </div>

      {:else if mode === 'classifying'}
        <div class="flex-1 relative">
          <ClassifyingScene mission={missionRequest} {complexity} />
        </div>

      {:else if mode === 'explorer'}
        <div class="flex-1 relative">
          <PlanExplorer phase={explorationPhase} />
        </div>

      {:else if mode === 'full'}
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
      {/if}
    </div>

    <!-- Debug panel -->
    {#if showDebug}
      <div class="w-80 border-l border-border bg-surface flex flex-col shrink-0 overflow-y-auto">
        <div class="px-3 py-2 border-b border-border bg-surface-2">
          <h3 class="text-xs font-mono text-text-1 uppercase tracking-wider">Debug State</h3>
        </div>

        <div class="p-3 space-y-4 text-xs font-mono">
          <!-- Scenario info -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Scenario</h4>
            <div class="text-text-2">{selectedScenario.name}</div>
            <div class="text-text-3 mt-0.5">{selectedScenario.description}</div>
            <div class="text-text-3 mt-0.5">{selectedScenario.steps.length} steps, {selectedScenario.events.length} events</div>
          </section>

          <!-- Current mode -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Mode</h4>
            <div class="text-text-2">{mode}</div>
            <div class="text-text-3">explorationPhase: <span class="text-purple">{explorationPhase}</span></div>
            <div class="text-text-3">isPlaying: <span class="{isPlaying ? 'text-green' : 'text-text-3'}">{isPlaying}</span></div>
          </section>

          <!-- Steps state -->
          {#if steps.length > 0}
            <section>
              <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Steps ({steps.length})</h4>
              <div class="space-y-1.5">
                {#each steps as step (step.id)}
                  <button
                    onclick={() => cycleStepStatus(step.id)}
                    class="w-full text-left flex items-center gap-2 px-2 py-1 rounded bg-surface-2 hover:bg-surface-3 border border-border transition-colors
                      {selectedStepId === step.id ? 'border-blue/40' : ''}"
                  >
                    <span class="pip {
                      step.status === 'running' ? 'pip-active' :
                      step.status === 'passed' ? 'pip-done' :
                      step.status === 'failed' ? 'pip-fail' : 'pip-idle'
                    }"></span>
                    <span class="text-text-2 truncate flex-1">{step.name}</span>
                    <span class="text-text-3 text-[10px]">{step.status}</span>
                    <span class="text-text-3 text-[10px]">{step.execution_mode}</span>
                  </button>
                {/each}
              </div>
              <p class="text-text-3 mt-1.5 text-[10px]">Click a step to cycle: pending → running → passed → failed</p>
            </section>
          {/if}

          <!-- Teammate Messages controls -->
          {#if teamSteps.length >= 2}
            <section>
              <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Communication</h4>
              <div class="space-y-1.5">
                <div class="text-text-3 text-[10px]">Active pulses: <span class="text-purple">{teammateMessages.length}</span></div>
                <button
                  onclick={sendManualMessage}
                  class="w-full px-2 py-1 text-[10px] font-mono rounded bg-purple/10 text-purple border border-purple/20 hover:bg-purple/20 transition-colors"
                >
                  Send Message ({teamSteps[0].name.slice(0, 15)} → {teamSteps[1].name.slice(0, 15)})
                </button>
              </div>
            </section>
          {/if}

          <!-- Sub-Plan controls -->
          {#if subagentSteps.length > 0}
            <section>
              <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Sub-Plans</h4>
              <div class="space-y-1.5">
                {#each subagentSteps as sa (sa.id)}
                  {@const sp = subPlanState.get(sa.id)}
                  <div class="px-2 py-1.5 rounded bg-surface-2 border border-border space-y-1">
                    <div class="text-text-2 text-[10px] truncate">{sa.name}</div>
                    {#if sp}
                      <div class="space-y-0.5">
                        {#each sp.subSteps as sub (sub.id)}
                          <div class="text-[10px] flex items-center gap-1.5">
                            <span class="pip {
                              sub.status === 'running' ? 'pip-active' :
                              sub.status === 'passed' ? 'pip-done' :
                              sub.status === 'failed' ? 'pip-fail' : 'pip-idle'
                            }"></span>
                            <span class="text-text-3 truncate">{sub.name}</span>
                            <span class="text-text-3 ml-auto">{sub.status}</span>
                          </div>
                        {/each}
                      </div>
                      <div class="flex gap-1">
                        <button
                          onclick={() => progressNextSubStep(sa.id)}
                          class="flex-1 px-1.5 py-0.5 text-[10px] font-mono rounded bg-amber/10 text-amber border border-amber/20 hover:bg-amber/20 transition-colors"
                        >
                          Progress
                        </button>
                        <button
                          onclick={() => completeSubPlan(sa.id)}
                          class="flex-1 px-1.5 py-0.5 text-[10px] font-mono rounded bg-green/10 text-green border border-green/20 hover:bg-green/20 transition-colors"
                        >
                          Complete
                        </button>
                      </div>
                    {:else}
                      <button
                        onclick={() => triggerSubPlan(sa.id)}
                        class="w-full px-1.5 py-0.5 text-[10px] font-mono rounded bg-amber/10 text-amber border border-amber/20 hover:bg-amber/20 transition-colors"
                      >
                        Start Sub-Plan
                      </button>
                    {/if}
                  </div>
                {/each}
              </div>
            </section>
          {/if}

          <!-- Events -->
          <section>
            <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Events ({events.length})</h4>
            <div class="max-h-40 overflow-y-auto space-y-0.5">
              {#each events.slice(-10) as event (event.seq)}
                <div class="text-text-3 text-[10px] truncate">
                  #{event.seq} {(event.kind as Record<string, unknown>).type}
                </div>
              {/each}
            </div>
          </section>

          <!-- Thoughts -->
          {#if thoughts.length > 0}
            <section>
              <h4 class="text-text-3 uppercase tracking-wider mb-1.5">Thoughts ({thoughts.length})</h4>
              <div class="max-h-32 overflow-y-auto space-y-0.5">
                {#each thoughts.slice(-5) as t (t.text)}
                  <div class="text-text-3 text-[10px] truncate">[{t.variant}] {t.text}</div>
                {/each}
              </div>
            </section>
          {/if}
        </div>
      </div>
    {/if}
  </div>

  <!-- Footer -->
  <footer class="flex items-center gap-3 px-4 py-1.5 border-t border-border bg-surface text-xs font-mono text-text-3 shrink-0">
    <span>Scenario: {selectedScenario.id}</span>
    <span>·</span>
    <span>Mode: {mode}</span>
    <span>·</span>
    <span>{steps.filter(s => s.status === 'passed').length}/{steps.length} completed</span>
    {#if budget}
      <span>·</span>
      <span>{budget.tokens_used.toLocaleString()} tok</span>
    {/if}
    <span class="ml-auto">localhost:5174</span>
  </footer>
</div>
