<script lang="ts">
  import type { GateResult, GateVerdict as GateVerdictType, DimensionGateCheck } from './types'

  let { result }: { result: GateResult } = $props()

  let expanded = $state(false)

  const VERDICT_STYLE: Record<GateVerdictType, { bg: string; text: string; border: string; icon: string }> = {
    Pass: { bg: 'bg-green-dim', text: 'text-green', border: 'border-green/20', icon: '✓' },
    Warn: { bg: 'bg-amber-dim', text: 'text-amber', border: 'border-amber/20', icon: '!' },
    Fail: { bg: 'bg-red-dim', text: 'text-red', border: 'border-red/20', icon: '✗' },
  }

  let style = $derived(VERDICT_STYLE[result.verdict])
  let failedCount = $derived(result.dimension_checks.filter(c => c.verdict === 'Fail').length)
  let warnCount = $derived(result.dimension_checks.filter(c => c.verdict === 'Warn').length)
  let passCount = $derived(result.dimension_checks.filter(c => c.verdict === 'Pass').length)

  function deltaLabel(d: number): string {
    const sign = d > 0 ? '+' : ''
    return `${sign}${(d * 100).toFixed(1)}%`
  }

  function checkStyle(c: DimensionGateCheck) {
    return VERDICT_STYLE[c.verdict]
  }
</script>

<div class="space-y-3">
  <!-- Main verdict badge -->
  <button
    onclick={() => expanded = !expanded}
    class="w-full flex items-center gap-3 px-4 py-3 rounded-lg {style.bg} border {style.border} transition-all hover:brightness-110 cursor-pointer"
  >
    <span class="text-lg {style.text} font-mono font-bold">{style.icon}</span>
    <div class="flex-1 text-left">
      <div class="text-sm font-medium {style.text}">Gate: {result.verdict}</div>
      <div class="text-xs text-text-3 font-mono mt-0.5">
        {result.policy.strategy} policy · {passCount}P {warnCount}W {failedCount}F
      </div>
    </div>
    <div class="text-right">
      <div class="text-lg font-mono {style.text}">{(result.candidate_overall * 100).toFixed(0)}</div>
      <div class="text-[10px] font-mono {result.overall_delta >= 0 ? 'text-green' : 'text-red'}">
        {deltaLabel(result.overall_delta)} vs baseline
      </div>
    </div>
    <span class="text-text-3 text-xs transition-transform {expanded ? 'rotate-180' : ''}">&darr;</span>
  </button>

  <!-- Expanded dimension checks -->
  {#if expanded}
    <div class="space-y-1.5 pl-2">
      {#each result.dimension_checks as check}
        {@const cs = checkStyle(check)}
        <div class="flex items-center gap-2 px-3 py-2 rounded {cs.bg} border {cs.border}">
          <span class="text-[10px] font-mono {cs.text} w-4">{cs.icon}</span>
          <span class="text-xs font-mono {cs.text} w-24 truncate">{check.dimension}</span>
          <div class="flex-1 flex items-center gap-1.5">
            <div class="h-1 flex-1 rounded-full bg-surface-3 overflow-hidden">
              <div class="h-full rounded-full" style="width: {check.candidate_score * 100}%; background: {cs.text === 'text-green' ? '#34d399' : cs.text === 'text-amber' ? '#fbbf24' : '#f87171'}; opacity: 0.6"></div>
            </div>
            <span class="text-[10px] font-mono text-text-3 w-8 text-right">{(check.candidate_score * 100).toFixed(0)}</span>
          </div>
          <span class="text-[10px] font-mono {check.delta >= 0 ? 'text-green' : 'text-red'} w-12 text-right">
            {deltaLabel(check.delta)}
          </span>
        </div>
      {/each}
    </div>

    <!-- Reasons -->
    {#if result.reasons.length > 0}
      <div class="pl-2 space-y-1">
        {#each result.reasons as reason}
          <div class="text-xs text-text-3 pl-3 border-l-2 border-border py-0.5">{reason}</div>
        {/each}
      </div>
    {/if}
  {/if}
</div>
