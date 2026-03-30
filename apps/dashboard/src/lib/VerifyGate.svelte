<script lang="ts">
  import { Handle, Position } from '@xyflow/svelte'

  let { data }: { data: Record<string, unknown> } = $props()

  let passed = $derived(data.passed as boolean | null)

  let bg = $derived(passed === true ? 'rgba(52,211,153,0.12)'
    : passed === false ? 'rgba(248,113,113,0.12)'
    : 'rgba(92,99,120,0.06)')

  let border = $derived(passed === true ? '#34d399'
    : passed === false ? '#f87171'
    : '#5c637840')

  let icon = $derived(passed === true ? '✓'
    : passed === false ? '✗'
    : '◆')

  let iconColor = $derived(passed === true ? '#34d399'
    : passed === false ? '#f87171'
    : '#5c6378')
</script>

<div
  class="verify-gate {passed != null ? 'verify-gate-resolved' : ''}"
  style="
    background: {bg};
    border: 1.5px solid {border};
    transform: rotate(45deg);
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
    transition: all 0.5s cubic-bezier(0.4, 0, 0.2, 1);
  "
>
  <Handle type="target" position={Position.Left} />
  <Handle type="source" position={Position.Right} />
  <span style="transform: rotate(-45deg); font-size: 11px; color: {iconColor}; font-weight: 700; line-height: 1;">{icon}</span>
</div>

<style>
  .verify-gate-resolved {
    box-shadow: 0 0 8px rgba(52, 211, 153, 0.15);
  }
  :global(.verify-gate .svelte-flow__handle) {
    opacity: 0 !important;
    width: 1px !important;
    height: 1px !important;
  }
</style>
