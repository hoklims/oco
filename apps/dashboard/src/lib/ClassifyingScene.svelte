<script lang="ts">
  /**
   * ClassifyingScene — premium "analyzing" visualization.
   *
   * Inspired by Vercel/Linear: abstract, confident, no fake code.
   * Three phases cross-fade (never hard-cut):
   *   1. Scan:     Rotating conic gradient + subtle pulse — "system is active"
   *   2. Analyze:  Gradient contracts inward, ring forms — "converging on answer"
   *   3. Reveal:   Complexity badge scales in with glow — "result delivered"
   *
   * All CSS-native. GPU-accelerated (transform + opacity only).
   */
  import { onMount } from 'svelte'

  let { mission = '', complexity = '' }: { mission?: string; complexity?: string } = $props()

  let progress = $state(0)  // 0 = scan, 1 = analyze, 2 = reveal, 3 = fade-out

  let badgeText = $derived(
    complexity?.match(/critical/i) ? 'CRITICAL' :
    complexity?.match(/high/i) ? 'HIGH' :
    complexity?.match(/medium/i) ? 'MEDIUM' :
    complexity?.match(/low/i) ? 'LOW' :
    complexity?.match(/trivial/i) ? 'TRIVIAL' :
    'HIGH'
  )

  let badgeColor = $derived(
    badgeText === 'CRITICAL' ? '#f87171' :
    badgeText === 'HIGH' ? '#fbbf24' :
    badgeText === 'MEDIUM' ? '#22d3ee' :
    badgeText === 'LOW' ? '#34d399' :
    '#34d399'
  )

  let statusText = $derived(
    progress === 0 ? 'Scanning workspace...' :
    progress === 1 ? 'Analyzing structure...' :
    progress >= 2 ? `Complexity: ${badgeText.toLowerCase()}` : ''
  )

  onMount(() => {
    const t1 = setTimeout(() => { progress = 1 }, 2400)
    const t2 = setTimeout(() => { progress = 2 }, 4400)
    const t3 = setTimeout(() => { progress = 3 }, 6200)
    return () => { clearTimeout(t1); clearTimeout(t2); clearTimeout(t3) }
  })
</script>

