<script lang="ts">
  import { Handle, Position } from '@xyflow/svelte'

  let { data }: { data: Record<string, unknown> } = $props()

  let name = $derived(data.name as string)
  let role = $derived((data.role as string).toLowerCase())
  let status = $derived(data.status as 'pending' | 'running' | 'passed' | 'failed')
  let executionMode = $derived(data.execution_mode as string)
  let verifyPassed = $derived(data.verify_passed as boolean | null)
  let durationMs = $derived(data.duration_ms as number | null)
  let tokensUsed = $derived(data.tokens_used as number | null)

  // ── Role colors ────────────────────────────────────────────
  const roleColor: Record<string, { bg: string; border: string; text: string; glow: string }> = {
    scout:       { bg: 'rgba(75,141,248,0.06)',  border: '#4b8df830', text: '#4b8df8', glow: 'rgba(75,141,248,0.2)' },
    architect:   { bg: 'rgba(167,139,250,0.06)', border: '#a78bfa30', text: '#a78bfa', glow: 'rgba(167,139,250,0.2)' },
    implementer: { bg: 'rgba(34,211,238,0.06)',  border: '#22d3ee30', text: '#22d3ee', glow: 'rgba(34,211,238,0.2)' },
    tester:      { bg: 'rgba(251,191,36,0.06)',  border: '#fbbf2430', text: '#fbbf24', glow: 'rgba(251,191,36,0.2)' },
    verifier:    { bg: 'rgba(52,211,153,0.06)',  border: '#34d39930', text: '#34d399', glow: 'rgba(52,211,153,0.2)' },
    planner:     { bg: 'rgba(167,139,250,0.06)', border: '#a78bfa30', text: '#a78bfa', glow: 'rgba(167,139,250,0.2)' },
    reviewer:    { bg: 'rgba(52,211,153,0.06)',  border: '#34d39930', text: '#34d399', glow: 'rgba(52,211,153,0.2)' },
  }

  let rc = $derived(roleColor[role] ?? { bg: 'rgba(92,99,120,0.06)', border: '#5c637830', text: '#5c6378', glow: 'rgba(92,99,120,0.2)' })

  const roleLabel: Record<string, string> = {
    scout: 'SCOUT', explorer: 'EXPLORER', architect: 'ARCH',
    implementer: 'IMPL', verifier: 'VERIFY', reviewer: 'REVIEW',
    planner: 'PLAN', tester: 'TEST',
  }

  // ── Execution mode — stripe + pill ─────────────────────────
  // inline  = no stripe (default, clean)
  // subagent = amber stripe + "FORK" pill
  // teammate = purple stripe + "TEAM" pill
  const MODE_COLORS = {
    subagent: { stripe: '#fbbf24', pill: '#fbbf24', pillBg: 'rgba(251,191,36,0.12)', label: 'FORK' },
    teammate: { stripe: '#a78bfa', pill: '#a78bfa', pillBg: 'rgba(167,139,250,0.12)', label: 'TEAM' },
  } as Record<string, { stripe: string; pill: string; pillBg: string; label: string }>

  let modeStyle = $derived(MODE_COLORS[executionMode] ?? null)

  // Left accent stripe: 3px solid color for non-inline modes
  let stripeStyle = $derived(modeStyle ? `border-left: 3px solid ${modeStyle.stripe};` : '')

  // ── Status styles ──────────────────────────────────────────
  let statusBorder = $derived(
    status === 'running' ? rc.text
    : status === 'passed' ? '#34d399'
    : status === 'failed' ? '#f87171'
    : rc.border)

  let statusBg = $derived(
    status === 'running' ? rc.bg
    : status === 'passed' ? 'rgba(52,211,153,0.04)'
    : status === 'failed' ? 'rgba(248,113,113,0.04)'
    : rc.bg)

  let statusGlow = $derived(status === 'running' ? `0 0 16px ${rc.glow}` : 'none')
  let statusIcon = $derived(status === 'passed' ? '✓' : status === 'failed' ? '✗' : '')
</script>

