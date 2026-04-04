<script lang="ts">
  import type { RunScorecard, ScorecardDimension } from './types'

  let { scorecard }: { scorecard: RunScorecard } = $props()

  const LABELS: Record<ScorecardDimension, { short: string; color: string }> = {
    Success:              { short: 'Success',    color: '#34d399' },
    TrustVerdict:         { short: 'Trust',      color: '#4b8df8' },
    VerificationCoverage: { short: 'Coverage',   color: '#a78bfa' },
    MissionContinuity:    { short: 'Continuity', color: '#22d3ee' },
    CostEfficiency:       { short: 'Cost',       color: '#fbbf24' },
    ReplanStability:      { short: 'Stability',  color: '#f87171' },
    ErrorRate:            { short: 'Errors',     color: '#f472b6' },
  }

  const DIMENSIONS: ScorecardDimension[] = [
    'Success', 'TrustVerdict', 'VerificationCoverage', 'MissionContinuity',
    'CostEfficiency', 'ReplanStability', 'ErrorRate',
  ]

  const CX = 140
  const CY = 130
  const R = 90
  const RINGS = [0.25, 0.5, 0.75, 1.0]

  function polarToXY(angle: number, radius: number): [number, number] {
    const rad = (angle - 90) * (Math.PI / 180)
    return [CX + radius * Math.cos(rad), CY + radius * Math.sin(rad)]
  }

  function angleFor(i: number): number {
    return (360 / DIMENSIONS.length) * i
  }

  let scores = $derived(
    DIMENSIONS.map(d => {
      const dim = scorecard.dimensions.find(s => s.dimension === d)
      return dim?.score ?? 0
    })
  )

  let polygonPoints = $derived(
    scores.map((s, i) => {
      const [x, y] = polarToXY(angleFor(i), R * s)
      return `${x},${y}`
    }).join(' ')
  )

  let overallColor = $derived(
    scorecard.overall_score >= 0.8 ? '#34d399' :
    scorecard.overall_score >= 0.6 ? '#fbbf24' :
    scorecard.overall_score >= 0.4 ? '#f97316' : '#f87171'
  )
</script>

<div class="flex flex-col items-center gap-3">
  <!-- Radar SVG -->
  <svg viewBox="0 0 280 270" class="w-full max-w-[320px]">
    <!-- Grid rings -->
    {#each RINGS as ring}
      <polygon
        points={DIMENSIONS.map((_, i) => {
          const [x, y] = polarToXY(angleFor(i), R * ring)
          return `${x},${y}`
        }).join(' ')}
        fill="none"
        stroke="var(--color-border)"
        stroke-width={ring === 1 ? 1 : 0.5}
        opacity={ring === 1 ? 0.6 : 0.3}
      />
    {/each}

    <!-- Axis lines -->
    {#each DIMENSIONS as _, i}
      {@const [x, y] = polarToXY(angleFor(i), R)}
      <line x1={CX} y1={CY} x2={x} y2={y}
        stroke="var(--color-border)" stroke-width="0.5" opacity="0.25" />
    {/each}

    <!-- Data polygon -->
    <polygon
      points={polygonPoints}
      fill={overallColor}
      fill-opacity="0.12"
      stroke={overallColor}
      stroke-width="1.5"
    />

    <!-- Data points -->
    {#each DIMENSIONS as d, i}
      {@const [x, y] = polarToXY(angleFor(i), R * scores[i])}
      <circle cx={x} cy={y} r="3"
        fill={LABELS[d].color}
        stroke="var(--color-bg)"
        stroke-width="1.5"
      />
    {/each}

    <!-- Labels -->
    {#each DIMENSIONS as d, i}
      {@const [x, y] = polarToXY(angleFor(i), R + 22)}
      <text
        x={x} y={y}
        text-anchor="middle"
        dominant-baseline="central"
        class="text-[9px] font-mono"
        fill={LABELS[d].color}
        opacity="0.9"
      >
        {LABELS[d].short}
      </text>
    {/each}

    <!-- Center score -->
    <text x={CX} y={CY - 6} text-anchor="middle" class="text-xl font-mono font-bold" fill={overallColor}>
      {(scorecard.overall_score * 100).toFixed(0)}
    </text>
    <text x={CX} y={CY + 10} text-anchor="middle" class="text-[9px] font-mono" fill="var(--color-text-3)">
      overall
    </text>
  </svg>

  <!-- Dimension breakdown -->
  <div class="w-full space-y-1.5 px-1">
    {#each DIMENSIONS as d, i}
      {@const score = scores[i]}
      {@const detail = scorecard.dimensions.find(s => s.dimension === d)?.detail ?? ''}
      <div class="flex items-center gap-2 text-xs">
        <span class="w-16 font-mono truncate" style="color: {LABELS[d].color}">{LABELS[d].short}</span>
        <div class="flex-1 h-1.5 rounded-full bg-surface-3 overflow-hidden">
          <div class="h-full rounded-full transition-all duration-700" style="width: {score * 100}%; background: {LABELS[d].color}; opacity: 0.7"></div>
        </div>
        <span class="w-8 text-right font-mono text-text-3">{(score * 100).toFixed(0)}</span>
      </div>
    {/each}
  </div>

  <!-- Cost metrics -->
  {#if scorecard.cost}
    <div class="w-full grid grid-cols-3 gap-2 pt-2 border-t border-border">
      <div class="text-center">
        <div class="text-sm font-mono text-text-1">{scorecard.cost.tokens.toLocaleString()}</div>
        <div class="text-[10px] text-text-3 uppercase">tokens</div>
      </div>
      <div class="text-center">
        <div class="text-sm font-mono text-text-1">{scorecard.cost.steps}</div>
        <div class="text-[10px] text-text-3 uppercase">steps</div>
      </div>
      <div class="text-center">
        <div class="text-sm font-mono text-text-1">{(scorecard.cost.duration_ms / 1000).toFixed(1)}s</div>
        <div class="text-[10px] text-text-3 uppercase">duration</div>
      </div>
    </div>
  {/if}
</div>