<div class="scene">
  <!-- Ambient rotating conic gradient — always present, fades with progress -->
  <div class="gradient-layer" class:gradient-dim={progress >= 2}></div>

  <!-- Center content -->
  <div class="center-stack">
    <!-- Phase 0-1: Radial pulse — just light breathing, no shapes -->
    {#if progress < 2}
      <div class="pulse-light" class:pulse-focus={progress === 1}></div>
    {/if}

    <!-- Phase 2-3: Badge reveal -->
    {#if progress >= 2}
      <div class="badge" class:badge-exit={progress === 3} style="--badge-color: {badgeColor};">
        <span class="badge-text">{badgeText}</span>
        <div class="badge-glow"></div>
      </div>
    {/if}

    <!-- Status text — cross-fades between phases -->
    <div class="status" class:status-reveal={progress >= 2}>
      <p class="status-main">{statusText}</p>
      {#if progress < 2 && mission}
        <p class="status-sub">{mission}</p>
      {:else if progress >= 2}
        <p class="status-sub">Routing to plan engine</p>
      {/if}
    </div>
  </div>

  <!-- Subtle grid dots — very faint, gives depth -->
  <div class="grid-dots"></div>
</div>

<style>
  .scene {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--color-bg, #08090c);
  }

  /* ── Ambient gradient ────────────────────────── */
  .gradient-layer {
    position: absolute;
    inset: 0;
    opacity: 0.35;
    transition: opacity 1.2s cubic-bezier(0.4, 0, 0.2, 1);
    background: conic-gradient(
      from 0deg at 50% 50%,
      #22d3ee10 0deg,
      #a78bfa15 90deg,
      #4b8df810 180deg,
      #34d39912 270deg,
      #22d3ee10 360deg
    );
    animation: gradientSpin 8s linear infinite;
    filter: blur(60px);
  }
  .gradient-dim {
    opacity: 0.08;
  }
  @keyframes gradientSpin {
    to { transform: rotate(360deg); }
  }

  /* ── Grid dots (depth) ───────────────────────── */
  .grid-dots {
    position: absolute;
    inset: 0;
    opacity: 0.03;
    background-image: radial-gradient(circle, #ffffff 0.5px, transparent 0.5px);
    background-size: 32px 32px;
  }

  /* ── Center stack ────────────────────────────── */
  .center-stack {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 24px;
    z-index: 2;
  }

  /* ── Radial pulse — pure light, no geometry ── */
  .pulse-light {
    width: 120px;
    height: 120px;
    border-radius: 50%;
    background: radial-gradient(circle, #22d3ee12 0%, transparent 70%);
    animation: lightBreath 3s ease-in-out infinite;
    transition: all 1.2s cubic-bezier(0.4, 0, 0.2, 1);
  }
  .pulse-focus {
    width: 80px;
    height: 80px;
    background: radial-gradient(circle, #22d3ee20 0%, transparent 70%);
    animation: lightBreath 2s ease-in-out infinite;
  }
  @keyframes lightBreath {
    0%, 100% { opacity: 0.4; transform: scale(1); }
    50% { opacity: 1; transform: scale(1.15); }
  }

  /* ── Badge ───────────────────────────────────── */
  .badge {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 120px;
    height: 120px;
    border-radius: 20px;
    border: 1.5px solid var(--badge-color);
    background: color-mix(in srgb, var(--badge-color) 6%, transparent);
    animation: badgeIn 0.6s cubic-bezier(0.34, 1.4, 0.64, 1) forwards;
    opacity: 0;
  }
  .badge-exit {
    animation: badgeOut 0.8s cubic-bezier(0.4, 0, 0.2, 1) forwards;
    opacity: 1;
  }

  .badge-text {
    font-family: ui-monospace, 'Cascadia Code', monospace;
    font-size: 22px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--badge-color);
  }

  .badge-glow {
    position: absolute;
    inset: -1px;
    border-radius: 20px;
    box-shadow: 0 0 30px color-mix(in srgb, var(--badge-color) 25%, transparent),
                0 0 60px color-mix(in srgb, var(--badge-color) 10%, transparent);
    animation: glowBreath 2.5s ease-in-out infinite;
  }

  @keyframes badgeIn {
    0% { opacity: 0; transform: scale(0.7); filter: blur(8px); }
    60% { opacity: 1; filter: blur(0); }
    80% { transform: scale(1.03); }
    100% { opacity: 1; transform: scale(1); }
  }
  @keyframes badgeOut {
    0% { opacity: 1; transform: scale(1); }
    100% { opacity: 0; transform: scale(0.95); filter: blur(4px); }
  }
  @keyframes glowBreath {
    0%, 100% { opacity: 0.5; }
    50% { opacity: 1; }
  }

  /* ── Status text ─────────────────────────────── */
  .status {
    text-align: center;
    transition: all 0.6s cubic-bezier(0.4, 0, 0.2, 1);
  }
  .status-main {
    font-family: ui-monospace, 'Cascadia Code', monospace;
    font-size: 13px;
    letter-spacing: 0.05em;
    color: #9aa0b4;
    transition: color 0.6s;
  }
  .status-reveal .status-main {
    color: var(--badge-color, #9aa0b4);
  }
  .status-sub {
    font-size: 12px;
    color: #5c6378;
    margin-top: 4px;
    max-width: 320px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Reduced motion ──────────────────────────── */
  @media (prefers-reduced-motion: reduce) {
    .gradient-layer { animation: none; }
    .orbital-ring { animation: none; opacity: 0.6; }
    .orbital-dot { animation: none; }
    .badge { animation: none; opacity: 1; }
    .badge-glow { animation: none; opacity: 0.7; }
  }
</style>
