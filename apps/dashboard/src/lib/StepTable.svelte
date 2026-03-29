<script lang="ts">
  import type { StepRow } from './types'

  let { steps }: { steps: StepRow[] } = $props()

  const statusLabel: Record<StepRow['status'], string> = {
    pending: 'STANDBY',
    running: 'ACTIVE',
    passed: 'DONE',
    failed: 'FAIL',
  }

  const statusPip: Record<StepRow['status'], string> = {
    pending: 'pip-pending',
    running: 'pip-running',
    passed: 'pip-success',
    failed: 'pip-error',
  }

  const statusText: Record<StepRow['status'], string> = {
    pending: 'text-text-dim',
    running: 'text-running',
    passed: 'text-success',
    failed: 'text-error',
  }
</script>

{#if steps.length > 0}
  <div class="divide-y divide-border">
    {#each steps as step, i (step.id)}
      <div class="flex items-center gap-3 px-3 py-2 hover:bg-surface-3/30 transition-colors {step.status === 'running' ? 'bg-running-dim' : ''}">
        <!-- Index -->
        <span class="text-[10px] font-mono text-text-dim w-4 text-right shrink-0">{i + 1}</span>

        <!-- Status pip -->
        <div class="pip {statusPip[step.status]} shrink-0"></div>

        <!-- Name + role -->
        <div class="flex-1 min-w-0">
          <div class="text-xs text-text-bright truncate">{step.name}</div>
          <div class="text-[10px] font-mono text-text-dim flex items-center gap-2">
            <span class="uppercase">{step.role}</span>
            <span class="text-border-bright">|</span>
            <span>{step.execution_mode}</span>
          </div>
        </div>

        <!-- Status label -->
        <span class="text-[10px] font-mono uppercase tracking-wider {statusText[step.status]} w-14 text-right shrink-0">
          {statusLabel[step.status]}
        </span>

        <!-- Duration -->
        <span class="text-[10px] font-mono text-text-dim w-14 text-right shrink-0">
          {step.duration_ms != null ? `${step.duration_ms}ms` : '—'}
        </span>

        <!-- Tokens -->
        <span class="text-[10px] font-mono text-text-dim w-12 text-right shrink-0">
          {step.tokens_used != null ? `${step.tokens_used}` : '—'}
        </span>

        <!-- Verify -->
        <div class="w-4 shrink-0 flex justify-center">
          {#if step.verify_passed === true}
            <div class="pip pip-success"></div>
          {:else if step.verify_passed === false}
            <div class="pip pip-error"></div>
          {:else}
            <div class="pip pip-pending"></div>
          {/if}
        </div>
      </div>
    {/each}
  </div>
{:else}
  <div class="text-[10px] font-mono text-text-dim uppercase tracking-wider text-center py-6">
    No plan — flat execution
  </div>
{/if}