<div
  class="dag-node {status === 'running' ? 'dag-node-running' : ''} {status === 'pending' ? 'dag-node-pending' : ''}"
  style="
    background: {statusBg};
    border: 1px solid {statusBorder};
    border-radius: 10px;
    padding: 10px 14px;
    min-width: 150px;
    max-width: 210px;
    box-shadow: {statusGlow};
    {stripeStyle}
    transition: all 0.6s cubic-bezier(0.4, 0, 0.2, 1);
  "
>
  <Handle type="target" position={Position.Left} />
  <Handle type="source" position={Position.Right} />

  <!-- Row 1: role label + status icon -->
  <div class="dag-header">
    <span class="dag-role" style="color: {rc.text}">{roleLabel[role] ?? role.toUpperCase()}</span>
    {#if status === 'running'}
      <span class="dag-pip dag-pip-running"></span>
    {:else if statusIcon}
      <span class="dag-status-icon" style="color: {status === 'passed' ? '#34d399' : '#f87171'}">{statusIcon}</span>
    {/if}
    {#if verifyPassed === true}
      <span class="dag-verify dag-verify-pass">V</span>
    {:else if verifyPassed === false}
      <span class="dag-verify dag-verify-fail">V</span>
    {/if}
  </div>

  <!-- Row 2: name -->
  <div class="dag-name">{name}</div>

  <!-- Row 3: mode pill + stats -->
  <div class="dag-footer">
    {#if modeStyle}
      <span class="dag-mode-pill" style="color: {modeStyle.pill}; background: {modeStyle.pillBg}">
        {modeStyle.label}
      </span>
    {/if}
    {#if durationMs != null}
      <span class="dag-stat">{(durationMs / 1000).toFixed(1)}s</span>
      {#if tokensUsed != null}
        <span class="dag-stat">{tokensUsed.toLocaleString()} tok</span>
      {/if}
    {/if}
  </div>
</div>

<style>
  .dag-node {
    font-family: var(--font-sans, system-ui);
  }
  .dag-node-pending {
    opacity: 0.4;
  }
  .dag-node-running {
    animation: dag-glow 2.5s ease-in-out infinite;
  }
  @keyframes dag-glow {
    0%, 100% { filter: brightness(1); }
    50% { filter: brightness(1.15); }
  }

  .dag-header {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 5px;
  }
  .dag-role {
    font-size: 10px;
    font-family: ui-monospace, monospace;
    font-weight: 600;
    letter-spacing: 0.08em;
  }
  .dag-pip {
    width: 6px; height: 6px; border-radius: 2px; margin-left: auto;
  }
  .dag-pip-running {
    background: #22d3ee;
    box-shadow: 0 0 6px rgba(34, 211, 238, 0.6);
    animation: pip-breathe 2s ease-in-out infinite;
  }
  @keyframes pip-breathe {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(0.7); }
  }
  .dag-status-icon {
    font-size: 12px; font-weight: 700; margin-left: auto; line-height: 1;
  }
  .dag-verify {
    font-size: 9px; font-weight: 700; font-family: ui-monospace, monospace;
    padding: 1px 4px; border-radius: 3px; line-height: 1;
  }
  .dag-verify-pass { background: rgba(52,211,153,0.15); color: #34d399; }
  .dag-verify-fail { background: rgba(248,113,113,0.15); color: #f87171; }

  .dag-name {
    font-size: 13px; color: #e8ecf4; font-weight: 500; line-height: 1.3;
  }

  .dag-footer {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
    min-height: 18px;
  }
  .dag-mode-pill {
    font-size: 9px;
    font-family: ui-monospace, monospace;
    font-weight: 700;
    letter-spacing: 0.1em;
    padding: 2px 6px;
    border-radius: 4px;
    line-height: 1;
  }
  .dag-stat {
    font-size: 11px;
    font-family: ui-monospace, monospace;
    color: #5c6378;
  }

  :global(.dag-node .svelte-flow__handle) {
    opacity: 0 !important;
    width: 1px !important;
    height: 1px !important;
  }
</style>
