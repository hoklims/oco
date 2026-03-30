<script lang="ts">
  /**
   * PulseEdge — custom Svelte Flow edge for teammate communication.
   *
   * Each endpoint has its own color (assigned per-teammate).
   * - Forward particle (source→target) uses sourceColor
   * - Reverse particle (target→source) uses targetColor
   * - On teammate_message: bright directional flash in sender's color
   *
   * Message CONTENT is displayed in the TeamGroup feed, not on the edge.
   * Hardware-accelerated via SVG animations (no JS per-frame).
   */
  import { BaseEdge, getBezierPath, type EdgeProps } from '@xyflow/svelte'

  let {
    id,
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
    data,
    style,
    markerEnd,
  }: EdgeProps = $props()

  let pathResult = $derived(getBezierPath({
    sourceX, sourceY, targetX, targetY,
    sourcePosition, targetPosition,
  }))
  let edgePath = $derived(pathResult[0])

  let isActive = $derived((data?.isActive as boolean) ?? false)
  let sourceColor = $derived((data?.sourceColor as string) ?? '#a78bfa')
  let targetColor = $derived((data?.targetColor as string) ?? '#22d3ee')
  let flashColor = $derived((data?.flashColor as string) ?? '#e8ecf4')
  let flashReverse = $derived((data?.flashReverse as boolean) ?? false)
  let hasFlash = $derived(((data?.flashMessage as string) ?? '').length > 0)

  let pathId = $derived(`pulse-path-${id}`)
</script>

<!-- Base edge — always rendered -->
<BaseEdge
  {id}
  path={edgePath}
  style={isActive
    ? `stroke: ${sourceColor}30; stroke-width: 2; transition: all 0.6s;`
    : (style as string) ?? 'stroke: #1c203060; stroke-width: 1; transition: all 0.6s;'}
  {markerEnd}
/>

{#if isActive}
  <defs>
    <path id={pathId} d={edgePath} />
  </defs>

  <!-- Subtle glow trail -->
  <path
    d={edgePath}
    fill="none"
    stroke={sourceColor}
    stroke-width="6"
    opacity="0.05"
    style="filter: blur(3px); pointer-events: none;"
  />

  <!-- Forward particle (source → target) -->
  <circle r="3.5" fill={sourceColor} opacity="0.75" style="pointer-events: none;">
    <animateMotion dur="2.8s" repeatCount="indefinite" keyPoints="0;1" keyTimes="0;1" calcMode="linear">
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>

  <!-- Reverse particle (target → source) -->
  <circle r="3" fill={targetColor} opacity="0.65" style="pointer-events: none;">
    <animateMotion dur="2.8s" repeatCount="indefinite" keyPoints="1;0" keyTimes="0;1" calcMode="linear" begin="1.4s">
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>

  <!-- Secondary particles for depth -->
  <circle r="2" fill={sourceColor} opacity="0.3" style="pointer-events: none;">
    <animateMotion dur="2.8s" repeatCount="indefinite" keyPoints="0;1" keyTimes="0;1" calcMode="linear" begin="0.9s">
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>
  <circle r="2" fill={targetColor} opacity="0.25" style="pointer-events: none;">
    <animateMotion dur="2.8s" repeatCount="indefinite" keyPoints="1;0" keyTimes="0;1" calcMode="linear" begin="2.1s">
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>
{/if}

<!-- Flash burst on teammate_message — no label, just visual pulse -->
{#if hasFlash}
  <defs>
    <path id="{pathId}-flash" d={edgePath} />
  </defs>

  <!-- Bright flash particle in sender color -->
  <circle r="5" fill={flashColor} opacity="0.9" style="pointer-events: none;">
    <animateMotion
      dur="0.8s" repeatCount="1"
      keyPoints={flashReverse ? "1;0" : "0;1"} keyTimes="0;1"
      calcMode="spline" keySplines="0.4 0 0.2 1" fill="freeze"
    >
      <mpath href="#{pathId}-flash" />
    </animateMotion>
    <animate attributeName="opacity" dur="0.8s" values="0.9;0.9;0" keyTimes="0;0.6;1" fill="freeze" />
  </circle>

  <!-- Glow halo around flash -->
  <circle r="10" fill={flashColor} opacity="0.12" style="pointer-events: none; filter: blur(4px);">
    <animateMotion
      dur="0.8s" repeatCount="1"
      keyPoints={flashReverse ? "1;0" : "0;1"} keyTimes="0;1"
      calcMode="spline" keySplines="0.4 0 0.2 1" fill="freeze"
    >
      <mpath href="#{pathId}-flash" />
    </animateMotion>
    <animate attributeName="opacity" dur="0.8s" values="0.12;0.12;0" keyTimes="0;0.6;1" fill="freeze" />
  </circle>
{/if}
