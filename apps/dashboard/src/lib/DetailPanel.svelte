<script lang="ts">
  import type { DashboardEvent, BudgetSnapshot, StepRow } from './types'
  import type { Thought } from './demo'

  let { event, budget, selectedStep, thoughts = [] }: {
    event: DashboardEvent | null
    budget: BudgetSnapshot | null
    selectedStep?: StepRow | null
    thoughts?: Thought[]
  } = $props()

  // Drill depth tracking
  let drillDepth = $state(0)

  // Reset depth when selection changes
  $effect(() => {
    if (event || selectedStep) drillDepth = 0
  })

  function stepThoughts(stepId: string): Thought[] {
    return thoughts.filter(t => t.stepId === stepId)
  }

  function pct(used: number, remaining: number): number {
    const total = used + remaining
    return total > 0 ? Math.round((used / total) * 100) : 0
  }

  function barColor(p: number): string {
    if (p >= 90) return 'bg-red'
    if (p >= 70) return 'bg-amber'
    return 'bg-blue'
  }
</script>

<div class="flex flex-col h-full">
  <div class="px-4 py-2 border-b border-border bg-surface shrink-0">
    <span class="text-xs font-mono text-text-3 uppercase tracking-widest">
      {event ? `Event #${event.seq}` : 'Resources'}
    </span>
  </div>

  <div class="flex-1 overflow-y-auto p-4">
    {#if event}
      {@const kind = event.kind as Record<string, unknown>}
      {@const type = kind.type as string}

      <div class="text-lg text-text-1 font-medium mb-1">{type.replace(/_/g, ' ')}</div>
      <div class="text-xs font-mono text-text-3 mb-6">{new Date(event.ts).toLocaleTimeString()}</div>

      {#if type === 'flat_step_completed' || type === 'step_completed'}
        <div class="space-y-4">
          {#if kind.step_name}
            <div>
              <div class="text-xs text-text-3 uppercase tracking-wider mb-1">Step</div>
              <div class="text-sm text-text-1">{kind.step_name}</div>
            </div>
          {/if}
          {#if kind.action_type}
            <div>
              <div class="text-xs text-text-3 uppercase tracking-wider mb-1">Action</div>
              <div class="text-sm text-text-1">{kind.action_type}</div>
            </div>
          {/if}
          {#if kind.reason}
            <div>
              <div class="text-xs text-text-3 uppercase tracking-wider mb-1">Reasoning</div>
              <div class="text-sm text-text-2 leading-relaxed">{kind.reason}</div>
            </div>
          {/if}
          <div class="flex gap-6 pt-2">
            {#if kind.duration_ms != null}
              <div>
                <div class="text-xs text-text-3 mb-0.5">Duration</div>
                <div class="text-xl font-mono text-text-1">{((kind.duration_ms as number) / 1000).toFixed(1)}s</div>
              </div>
            {/if}
            {#if kind.tokens_used != null}
              <div>
                <div class="text-xs text-text-3 mb-0.5">Tokens</div>
                <div class="text-xl font-mono text-text-1">{(kind.tokens_used as number).toLocaleString()}</div>
              </div>
            {/if}
          </div>
        </div>

      {:else if type === 'plan_generated'}
        <div class="space-y-3">
          <div class="flex gap-6">
            <div>
              <div class="text-xs text-text-3 mb-0.5">Steps</div>
              <div class="text-2xl font-mono text-text-1">{kind.step_count}</div>
            </div>
            <div>
              <div class="text-xs text-text-3 mb-0.5">Parallel</div>
              <div class="text-2xl font-mono text-text-1">{kind.parallel_group_count}</div>
            </div>
            <div>
              <div class="text-xs text-text-3 mb-0.5">Depth</div>
              <div class="text-2xl font-mono text-text-1">{kind.critical_path_length}</div>
            </div>
          </div>
          <div>
            <div class="text-xs text-text-3 uppercase tracking-wider mb-1">Strategy</div>
            <div class="text-sm text-blue font-medium">{kind.strategy}</div>
          </div>
          <div>
            <div class="text-xs text-text-3 mb-0.5">Estimated cost</div>
            <div class="text-lg font-mono text-text-1">{(kind.estimated_total_tokens as number).toLocaleString()} tokens</div>
          </div>
        </div>

      {:else if type === 'verify_gate_result'}
        <div class="space-y-3">
          <div class="text-sm text-text-1 mb-2">{kind.step_name}</div>
          {#each (kind.checks as Array<Record<string, unknown>>) as check}
            <div class="flex items-center gap-3 py-2 px-3 rounded {check.passed ? 'bg-green-dim' : 'bg-red-dim'}">
              <div class="pip {check.passed ? 'pip-done' : 'pip-fail'}"></div>
              <span class="text-sm font-mono uppercase {check.passed ? 'text-green' : 'text-red'}">{check.check_type}</span>
              <span class="text-xs text-text-2 ml-auto">{check.summary}</span>
            </div>
          {/each}
          {#if kind.replan_triggered}
            <div class="mt-2 py-2 px-3 rounded bg-amber-dim border border-amber/20">
              <span class="text-sm text-amber font-medium">Replan triggered</span>
            </div>
          {/if}
        </div>

      {:else if type === 'replan_triggered'}
        <div class="space-y-3">
          <div class="text-red text-sm mb-2">Failed: {kind.failed_step_name}</div>
          <div class="text-sm font-mono text-amber">Attempt {kind.attempt} / {kind.max_attempts}</div>
          <div class="flex gap-4 pt-2">
            <div class="text-center px-4 py-3 rounded bg-green-dim">
              <div class="text-xl font-mono text-green">+{kind.steps_added}</div>
              <div class="text-xs text-text-3">added</div>
            </div>
            <div class="text-center px-4 py-3 rounded bg-red-dim">
              <div class="text-xl font-mono text-red">-{kind.steps_removed}</div>
              <div class="text-xs text-text-3">removed</div>
            </div>
            <div class="text-center px-4 py-3 rounded bg-surface-3">
              <div class="text-xl font-mono text-text-2">{kind.steps_preserved}</div>
              <div class="text-xs text-text-3">kept</div>
            </div>
          </div>
        </div>

      {:else}
        <pre class="text-xs font-mono text-text-3 whitespace-pre-wrap break-all leading-relaxed">{JSON.stringify(kind, null, 2)}</pre>
      {/if}

    {:else if selectedStep}
      <!-- Step detail view (from plan click) -->
      <div class="space-y-4">
        <div>
          <div class="text-lg text-text-1 font-medium">{selectedStep.name}</div>
          <div class="text-xs font-mono text-text-3 uppercase tracking-wider mt-1">{selectedStep.role} / {selectedStep.execution_mode}</div>
        </div>

        <div class="flex gap-6">
          {#if selectedStep.duration_ms != null}
            <div>
              <div class="text-xs text-text-3 mb-0.5">Duration</div>
              <div class="text-2xl font-mono text-text-1">{(selectedStep.duration_ms / 1000).toFixed(1)}s</div>
            </div>
          {/if}
          {#if selectedStep.tokens_used != null}
            <div>
              <div class="text-xs text-text-3 mb-0.5">Tokens</div>
              <div class="text-2xl font-mono text-text-1">{selectedStep.tokens_used.toLocaleString()}</div>
            </div>
          {/if}
        </div>

        {#if selectedStep.verify_passed != null}
          <div class="py-2 px-3 rounded {selectedStep.verify_passed ? 'bg-green-dim border border-green/20' : 'bg-red-dim border border-red/20'}">
            <span class="text-sm font-medium {selectedStep.verify_passed ? 'text-green' : 'text-red'}">
              {selectedStep.verify_passed ? 'All verification checks passed' : 'Verification failed'}
            </span>
          </div>
        {/if}

        <!-- Agent thoughts for this step -->
        {#if stepThoughts(selectedStep.id).length > 0}
          {@const stepT = stepThoughts(selectedStep.id)}
          <div>
            <div class="text-xs text-text-3 uppercase tracking-wider mb-2">Agent thoughts</div>
            <div class="space-y-1.5">
              {#each stepT as t}
                <div class="text-xs pl-3 border-l-2 py-1 {t.variant === 'warning' ? 'border-amber/40 text-amber' : t.variant === 'success' ? 'border-green/40 text-green' : t.variant === 'action' ? 'border-cyan/40 text-cyan' : 'border-border-2 text-text-2'}">
                  {t.text}
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </div>

    {:else}
      <!-- Resources overview — default view -->
      {#if budget}
        {@const tokenPct = pct(budget.tokens_used, budget.tokens_remaining)}
        {@const toolPct = pct(budget.tool_calls_used, budget.tool_calls_remaining)}
        <div class="space-y-5">
          <div>
            <div class="flex justify-between text-xs mb-1.5">
              <span class="text-text-3 uppercase tracking-wider">Tokens</span>
              <span class="font-mono text-text-2">{budget.tokens_used.toLocaleString()} / {(budget.tokens_used + budget.tokens_remaining).toLocaleString()}</span>
            </div>
            <div class="rail h-2"><div class="rail-fill {barColor(tokenPct)}" style="width: {tokenPct}%"></div></div>
          </div>
          <div>
            <div class="flex justify-between text-xs mb-1.5">
              <span class="text-text-3 uppercase tracking-wider">Actions</span>
              <span class="font-mono text-text-2">{budget.tool_calls_used} / {budget.tool_calls_used + budget.tool_calls_remaining}</span>
            </div>
            <div class="rail h-2"><div class="rail-fill {barColor(toolPct)}" style="width: {toolPct}%"></div></div>
          </div>
          <div class="grid grid-cols-3 gap-4 pt-3">
            <div class="text-center">
              <div class="text-2xl font-mono text-text-1">{budget.retrievals_used}</div>
              <div class="text-xs text-text-3 uppercase tracking-wider mt-0.5">Recon</div>
            </div>
            <div class="text-center">
              <div class="text-2xl font-mono text-text-1">{budget.verify_cycles_used}</div>
              <div class="text-xs text-text-3 uppercase tracking-wider mt-0.5">Checks</div>
            </div>
            <div class="text-center">
              <div class="text-2xl font-mono text-text-1">{budget.elapsed_secs}s</div>
              <div class="text-xs text-text-3 uppercase tracking-wider mt-0.5">Elapsed</div>
            </div>
          </div>
        </div>
      {:else}
        <div class="text-sm text-text-3 opacity-50">Awaiting data</div>
      {/if}
    {/if}
  </div>
</div>
