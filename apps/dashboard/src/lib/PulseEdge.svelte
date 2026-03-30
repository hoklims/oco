<script lang="ts">
  /**
   * PulseEdge — custom Svelte Flow edge for teammate communication.
   *
   * Each endpoint has its own color (assigned per-teammate).
   * - Forward particle (source→target) uses sourceColor
   * - Reverse particle (target→source) uses targetColor
   * - On teammate_message: directional flash in sender's color
   *   with "SenderName → message" label
   *
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
  let sourceColor = $derived((data?.sourceColor as string) ?? '#a78bfa')
  let targetColor = $derived((data?.targetColor as string) ?? '#22d3ee')
  let flashMessage = $derived((data?.flashMessage as string) ?? '')
  let flashSender = $derived((data?.flashSender as string) ?? '')
  let flashColor = $derived((data?.flashColor as string) ?? '#e8ecf4')
  let flashReverse = $derived((data?.flashReverse as boolean) ?? false)
  let showFlash = $derived(flashMessage.length > 0)

  // Unique IDs for SVG defs
  let pathId = $derived(`pulse-path-${id}`)

  // Blended glow color from both endpoints
  let glowColor = $derived(sourceColor)
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
  <!-- SVG defs for animateMotion paths -->
  <defs>
    <path id={pathId} d={edgePath} />
  </defs>

  <!-- Glow trail -->
  <path
    d={edgePath}
    fill="none"
    stroke={glowColor}
    stroke-width="8"
    opacity="0.06"
    style="filter: blur(4px); pointer-events: none;"
  />

  <!-- Forward particle (source → target) in source color -->
  <circle r="3.5" fill={sourceColor} opacity="0.75" style="pointer-events: none;">
    <animateMotion
      dur="2.8s"
      repeatCount="indefinite"
      keyPoints="0;1"
      keyTimes="0;1"
      calcMode="linear"
    >
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>

  <!-- Reverse particle (target → source) in target color, offset start -->
  <circle r="3" fill={targetColor} opacity="0.65" style="pointer-events: none;">
    <animateMotion
      dur="2.8s"
      repeatCount="indefinite"
      keyPoints="1;0"
      keyTimes="0;1"
      calcMode="linear"
      begin="1.4s"
    >
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>

  <!-- Small secondary particles for richer feel -->
  <circle r="2" fill={sourceColor} opacity="0.3" style="pointer-events: none;">
    <animateMotion
      dur="2.8s"
      repeatCount="indefinite"
      keyPoints="0;1"
      keyTimes="0;1"
      calcMode="linear"
      begin="0.9s"
    >
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>
  <circle r="2" fill={targetColor} opacity="0.25" style="pointer-events: none;">
    <animateMotion
      dur="2.8s"
      repeatCount="indefinite"
      keyPoints="1;0"
      keyTimes="0;1"
      calcMode="linear"
      begin="2.1s"
    >
      <mpath href="#{pathId}" />
    </animateMotion>
  </circle>
{/if}

<!-- Message flash — directional particle on teammate_message event -->
{#if showFlash}
  <defs>
    <path id="{pathId}-flash" d={edgePath} />
  </defs>

  <!-- Flash particle in sender's color, travels in correct direction -->
  <circle r="5" fill={flashColor} opacity="0.9" style="pointer-events: none; filter: blur(1px);">
    <animateMotion
      dur="1s"
      repeatCount="1"
      keyPoints={flashReverse ? "1;0" : "0;1"}
      keyTimes="0;1"
      calcMode="spline"
      keySplines="0.4 0 0.2 1"
      fill="freeze"
    >
      <mpath href="#{pathId}-flash" />
    </animateMotion>
    <animate attributeName="opacity" dur="1s" values="0.9;0.9;0" keyTimes="0;0.7;1" fill="freeze" />
  </circle>

  <!-- Sender trail glow -->
  <circle r="8" fill={flashColor} opacity="0.15" style="pointer-events: none; filter: blur(3px);">
    <animateMotion
      dur="1s"
      repeatCount="1"
      keyPoints={flashReverse ? "1;0" : "0;1"}
      keyTimes="0;1"
      calcMode="spline"
      keySplines="0.4 0 0.2 1"
      fill="freeze"
    >
      <mpath href="#{pathId}-flash" />
    </animateMotion>
    <animate attributeName="opacity" dur="1s" values="0.15;0.15;0" keyTimes="0;0.7;1" fill="freeze" />
  </circle>

  <!-- Message label with sender name + direction -->
  <g style="pointer-events: none;">
    <foreignObject x={labelX - 80} y={labelY - 34} width="160" height="30">
      <div class="msg-label" style="border-color: {flashColor}30; background: linear-gradient(90deg, {flashColor}15, transparent);">
        <span class="msg-sender" style="color: {flashColor}">{flashSender}</span>
        <span class="msg-arrow">:</span>
        <span class="msg-text">{flashMessage}</span>
      </div>
    </foreignObject>
  </g>
{/if}

<style>
  .msg-label {
    font-family: ui-monospace, monospace;
    font-size: 9px;
    color: #e8ecf4;
    border: 1px solid;
    border-radius: 4px;
    padding: 3px 8px;
    display: flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
    overflow: hidden;
    animation: msgFade 2.5s forwards;
  }
  .msg-sender {
    font-weight: 700;
    flex-shrink: 0;
    letter-spacing: 0.03em;
  }
  .msg-arrow {
    color: #5c6378;
    flex-shrink: 0;
  }
  .msg-text {
    color: #9aa0b4;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes msgFade {
    0% { opacity: 0; transform: translateY(4px); }
    10% { opacity: 1; transform: translateY(0); }
    75% { opacity: 1; }
    100% { opacity: 0; }
  }
</style>
