<script lang="ts">
  import type { BudgetSnapshot } from './types'

  let { budget }: { budget: BudgetSnapshot | null } = $props()

  function pct(used: number, remaining: number): number {
    const total = used + remaining
    return total > 0 ? Math.round((used / total) * 100) : 0
  }

  function barColor(p: number): string {
    if (p >= 90) return 'bg-error'
    if (p >= 75) return 'bg-warning'
    return 'bg-accent'
  }

  let tokenPct = $derived(budget ? pct(budget.tokens_used, budget.tokens_remaining) : 0)
  let toolPct = $derived(budget ? pct(budget.tool_calls_used, budget.tool_calls_remaining) : 0)
</script>

{#if budget}
  <div class="grid grid-cols-2 gap-3">
    <div class="rounded-lg bg-surface-3 p-3">
      <div class="flex justify-between text-xs text-text-dim mb-1">
        <span>Tokens</span>
        <span>{budget.tokens_used.toLocaleString()} / {(budget.tokens_used + budget.tokens_remaining).toLocaleString()}</span>
      </div>
      <div class="h-2 rounded-full bg-surface overflow-hidden">
        <div class="h-full rounded-full transition-all duration-300 {barColor(tokenPct)}" style="width: {tokenPct}%"></div>
      </div>
    </div>

    <div class="rounded-lg bg-surface-3 p-3">
      <div class="flex justify-between text-xs text-text-dim mb-1">
        <span>Tool calls</span>
        <span>{budget.tool_calls_used} / {budget.tool_calls_used + budget.tool_calls_remaining}</span>
      </div>
      <div class="h-2 rounded-full bg-surface overflow-hidden">
        <div class="h-full rounded-full transition-all duration-300 {barColor(toolPct)}" style="width: {toolPct}%"></div>
      </div>
    </div>

    <div class="rounded-lg bg-surface-3 p-3">
      <div class="flex justify-between text-xs text-text-dim mb-1">
        <span>Retrievals</span>
        <span>{budget.retrievals_used}</span>
      </div>
    </div>

    <div class="rounded-lg bg-surface-3 p-3">
      <div class="flex justify-between text-xs text-text-dim mb-1">
        <span>Verify cycles</span>
        <span>{budget.verify_cycles_used}</span>
      </div>
    </div>
  </div>
{:else}
  <div class="text-text-dim text-sm">No budget data yet</div>
{/if}
