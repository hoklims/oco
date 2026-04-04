<script lang="ts">
  import type { MergeReadiness, GateVerdict, TrustVerdict, BaselineFreshness, BaselineFreshnessCheck } from './types'

  let { mergeReadiness, gateVerdict = null, trustVerdict = null, freshness = null }: {
    mergeReadiness: MergeReadiness
    gateVerdict?: GateVerdict | null
    trustVerdict?: TrustVerdict | null
    freshness?: BaselineFreshnessCheck | null
  } = $props()

  const READINESS: Record<MergeReadiness, { label: string; color: string; bg: string; icon: string }> = {
    Ready:              { label: 'Merge Ready',     color: 'text-green', bg: 'bg-green-dim', icon: '✓' },
    ConditionallyReady: { label: 'Conditional',     color: 'text-amber', bg: 'bg-amber-dim', icon: '~' },
    NotReady:           { label: 'Not Ready',       color: 'text-red',   bg: 'bg-red-dim',   icon: '✗' },
    Unknown:            { label: 'Unknown',         color: 'text-text-3', bg: 'bg-surface-3', icon: '?' },
  }

  const TRUST: Record<TrustVerdict, { color: string; label: string }> = {
    High:   { color: 'text-green', label: 'High Trust' },
    Medium: { color: 'text-amber', label: 'Med Trust' },
    Low:    { color: 'text-red',   label: 'Low Trust' },
    None:   { color: 'text-text-3', label: 'No Trust' },
  }

  const FRESHNESS: Record<BaselineFreshness, { color: string; dot: string }> = {
    Fresh:   { color: 'text-green', dot: 'bg-green' },
    Aging:   { color: 'text-amber', dot: 'bg-amber' },
    Stale:   { color: 'text-red',   dot: 'bg-red' },
    Unknown: { color: 'text-text-3', dot: 'bg-text-3' },
  }

  let readinessStyle = $derived(READINESS[mergeReadiness])
</script>

<div class="flex items-center gap-2 flex-wrap">
  <!-- Main readiness pill -->
  <div class="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full {readinessStyle.bg} border border-current/10">
    <span class="text-[10px] font-mono font-bold {readinessStyle.color}">{readinessStyle.icon}</span>
    <span class="text-xs font-mono font-medium {readinessStyle.color}">{readinessStyle.label}</span>
  </div>

  <!-- Gate verdict pill -->
  {#if gateVerdict}
    {@const gStyle = gateVerdict === 'Pass' ? 'text-green bg-green-dim' : gateVerdict === 'Warn' ? 'text-amber bg-amber-dim' : 'text-red bg-red-dim'}
    <div class="inline-flex items-center gap-1 px-2 py-1 rounded-full {gStyle} text-[10px] font-mono">
      Gate: {gateVerdict}
    </div>
  {/if}

  <!-- Trust verdict pill -->
  {#if trustVerdict}
    {@const tStyle = TRUST[trustVerdict]}
    <div class="inline-flex items-center gap-1 px-2 py-1 rounded-full bg-surface-3 text-[10px] font-mono {tStyle.color}">
      {tStyle.label}
    </div>
  {/if}

  <!-- Baseline freshness -->
  {#if freshness}
    {@const fStyle = FRESHNESS[freshness.freshness]}
    <div class="inline-flex items-center gap-1.5 px-2 py-1 rounded-full bg-surface-3 text-[10px] font-mono {fStyle.color}">
      <span class="w-1.5 h-1.5 rounded-full {fStyle.dot}"></span>
      {freshness.freshness}
      {#if freshness.age_days != null}
        <span class="text-text-3">({freshness.age_days.toFixed(0)}d)</span>
      {/if}
    </div>
  {/if}
</div>
