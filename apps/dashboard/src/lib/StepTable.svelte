<script lang="ts">
  import type { StepRow } from './types'

  let { steps }: { steps: StepRow[] } = $props()

  const statusIcon: Record<StepRow['status'], string> = {
    pending: '○',
    running: '◉',
    passed: '✓',
    failed: '✗',
  }

  const statusColor: Record<StepRow['status'], string> = {
    pending: 'text-text-dim',
    running: 'text-running',
    passed: 'text-success',
    failed: 'text-error',
  }
</script>

{#if steps.length > 0}
  <div class="overflow-x-auto">
    <table class="w-full text-sm">
      <thead>
        <tr class="border-b border-border text-text-dim text-left">
          <th class="py-2 px-3 font-normal w-8"></th>
          <th class="py-2 px-3 font-normal">Step</th>
          <th class="py-2 px-3 font-normal">Role</th>
          <th class="py-2 px-3 font-normal">Mode</th>
          <th class="py-2 px-3 font-normal text-right">Duration</th>
          <th class="py-2 px-3 font-normal text-right">Tokens</th>
          <th class="py-2 px-3 font-normal text-center">Verify</th>
        </tr>
      </thead>
      <tbody>
        {#each steps as step (step.id)}
          <tr class="border-b border-border/50 hover:bg-surface-3/50 transition-colors">
            <td class="py-2 px-3 {statusColor[step.status]} font-mono">
              {statusIcon[step.status]}
            </td>
            <td class="py-2 px-3 text-text-bright font-medium truncate max-w-48">
              {step.name}
            </td>
            <td class="py-2 px-3 text-text-dim">{step.role}</td>
            <td class="py-2 px-3">
              <span class="text-xs px-1.5 py-0.5 rounded bg-accent-dim text-accent">
                {step.execution_mode}
              </span>
            </td>
            <td class="py-2 px-3 text-right font-mono text-text-dim">
              {step.duration_ms != null ? `${step.duration_ms}ms` : '—'}
            </td>
            <td class="py-2 px-3 text-right font-mono text-text-dim">
              {step.tokens_used != null ? step.tokens_used.toLocaleString() : '—'}
            </td>
            <td class="py-2 px-3 text-center">
              {#if step.verify_passed === true}
                <span class="text-success">✓</span>
              {:else if step.verify_passed === false}
                <span class="text-error">✗</span>
              {:else}
                <span class="text-text-dim">—</span>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{:else}
  <div class="text-text-dim text-sm py-4 text-center">No plan steps yet</div>
{/if}
