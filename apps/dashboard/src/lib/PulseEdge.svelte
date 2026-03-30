<script lang="ts">
  /**
   * PulseEdge — custom Svelte Flow edge for teammate communication.
   *
   * When active (both endpoints running teammates):
   *   - Base path glows purple
   *   - Two particles travel in opposite directions (SVG animateMotion)
   *   - On teammate_message: bright flash particle
   *
   * When inactive: standard muted edge.
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

  // Compute bezier path
  let pathResult = $derived(getBezierPath({
    sourceX, sourceY, targetX, targetY,
    sourcePosition, targetPosition,
  }))
  let edgePath = $derived(pathResult[0])
  let labelX = $derived(pathResult[1])
  let labelY = $derived(pathResult[2])

  // Edge state from data
  let isActive = $derived((data?.isActive as boolean) ?? false)
  let flashMessage = $derived((data?.flashMessage as string) ?? '')
  let showFlash = $derived(flashMessage.length > 0)

  // Unique IDs for SVG defs
  let pathId = $derived(`pulse-path-${id}`)
  let pathRevId = $derived(`pulse-rev-${id}`)
</script>

<!-- Base edge — always rendered -->
<BaseEdge
  {id}
  path={edgePath}
  style={isActive
    ? 'stroke: #a78bfa40; stroke-width: 2; transition: all 0.6s;'
    : (style as string) ?? 'stroke: #1c203060; stroke-width: 1; transition: all 0.6s;'}
  {markerEnd}
/>

{#if isActive}
  <!-- SVG defs for animateMotion paths -->
  <defs>
    <path id={pathId} d={edgePath} />
  </defs>

  <!-- Glow trail -->
  <path
    d={edgePath}
    fill="none"
    stroke="#a78bfa"
    stroke-width="8"
    opacity="0.06"
    style="filter: blur(4px); pointer-events: none;"
  />

  <!-- Forward particle (purple) -->
  <circle r="3.5" fill="#a78bfa" opacity="0.7" style="pointer-events: none;">
    <animateMotion
      dur="3s"
      repeatCount="indefinite"
      keyPoints="0;1"
      keyTimes="0;1"
      calcMode="linear"
    >
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>

  <!-- Reverse particle (cyan, offset start) -->
  <circle r="3" fill="#22d3ee" opacity="0.45" style="pointer-events: none;">
    <animateMotion
      dur="3s"
      repeatCount="indefinite"
      keyPoints="1;0"
      keyTimes="0;1"
      calcMode="linear"
      begin="1.5s"
    >
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>
{/if}

<!-- Message flash — bright particle on teammate_message event -->
{#if showFlash}
  <defs>
    <path id="{pathId}-flash" d={edgePath} />
  </defs>

  <!-- Flash particle (bright, larger) -->
  <circle r="5" fill="#e8ecf4" opacity="0.9" style="pointer-events: none; filter: blur(1px);">
    <animateMotion
      dur="1.2s"
      repeatCount="1"
      keyPoints="0;1"
      keyTimes="0;1"
      calcMode="spline"
      keySplines="0.4 0 0.2 1"
      fill="freeze"
    >
      <mpath href="#{pathId}-flash" />
    </animateMotion>
    <animate attributeName="opacity" dur="1.2s" values="0.9;0.9;0" keyTimes="0;0.7;1" fill="freeze" />
  </circle>

  <!-- Message label above midpoint (offset up to avoid node overlap) -->
  <g style="pointer-events: none;">
    <foreignObject x={labelX - 60} y={labelY - 32} width="120" height="28">
      <div class="msg-label">
        {flashMessage}
      </div>
    </foreignObject>
  </g>
{/if}

<style>
  .msg-label {
    font-family: ui-monospace, monospace;
    font-size: 9px;
    color: #e8ecf4;
    background: rgba(167, 139, 250, 0.2);
    border: 1px solid #a78bfa30;
    border-radius: 4px;
    padding: 2px 8px;
    text-align: center;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    animation: msgFade 2s forwards;
  }

  @keyframes msgFade {
    0% { opacity: 0; transform: translateY(4px); }
    15% { opacity: 1; transform: translateY(0); }
    70% { opacity: 1; }
    100% { opacity: 0; }
  }
</style>
