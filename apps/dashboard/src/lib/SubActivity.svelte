<script lang="ts">
  /**
   * SubActivity — mini agent card showing a sub-agent's current work.
   * Clearly communicates "this is an independent worker doing a task".
   */
  import { Handle, Position } from '@xyflow/svelte'

  let { data }: { data: Record<string, unknown> } = $props()

  let label = $derived(data.label as string)
  let subStatus = $derived(data.subStatus as 'pending' | 'running' | 'passed')
  let agentIndex = $derived((data.agentIndex as number) ?? 0)

  let agentName = $derived(`Agent ${agentIndex + 1}`)

  let statusColor = $derived(
    subStatus === 'running' ? '#fbbf24' :
    subStatus === 'passed' ? '#34d399' :
    '#5c6378'
  )
  let statusIcon = $derived(
    subStatus === 'running' ? '▸' :
    subStatus === 'passed' ? '✓' :
    '·'
  )
  let borderColor = $derived(
    subStatus === 'running' ? '#fbbf2440' :
    subStatus === 'passed' ? '#34d39930' :
    '#1c203040'
  )
  let bg = $derived(
    subStatus === 'running' ? 'rgba(251,191,36,0.06)' :
    subStatus === 'passed' ? 'rgba(52,211,153,0.04)' :
    'rgba(13,15,20,0.6)'
  )
</script>

<div
  class="sub-agent {subStatus === 'running' ? 'sub-agent-running' : ''} {subStatus === 'pending' ? 'sub-agent-pending' : ''}"
  style="background: {bg}; border-color: {borderColor};"
>
  <Handle type="target" position={Position.Left} />
  <Handle type="source" position={Position.Right} />

  <!-- Agent identity -->
  <div class="sub-header">
    <span class="sub-pip" style="background: {statusColor}; box-shadow: 0 0 4px {statusColor}40;"></span>
    <span class="sub-name">{agentName}</span>
    <span class="sub-status-icon" style="color: {statusColor}">{statusIcon}</span>
  </div>

  <!-- Task being performed -->
  <div class="sub-task" style="color: {subStatus === 'pending' ? '#3c4152' : '#9aa0b4'}">
    {label}
  </div>
</div>

<style>
  .sub-agent {
    border: 1px solid;
    border-left: 2px solid #fbbf2450;
    border-radius: 8px;
    padding: 7px 10px;
    min-width: 130px;
    font-family: var(--font-sans, system-ui);
    transition: all 0.4s cubic-bezier(0.4, 0, 0.2, 1);
  }
  .sub-agent-pending {
    opacity: 0.3;
  }
  .sub-agent-running {
    animation: sub-active 2.5s ease-in-out infinite;
  }
  @keyframes sub-active {
    0%, 100% { filter: brightness(1); }
    50% { filter: brightness(1.12); }
  }

  .sub-header {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-bottom: 3px;
  }
  .sub-pip {
    width: 5px;
    height: 5px;
    border-radius: 2px;
    flex-shrink: 0;
  }
  .sub-name {
    font-size: 9px;
    font-family: ui-monospace, monospace;
    font-weight: 600;
    color: #fbbf24;
    letter-spacing: 0.06em;
    opacity: 0.7;
  }
  .sub-status-icon {
    font-size: 10px;
    margin-left: auto;
    line-height: 1;
  }

  .sub-task {
    font-size: 11px;
    line-height: 1.2;
    font-weight: 500;
  }

  :global(.sub-agent .svelte-flow__handle) {
    opacity: 0 !important;
    width: 1px !important;
    height: 1px !important;
  }
</style>
