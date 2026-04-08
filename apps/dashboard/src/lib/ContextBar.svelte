<script lang="ts">
  /**
   * ContextBar — real-time visualisation of the token budget.
   *
   * Fed by `BudgetSnapshot` events (or the `budget` field inside
   * `Progress`). Shows a horizontal gauge with:
   *   - green zone (0-60%)
   *   - amber warning (60-85%)
   *   - red pressure (85-100%)
   * A soft pulse animates the cursor once utilization crosses 80%.
   */

  import type { BudgetSnapshot } from './types'

  let { budget }: { budget: BudgetSnapshot | null } = $props()

  let used = $derived(budget?.tokens_used ?? 0)
  let remaining = $derived(budget?.tokens_remaining ?? 0)
  let total = $derived(used + remaining)
  let pct = $derived(total > 0 ? Math.min(100, (used / total) * 100) : 0)

  let zone = $derived(pct >= 85 ? 'danger' : pct >= 60 ? 'warn' : 'ok')
  let fillColor = $derived(
    zone === 'danger' ? '#f87171' :
    zone === 'warn' ? '#fbbf24' :
    '#34d399',
  )

  function fmt(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`
    return String(n)
  }
</script>

<div class="ctx">
  <div class="ctx-header">
    <span class="ctx-label">CONTEXT</span>
    <span class="ctx-numbers">
      <span style="color:{fillColor}">{fmt(used)}</span>
      <span class="sep">/</span>
      <span class="total">{fmt(total)}</span>
      <span class="pct" style="color:{fillColor}">{pct.toFixed(0)}%</span>
    </span>
  </div>
  <div class="ctx-track">
    <div class="ctx-fill {zone === 'danger' ? 'pulse' : ''}"
      style="width: {pct}%; background: linear-gradient(90deg, {fillColor}88, {fillColor});">
    </div>
    <div class="ctx-marker" style="left: 80%"></div>
  </div>
</div>

<style>
  .ctx {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 10px;
    background: rgba(13, 15, 20, 0.4);
    border-radius: 6px;
    border: 1px solid #1c203040;
    font-family: ui-monospace, monospace;
  }

  .ctx-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 9px;
  }
  .ctx-label {
    color: #8890a4;
    letter-spacing: 0.15em;
    text-transform: uppercase;
    font-weight: 600;
  }
  .ctx-numbers {
    display: inline-flex;
    align-items: baseline;
    gap: 4px;
    font-weight: 600;
  }
  .sep { color: #2a3546; }
  .total { color: #5c6378; }
  .pct { margin-left: 4px; font-size: 10px; }

  .ctx-track {
    position: relative;
    height: 4px;
    background: #1c2030;
    border-radius: 2px;
    overflow: hidden;
  }
  .ctx-fill {
    position: absolute;
    inset: 0 auto 0 0;
    border-radius: 2px;
    transition: width 0.4s ease, background 0.4s ease;
  }
  .ctx-fill.pulse {
    animation: ctx-pulse 1.2s ease-in-out infinite;
  }
  @keyframes ctx-pulse {
    0%, 100% { filter: brightness(1); }
    50%      { filter: brightness(1.25); }
  }
  .ctx-marker {
    position: absolute;
    top: -2px;
    bottom: -2px;
    width: 1px;
    background: #5c637860;
    pointer-events: none;
  }
</style>
