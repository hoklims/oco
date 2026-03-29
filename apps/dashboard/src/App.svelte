<script lang="ts">
  import './app.css'
  import { connectSSE, type SSEStatus } from './lib/sse'
  import type { DashboardEvent, BudgetSnapshot, StepRow } from './lib/types'
  import EventLog from './lib/EventLog.svelte'
  import StepTable from './lib/StepTable.svelte'
  import BudgetGauge from './lib/BudgetGauge.svelte'

  let events = $state<DashboardEvent[]>([])
  let steps = $state<StepRow[]>([])
  let budget = $state<BudgetSnapshot | null>(null)
  let status = $state<SSEStatus>('closed')
  let replayId = $state('')
  let runDir = $state('')
  let speed = $state(1)
  let connected = $state(false)

  async function createReplay() {
    if (!runDir.trim()) return
    const res = await fetch('/api/v1/dashboard/replays', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ run_dir: runDir, speed }),
    })
    if (!res.ok) {
      const err = await res.json()
      alert(err.error || 'Failed to create replay')
      return
    }
    const data = await res.json()
    replayId = data.replay_id
    connectToReplay(data.stream_url)
  }

  let client: ReturnType<typeof connectSSE> | null = null

  function connectToReplay(streamUrl: string) {
    client?.close()
    events = []
    steps = []
    budget = null
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
      case 'plan_generated': {
        const planSteps = (kind.steps as Array<Record<string, unknown>>) ?? []
        steps = planSteps.map(s => ({
          id: s.id as string,
          name: s.name as string,
          role: s.role as string,
          status: 'pending' as const,
          duration_ms: null,
          tokens_used: null,
          execution_mode: s.execution_mode as string,
          verify_passed: null,
        }))
        break
      }
      case 'step_started': {
        const id = kind.step_id as string
        steps = steps.map(s => s.id === id ? { ...s, status: 'running' as const } : s)
        break
      }
      case 'step_completed': {
        const id = kind.step_id as string
        steps = steps.map(s => s.id === id ? {
          ...s,
          status: (kind.success ? 'passed' : 'failed') as StepRow['status'],
          duration_ms: kind.duration_ms as number,
          tokens_used: kind.tokens_used as number,
        } : s)
        break
      }
      case 'verify_gate_result': {
        const id = kind.step_id as string
        steps = steps.map(s => s.id === id ? {
          ...s,
          verify_passed: kind.overall_passed as boolean,
        } : s)
        break
      }
      case 'progress':
      case 'flat_step_completed': {
        const snap = (kind.budget ?? kind.budget_snapshot) as BudgetSnapshot | undefined
        if (snap && snap.tokens_used !== undefined) budget = snap
        break
      }
    }
  }

  async function pause() { await fetch(`/api/v1/dashboard/replays/${replayId}/pause`, { method: 'POST' }) }
  async function resume() { await fetch(`/api/v1/dashboard/replays/${replayId}/resume`, { method: 'POST' }) }
  async function setSpeed(s: number) {
    speed = s
    await fetch(`/api/v1/dashboard/replays/${replayId}/speed`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ speed: s }),
    })
  }

  function disconnect() {
    client?.close()
    connected = false
    status = 'closed'
  }

  const statusColors: Record<SSEStatus, string> = {
    connecting: 'bg-warning',
    connected: 'bg-success',
    reconnecting: 'bg-warning',
    closed: 'bg-text-dim',
  }
</script>

<div class="h-screen flex flex-col bg-surface text-text">
  <header class="flex items-center justify-between px-4 py-2 border-b border-border bg-surface-2">
    <div class="flex items-center gap-3">
      <h1 class="text-sm font-semibold text-text-bright tracking-wide">OCO Dashboard</h1>
      <span class="text-xs text-text-dim font-mono">v0.10.0</span>
    </div>
    <div class="flex items-center gap-2">
      <span class="w-2 h-2 rounded-full {statusColors[status]}"></span>
      <span class="text-xs text-text-dim">{status}</span>
    </div>
  </header>

  {#if !connected}
    <div class="flex-1 flex items-center justify-center">
      <div class="bg-surface-2 border border-border rounded-xl p-6 w-full max-w-md">
        <h2 class="text-lg font-semibold text-text-bright mb-4">Load a trace</h2>
        <div class="space-y-3">
          <div>
            <label for="rundir" class="text-xs text-text-dim block mb-1">Run directory</label>
            <input
              id="rundir"
              type="text"
              bind:value={runDir}
              placeholder=".oco/runs/abc123..."
              class="w-full bg-surface-3 border border-border rounded-lg px-3 py-2 text-sm text-text outline-none focus:border-accent"
            />
          </div>
          <div>
            <label for="speed" class="text-xs text-text-dim block mb-1">Speed: {speed}x</label>
            <input id="speed" type="range" min="0.5" max="20" step="0.5" bind:value={speed} class="w-full accent-accent" />
          </div>
          <button
            onclick={createReplay}
            disabled={!runDir.trim()}
            class="w-full bg-accent hover:bg-accent/80 disabled:opacity-40 text-white font-medium py-2 rounded-lg text-sm transition-colors"
          >Start replay</button>
        </div>
      </div>
    </div>
  {:else}
    <div class="flex-1 flex overflow-hidden">
      <div class="w-1/2 border-r border-border flex flex-col">
        <div class="px-3 py-2 border-b border-border bg-surface-2">
          <span class="text-xs font-semibold text-text-bright">Event Log</span>
        </div>
        <div class="flex-1 overflow-hidden">
          <EventLog {events} />
        </div>
      </div>

      <div class="w-1/2 flex flex-col">
        <div class="flex items-center gap-2 px-3 py-2 border-b border-border bg-surface-2">
          <button onclick={pause} class="text-xs px-2 py-1 rounded bg-surface-3 hover:bg-border text-text-dim">Pause</button>
          <button onclick={resume} class="text-xs px-2 py-1 rounded bg-surface-3 hover:bg-border text-text-dim">Resume</button>
          {#each [1, 2, 5, 10, 50] as s}
            <button
              onclick={() => setSpeed(s)}
              class="text-xs px-2 py-1 rounded {speed === s ? 'bg-accent text-white' : 'bg-surface-3 text-text-dim hover:bg-border'}"
            >{s}x</button>
          {/each}
          <div class="flex-1"></div>
          <button onclick={disconnect} class="text-xs px-2 py-1 rounded bg-error/20 text-error hover:bg-error/30">Disconnect</button>
        </div>

        <div class="px-3 py-3 border-b border-border">
          <div class="text-xs font-semibold text-text-bright mb-2">Budget</div>
          <BudgetGauge {budget} />
        </div>

        <div class="flex-1 overflow-y-auto">
          <div class="px-3 py-2 border-b border-border bg-surface-2">
            <span class="text-xs font-semibold text-text-bright">Plan Steps</span>
            <span class="text-xs text-text-dim ml-2">{steps.filter(s => s.status === 'passed').length}/{steps.length}</span>
          </div>
          <StepTable {steps} />
        </div>
      </div>
    </div>
  {/if}
</div>
