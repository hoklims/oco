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
  let teammateColor = $derived((data.teammateColor as string | null) ?? null)
  let subSteps = $derived((data.subSteps as Array<{ id: string; name: string; status: string }> | null) ?? null)

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

  const MODE_COLORS = {
    subagent: { stripe: '#fbbf24', pill: '#fbbf24', pillBg: 'rgba(251,191,36,0.12)', label: 'FORK' },
    teammate: { stripe: '#a78bfa', pill: '#a78bfa', pillBg: 'rgba(167,139,250,0.12)', label: 'TEAM' },
  } as Record<string, { stripe: string; pill: string; pillBg: string; label: string }>

  let modeStyle = $derived(MODE_COLORS[executionMode] ?? null)
  let stripeColor = $derived(teammateColor ? teammateColor : modeStyle?.stripe ?? null)
  let stripeStyle = $derived(stripeColor ? `border-left: 3px solid ${stripeColor};` : '')

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

  // Sub-step progress helpers
  let subDone = $derived(subSteps?.filter(s => s.status === 'passed').length ?? 0)
  let subTotal = $derived(subSteps?.length ?? 0)
</script>

<div
  class="dag-node {status === 'running' ? 'dag-node-running' : ''} {status === 'pending' ? 'dag-node-pending' : ''} {status === 'failed' ? 'dag-node-failed' : ''}"
  style="
    background: {statusBg};
    border: 1px solid {statusBorder};
    border-radius: 10px;
    padding: 10px 14px;
    min-width: 150px;
    max-width: 240px;
    box-shadow: {statusGlow};
    {stripeStyle}
    transition: background 0.6s, border-color 0.6s, box-shadow 0.6s, opacity 0.5s, filter 0.8s, scale 0.5s;
    transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
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
      <span class="dag-mode-pill" style="color: {teammateColor ?? modeStyle.pill}; background: {teammateColor ? teammateColor + '18' : modeStyle.pillBg}">
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

  <!-- Sub-step progress (inline, no separate nodes) -->
  {#if subSteps && subSteps.length > 0}
    <div class="sub-divider"></div>
    <div class="sub-strip">
      {#each subSteps as sub (sub.id)}
        <div class="sub-item" class:sub-running={sub.status === 'running'} class:sub-done={sub.status === 'passed'} class:sub-fail={sub.status === 'failed'}>
          <span class="sub-dot"
            style="background: {sub.status === 'running' ? '#fbbf24' : sub.status === 'passed' ? '#34d399' : sub.status === 'failed' ? '#f87171' : '#3c4152'};
              {sub.status === 'running' ? 'box-shadow: 0 0 4px #fbbf2480;' : ''}"
          ></span>
          <span class="sub-label">{sub.name}</span>
          {#if sub.status === 'passed'}
            <span class="sub-check">✓</span>
          {:else if sub.status === 'failed'}
            <span class="sub-fail-icon">✗</span>
          {/if}
        </div>
      {/each}
      <div class="sub-counter">{subDone}/{subTotal}</div>
    </div>
  {/if}
</div>

<style>
  .dag-node {
    font-family: var(--font-sans, system-ui);
    /* Organic entrance — node grows in with slight overshoot */
    animation: dag-emerge 0.5s cubic-bezier(0.34, 1.56, 0.64, 1) both;
  }
  @keyframes dag-emerge {
    from {
      opacity: 0;
      scale: 0.3;
      filter: blur(4px);
    }
    60% {
      opacity: 0.9;
      scale: 1.04;
      filter: blur(0.5px);
    }
    to {
      opacity: 1;
      scale: 1;
      filter: blur(0);
    }
  }

  .dag-node-pending { opacity: 0.4; }

  .dag-node-running {
    animation: dag-glow 2.5s ease-in-out infinite;
  }
  @keyframes dag-glow {
    0%, 100% { filter: brightness(1); }
    50% { filter: brightness(1.15); }
  }

  /* Organic decay — failed node visually decomposes */
  .dag-node-failed {
    animation: dag-decay 0.8s ease-in forwards;
  }
  @keyframes dag-decay {
    0% {
      filter: none;
      scale: 1;
      opacity: 1;
    }
    40% {
      filter: saturate(0.4) brightness(0.9);
      scale: 0.98;
      opacity: 0.85;
    }
    100% {
      filter: saturate(0.15) brightness(0.7) blur(0.5px);
      scale: 0.94;
      opacity: 0.55;
    }
  }

  .dag-header { display: flex; align-items: center; gap: 6px; margin-bottom: 5px; }
  .dag-role { font-size: 10px; font-family: ui-monospace, monospace; font-weight: 600; letter-spacing: 0.08em; }
  .dag-pip { width: 6px; height: 6px; border-radius: 2px; margin-left: auto; }
  .dag-pip-running {
    background: #22d3ee;
    box-shadow: 0 0 6px rgba(34, 211, 238, 0.6);
    animation: pip-breathe 2s ease-in-out infinite;
  }
  @keyframes pip-breathe {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(0.7); }
  }
  .dag-status-icon { font-size: 12px; font-weight: 700; margin-left: auto; line-height: 1; }
  .dag-verify { font-size: 9px; font-weight: 700; font-family: ui-monospace, monospace; padding: 1px 4px; border-radius: 3px; line-height: 1; }
  .dag-verify-pass { background: rgba(52,211,153,0.15); color: #34d399; }
  .dag-verify-fail { background: rgba(248,113,113,0.15); color: #f87171; }
  .dag-name { font-size: 13px; color: #e8ecf4; font-weight: 500; line-height: 1.3; }
  .dag-footer { display: flex; align-items: center; gap: 8px; margin-top: 6px; min-height: 18px; }
  .dag-mode-pill { font-size: 9px; font-family: ui-monospace, monospace; font-weight: 700; letter-spacing: 0.1em; padding: 2px 6px; border-radius: 4px; line-height: 1; }
  .dag-stat { font-size: 11px; font-family: ui-monospace, monospace; color: #5c6378; }

  /* Sub-step progress strip */
  .sub-divider {
    height: 1px;
    background: linear-gradient(90deg, transparent, #fbbf2420, transparent);
    margin: 7px -4px 5px;
  }
  .sub-strip {
    display: flex;
    flex-direction: column;
    gap: 3px;
    position: relative;
  }
  .sub-item {
    display: flex;
    align-items: center;
    gap: 5px;
    font-family: ui-monospace, monospace;
    font-size: 10px;
    color: #5c6378;
    transition: all 0.3s;
  }
  .sub-item.sub-running { color: #fbbf24; }
  .sub-item.sub-done { color: #34d399; }
  .sub-item.sub-fail { color: #f87171; }
  .sub-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    flex-shrink: 0;
    transition: all 0.3s;
  }
  .sub-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 140px;
  }
  .sub-check { font-size: 9px; color: #34d399; margin-left: auto; font-weight: 700; }
  .sub-fail-icon { font-size: 9px; color: #f87171; margin-left: auto; font-weight: 700; }
  .sub-counter {
    font-family: ui-monospace, monospace;
    font-size: 9px;
    color: #5c637860;
    text-align: right;
    margin-top: 1px;
  }

  :global(.dag-node .svelte-flow__handle) {
    opacity: 0 !important;
    width: 1px !important;
    height: 1px !important;
  }
</style>
