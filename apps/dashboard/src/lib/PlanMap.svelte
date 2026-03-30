<script lang="ts">
  import type { StepRow } from './types'
  import type { Thought } from './demo'
  import ThoughtBubble from './ThoughtBubble.svelte'

  let { steps, selectedId, onSelect, thoughts = [] }: {
    steps: StepRow[]
    selectedId: string | null
    onSelect: (id: string) => void
    thoughts?: Thought[]
  } = $props()

  let expanded = $state<Set<string>>(new Set())

  function toggle(id: string) {
    const next = new Set(expanded)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    expanded = next
    onSelect(id)
  }

  const roleLabel: Record<string, string> = {
    scout: 'SCOUT', explorer: 'EXPLORER', architect: 'ARCHITECT',
    implementer: 'IMPL', verifier: 'VERIFIER', reviewer: 'REVIEWER',
    planner: 'PLANNER', tester: 'TESTER',
  }

  const roleBg: Record<string, string> = {
    scout: 'bg-blue/8 border-blue/25 hover:border-blue/40',
    architect: 'bg-purple/8 border-purple/25 hover:border-purple/40',
    implementer: 'bg-cyan/8 border-cyan/25 hover:border-cyan/40',
    tester: 'bg-amber/8 border-amber/25 hover:border-amber/40',
    verifier: 'bg-green/8 border-green/25 hover:border-green/40',
  }

  const roleText: Record<string, string> = {
    scout: 'text-blue', architect: 'text-purple',
    implementer: 'text-cyan', tester: 'text-amber',
    verifier: 'text-green',
  }

  const pipClass: Record<StepRow['status'], string> = {
    pending: 'pip-idle', running: 'pip-active', passed: 'pip-done', failed: 'pip-fail',
  }

  function thoughtsFor(stepId: string): Thought[] {
    return thoughts.filter(t => t.stepId === stepId)
  }

  function statusRing(status: StepRow['status']): string {
    switch (status) {
      case 'running': return 'ring-1 ring-cyan/30 node-active'
      case 'passed': return 'ring-1 ring-green/20'
      case 'failed': return 'ring-1 ring-red/30'
      default: return ''
    }
  }
</script>

<div class="flex items-center justify-center gap-3 px-6 overflow-x-auto h-full">
  {#if steps.length === 0}
    <div class="text-center opacity-40">
      <div class="text-text-3 text-sm">Flat execution</div>
      <div class="text-text-3 text-xs mt-1">No plan DAG</div>
    </div>
  {:else}
    {#each steps as step, i (step.id)}
      {#if i > 0}
        <!-- Fluid connector — animated flow between nodes -->
        <div class="w-10 h-[3px] shrink-0 rounded-full {steps[i-1].status === 'passed' ? 'bg-green/25' : step.status === 'running' ? 'connector-flow-h' : 'bg-border/50'}"></div>
      {/if}

      <!-- Node wrapper — relative so bubbles can be absolute -->
      <div class="relative shrink-0" style="width: 240px">
        <button
          onclick={() => toggle(step.id)}
          class="w-full border rounded-lg {roleBg[step.role.toLowerCase()] ?? 'bg-surface-2 border-border hover:border-border-2'} {statusRing(step.status)} p-4 text-left transition-all cursor-pointer {selectedId === step.id ? 'selected' : ''}"
        >
          <div class="flex items-center gap-2 mb-2">
            <div class="pip {pipClass[step.status]}"></div>
            <span class="text-[11px] font-mono {roleText[step.role.toLowerCase()] ?? 'text-text-3'} uppercase tracking-wider font-medium">
              {roleLabel[step.role.toLowerCase()] ?? step.role}
            </span>
            {#if step.verify_passed === true}
              <span class="text-[10px] font-mono text-green ml-auto">VERIFIED</span>
            {:else if step.verify_passed === false}
              <span class="text-[10px] font-mono text-red ml-auto">FAILED</span>
            {/if}
          </div>

          <div class="text-[14px] text-text-1 font-medium leading-snug">{step.name}</div>

          {#if step.duration_ms != null}
            <div class="flex items-center gap-3 mt-2.5 text-[12px] font-mono text-text-3">
              <span>{(step.duration_ms / 1000).toFixed(1)}s</span>
              {#if step.tokens_used != null}
                <span>{step.tokens_used.toLocaleString()} tok</span>
              {/if}
            </div>
          {/if}

          {#if expanded.has(step.id)}
            <div class="mt-3 pt-3 border-t border-border/50 space-y-1.5 text-[12px]">
              <div class="text-text-3">Mode: <span class="text-text-2">{step.execution_mode}</span></div>
              {#if step.verify_passed != null}
                <div class="{step.verify_passed ? 'text-green' : 'text-red'}">
                  {step.verify_passed ? 'All checks passed' : 'Checks failed — replan triggered'}
                </div>
              {/if}
            </div>
          {/if}
        </button>

        <!-- Thought bubbles — absolute, below the node, doesn't push layout -->
        {#if thoughtsFor(step.id).length > 0}
          <div class="absolute top-full left-0 w-full mt-2 flex flex-col gap-1 z-10 pointer-events-none">
            {#each thoughtsFor(step.id).slice(-2) as thought (thought.text)}
              <ThoughtBubble text={thought.text} variant={thought.variant} />
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  {/if}
</div>
