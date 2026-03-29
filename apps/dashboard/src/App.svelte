<script lang="ts">
  import './app.css'
  import { connectSSE, type SSEStatus } from './lib/sse'
  import type { DashboardEvent, BudgetSnapshot, StepRow } from './lib/types'
  import MissionCard from './lib/MissionCard.svelte'
  import MissionView from './lib/MissionView.svelte'

  let events = $state<DashboardEvent[]>([])
  let steps = $state<StepRow[]>([])
  let budget = $state<BudgetSnapshot | null>(null)
  let status = $state<SSEStatus>('closed')
  let replayId = $state('')
  let speed = $state(10)
  let connected = $state(false)
  let missionRequest = $state('')

  type Run = {
    id: string; request: string; complexity: string; steps: number
    duration_ms: number; success: boolean; created_at: string; run_dir: string
    tokens_used: number; tokens_max: number; status: string
  }

  let runs = $state<Run[]>([])
  let loading = $state(true)
  let statsTotal = $derived(runs.length)
  let statsSuccess = $derived(runs.filter(r => r.success).length)
  let statsFail = $derived(runs.filter(r => !r.success).length)
  let statsTotalSteps = $derived(runs.reduce((a, r) => a + r.steps, 0))

  $effect(() => { fetchRuns() })

  async function fetchRuns() {
    loading = true
    try {
      const res = await fetch('/api/v1/dashboard/runs?limit=50')
      if (res.ok) runs = await res.json()
    } catch {}
    loading = false
  }

  async function selectRun(runDir: string) {
    const res = await fetch('/api/v1/dashboard/replays', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ run_dir: runDir, speed }),
    })
    if (!res.ok) { alert((await res.json()).error || 'Load failed'); return }
    const data = await res.json()
    replayId = data.replay_id
    const run = runs.find(r => r.run_dir === runDir)
    missionRequest = run?.request ?? ''
    connectToReplay(data.stream_url)
  }

  let client: ReturnType<typeof connectSSE> | null = null

  function connectToReplay(streamUrl: string) {
    client?.close()
    events = []; steps = []; budget = null
    connected = true
    client = connectSSE(streamUrl)
    client.onStatus(s => { status = s })
    client.onEvent(handleEvent)
  }

  function handleEvent(event: DashboardEvent) {
    events = [...events, event]
    const kind = event.kind as Record<string, unknown>
    const type = kind.type as string
    switch (type) {
      case 'plan_generated':
        steps = ((kind.steps as Array<Record<string, unknown>>) ?? []).map(s => ({
          id: s.id as string, name: s.name as string, role: s.role as string,
          status: 'pending' as const, duration_ms: null, tokens_used: null,
          execution_mode: s.execution_mode as string, verify_passed: null,
        }))
        break
      case 'step_started':
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
        steps = steps.map(s => s.id === (kind.step_id as string) ? { ...s, verify_passed: kind.overall_passed as boolean } : s)
        break
      case 'progress': case 'flat_step_completed': {
        const snap = (kind.budget ?? kind.budget_snapshot) as BudgetSnapshot | undefined
        if (snap?.tokens_used !== undefined) budget = snap
        break
      }
    }
  }

  async function pause() { await fetch(`/api/v1/dashboard/replays/${replayId}/pause`, { method: 'POST' }) }
  async function resume() { await fetch(`/api/v1/dashboard/replays/${replayId}/resume`, { method: 'POST' }) }
  async function setSpeed(s: number) {
    speed = s
    if (replayId) await fetch(`/api/v1/dashboard/replays/${replayId}/speed`, {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ speed: s }),
    })
  }

  function disconnect() {
    client?.close()
    connected = false; status = 'closed'
    events = []; steps = []; budget = null; missionRequest = ''
    fetchRuns()
  }
</script>

{#if connected}
  <MissionView
    {events} {steps} {budget} request={missionRequest}
    onDisconnect={disconnect} onPause={pause} onResume={resume} onSpeed={setSpeed} {speed} {status}
  />
{:else}
  <div class="min-h-screen bg-surface scanlines">
    <!-- Header -->
    <header class="border-b border-border bg-surface-2">
      <div class="max-w-5xl mx-auto px-6 py-3 flex items-center justify-between">
        <div>
          <div class="text-[10px] font-mono uppercase tracking-[0.2em] text-text-dim">Open Context Orchestrator</div>
          <div class="text-sm font-semibold text-text-bright mt-0.5">Mission Control</div>
        </div>
        <div class="flex items-center gap-4">
          <!-- Aggregate stats -->
          <div class="flex items-center gap-3 text-[10px] font-mono">
            <div><span class="text-text-dim">RUNS</span> <span class="text-text-bright">{statsTotal}</span></div>
            <div><span class="text-text-dim">PASS</span> <span class="text-success">{statsSuccess}</span></div>
            <div><span class="text-text-dim">FAIL</span> <span class="text-error">{statsFail}</span></div>
            <div><span class="text-text-dim">STEPS</span> <span class="text-text-bright">{statsTotalSteps}</span></div>
          </div>
          <div class="w-px h-6 bg-border"></div>
          <!-- Speed selector -->
          <div class="flex items-center gap-1">
            <span class="text-[10px] font-mono text-text-dim uppercase">Speed</span>
            {#each [1, 5, 10, 50] as s}
              <button
                onclick={() => { speed = s }}
                class="text-[10px] font-mono px-1.5 py-0.5 {speed === s ? 'bg-accent text-white' : 'text-text-dim bg-surface-3 hover:bg-border-bright'} transition-colors"
              >{s}x</button>
            {/each}
          </div>
        </div>
      </div>
    </header>

    <!-- Mission grid -->
    <main class="max-w-5xl mx-auto px-6 py-6">
      {#if loading}
        <div class="text-center py-16 text-[10px] font-mono text-text-dim uppercase tracking-widest">
          Scanning archives...
        </div>
      {:else if runs.length === 0}
        <div class="text-center py-16 border border-border bg-surface-2">
          <div class="text-xs text-text-dim mb-2">No missions recorded</div>
          <div class="text-[10px] font-mono text-text-dim">
            Execute <span class="text-accent">oco run "your task"</span> to begin
          </div>
        </div>
      {:else}
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2">
          {#each runs as run (run.id)}
            <MissionCard {run} onSelect={selectRun} />
          {/each}
        </div>
      {/if}
    </main>
  </div>
{/if}
