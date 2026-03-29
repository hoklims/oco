<script lang="ts">
  import type { BudgetSnapshot } from './types'

  let { budget }: { budget: BudgetSnapshot | null } = $props()

  function pct(used: number, remaining: number): number {
    const total = used + remaining
    return total > 0 ? Math.round((used / total) * 100) : 0
  }

  function barClass(p: number): string {
    if (p >= 90) return 'bg-error'
    if (p >= 75) return 'bg-warning'
    return 'bg-accent'
  }

  let tokenPct = $derived(budget ? pct(budget.tokens_used, budget.tokens_remaining) : 0)
  let toolPct = $derived(budget ? pct(budget.tool_calls_used, budget.tool_calls_remaining) : 0)
</script>

{#if budget}
  <div class="space-y-2">
    <!-- Tokens -->
    <div>
      <div class="flex justify-between text-[10px] font-mono mb-0.5">
        <span class="text-text-dim uppercase tracking-wider">Tokens</span>
        <span class="text-text">{budget.tokens_used.toLocaleString()} / {(budget.tokens_used + budget.tokens_remaining).toLocaleString()}</span>
      </div>
      <div class="h-1.5 bg-surface overflow-hidden">
        <div class="h-full transition-all duration-500 {barClass(tokenPct)}" style="width: {tokenPct}%"></div>
      </div>
    </div>

    <!-- Tool calls -->
    <div>
      <div class="flex justify-between text-[10px] font-mono mb-0.5">
        <span class="text-text-dim uppercase tracking-wider">Actions</span>
        <span class="text-text">{budget.tool_calls_used} / {budget.tool_calls_used + budget.tool_calls_remaining}</span>
      </div>
      <div class="h-1.5 bg-surface overflow-hidden">
        <div class="h-full transition-all duration-500 {barClass(toolPct)}" style="width: {toolPct}%"></div>
      </div>
    </div>

    <!-- Counters row -->
    <div class="flex gap-4 pt-1">
      <div class="text-[10px] font-mono">
        <span class="text-text-dim uppercase tracking-wider">Recon</span>
        <span class="text-text ml-1.5">{budget.retrievals_used}</span>
      </div>
      <div class="text-[10px] font-mono">
        <span class="text-text-dim uppercase tracking-wider">Verify</span>
        <span class="text-text ml-1.5">{budget.verify_cycles_used}</span>
      </div>
      <div class="text-[10px] font-mono">
        <span class="text-text-dim uppercase tracking-wider">Elapsed</span>
        <span class="text-text ml-1.5">{budget.elapsed_secs}s</span>
      </div>
    </div>
  </div>
{:else}
  <div class="text-[10px] font-mono text-text-dim uppercase tracking-wider">Awaiting data</div>
{/if}
