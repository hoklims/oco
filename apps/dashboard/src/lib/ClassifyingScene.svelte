<script lang="ts">
  /**
   * ClassifyingScene — progressive dimensional analysis.
   *
   * Single continuous build-up: each element appears and STAYS visible.
   * Nothing disappears until the final fade-out.
   *
   *   0.0s  Mission text starts revealing word-by-word
   *   ~2.0s All words visible, keywords highlighted — hold
   *   3.0s  Dimension cards stagger in (300ms each)
   *   4.5s  Bars start filling (1.4s transition)
   *   ~6.0s Bars + scores fully visible — hold
   *   7.5s  Badge scales in below dimensions
   *   8.2s  Routing label fades in
   *   8.2s  Hold — everything visible together for ~3s
   *  11.0s  Entire scene fades out
   *  11.8s  Gone
   *
   * Total: ~12s. Everything on screen together for the last ~3.5s.
   * Timing: professional subtitle pacing (150 wpm, +0.5s cognitive latency).
   * All CSS-native. GPU-accelerated (transform + opacity only).
   */
  import { onMount } from 'svelte'

  let { mission = '', complexity = '' }: { mission?: string; complexity?: string } = $props()

  // ── Progressive reveal flags (only go true, never back) ────
  let wordsReady = $state(false)
  let dimsVisible = $state(0)  // 0–4: how many cards are visible
  let barsActive = $state(false)
  let badgeVisible = $state(false)
  let routingVisible = $state(false)
  let exiting = $state(false)

  // ── Word reveal ────────────────────────────────────────────
  let words = $derived(mission ? mission.split(/\s+/) : [])
  let revealedWords = $state(0)
  const KEYWORDS = /\b(refactor|implement|add|fix|update|migrate|create|build|delete|remove|auth|api|jwt|token|module|schema|database|endpoint|config|deploy|test|service|route|handler|middleware)\b/i

  // ── Derived from complexity prop ───────────────────────────
  let level = $derived(
    complexity?.match(/critical/i) ? 'CRITICAL' :
    complexity?.match(/high/i) ? 'HIGH' :
    complexity?.match(/medium/i) ? 'MEDIUM' :
    complexity?.match(/low/i) ? 'LOW' :
    complexity?.match(/trivial/i) ? 'TRIVIAL' :
    'MEDIUM'
  )

  let levelColor = $derived(
    level === 'CRITICAL' ? '#f87171' :
    level === 'HIGH' ? '#fbbf24' :
    level === 'MEDIUM' ? '#22d3ee' :
    level === 'LOW' ? '#34d399' :
    '#34d399'
  )

  let routing = $derived(
    level === 'TRIVIAL' || level === 'LOW'
      ? 'Direct execution'
      : 'Plan engine (competitive)'
  )

  // ── Dimension scores ──────────────────────────────────────
  interface Dimension {
    key: string
    label: string
    icon: string
    score: number
    color: string
  }

  function scoreDimensions(cx: string, lvl: string): Dimension[] {
    const base = lvl === 'CRITICAL' ? 88 : lvl === 'HIGH' ? 72 : lvl === 'MEDIUM' ? 52 : lvl === 'LOW' ? 30 : 15
    const hasParallel = /parallel/i.test(cx)
    const stepMatch = cx.match(/(\d+)\s*steps?/i)
    const stepCount = stepMatch ? parseInt(stepMatch[1]) : 3

    const scope = Math.min(95, base + stepCount * 4)
    const depth = Math.min(90, base + (stepCount > 5 ? 15 : 0))
    const risk = lvl === 'CRITICAL' ? 90 : lvl === 'HIGH' ? 65 : lvl === 'MEDIUM' ? 40 : 18
    const parallel = hasParallel ? Math.min(85, 40 + stepCount * 6) : Math.max(10, base - 20)

    const colorFor = (v: number) =>
      v >= 75 ? '#fbbf24' : v >= 50 ? '#22d3ee' : v >= 30 ? '#34d399' : '#5c6378'

    return [
      { key: 'scope', label: 'Scope', icon: '◫', score: scope, color: colorFor(scope) },
      { key: 'depth', label: 'Depth', icon: '◈', score: depth, color: colorFor(depth) },
      { key: 'risk', label: 'Risk', icon: '◆', score: risk, color: colorFor(risk) },
      { key: 'parallel', label: 'Parallelism', icon: '⫘', score: parallel, color: colorFor(parallel) },
    ]
  }

  let dimensions = $derived(scoreDimensions(complexity, level))

  // ── Status text — evolves progressively ────────────────────
  let statusText = $derived(
    badgeVisible ? `Classified: ${level.toLowerCase()} · ${routing}` :
    barsActive ? 'Evaluating complexity...' :
    dimsVisible > 0 ? 'Scanning dimensions...' :
    'Parsing request...'
  )

  // ── Lifecycle — single progressive timeline ────────────────
  onMount(() => {
    const timers: ReturnType<typeof setTimeout>[] = []

    // 0s: Start word reveal (~180ms per word)
    const wordCount = words.length || 1
    const wordDelay = Math.min(180, 1800 / wordCount)
    for (let i = 0; i < words.length; i++) {
      timers.push(setTimeout(() => { revealedWords = i + 1 }, 200 + i * wordDelay))
    }

    // 3.0s: Dimension cards stagger in (300ms each)
    for (let i = 0; i < 4; i++) {
      timers.push(setTimeout(() => { dimsVisible = i + 1 }, 3000 + i * 300))
    }

    // 4.5s: Bars start filling
    timers.push(setTimeout(() => { barsActive = true }, 4500))

    // 7.5s: Badge appears
    timers.push(setTimeout(() => { badgeVisible = true }, 7500))

    // 8.2s: Routing label
    timers.push(setTimeout(() => { routingVisible = true }, 8200))

    // 11.0s: Exit
    timers.push(setTimeout(() => { exiting = true }, 11000))

    return () => timers.forEach(clearTimeout)
  })
</script>

<div class="scene" class:scene-exit={exiting}>
  <!-- Ambient -->
  <div class="ambient" class:ambient-focus={badgeVisible}></div>
  <div class="grid-dots"></div>

  <!-- Single progressive stack — everything accumulates -->
  <div class="stack">

    <!-- Mission text — always present once revealed -->
    <div class="mission" class:mission-settled={dimsVisible > 0}>
      <div class="mission-label">REQUEST</div>
      <p class="mission-text">
        {#each words as word, i}
          <span
            class="word"
            class:word-visible={i < revealedWords}
            class:word-key={i < revealedWords && KEYWORDS.test(word)}
            style="transition-delay: {i * 15}ms"
          >{word}{' '}</span>
        {/each}
        <span class="cursor" class:cursor-done={revealedWords >= words.length}></span>
      </p>
    </div>

    <!-- Dimension cards — appear below mission, stay -->
    {#if dimsVisible > 0}
      <div class="dims">
        {#each dimensions as dim, i (dim.key)}
          <div
            class="dim"
            class:dim-visible={i < dimsVisible}
            style="transition-delay: {i * 60}ms"
          >
            <div class="dim-top">
              <span class="dim-icon" style="color: {dim.color}">{dim.icon}</span>
              <span class="dim-name">{dim.label}</span>
              {#if barsActive}
                <span class="dim-score" style="color: {dim.color}">{dim.score}</span>
              {/if}
            </div>
            <div class="dim-track">
              <div
                class="dim-fill"
                style="
                  width: {barsActive ? dim.score : 0}%;
                  background: {dim.color};
                  transition-delay: {i * 200 + 100}ms;
                "
              ></div>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <!-- Badge + routing — appears below dimensions, stays -->
    {#if badgeVisible}
      <div class="result">
        <div class="badge" style="--c: {levelColor}">
          <span class="badge-label">{level}</span>
          <div class="badge-glow"></div>
        </div>
        {#if routingVisible}
          <div class="route">
            <span class="route-arrow" style="color: {levelColor}">→</span>
            <span class="route-text">{routing}</span>
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <!-- Status line -->
  <div class="status">
    <span class="status-pip {badgeVisible ? 'pip-done' : 'pip-active'}"></span>
    <span class="status-label">{statusText}</span>
  </div>
</div>

<style>
  /* ── Scene ──────────────────────────────────── */
  .scene {
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--color-bg, #08090c);
    transition: opacity 0.8s cubic-bezier(0.4, 0, 0.2, 1),
                transform 0.8s cubic-bezier(0.4, 0, 0.2, 1);
  }
  .scene-exit {
    opacity: 0;
    transform: scale(0.99);
  }

  .ambient {
    position: absolute;
    inset: 0;
    opacity: 0.2;
    background: radial-gradient(ellipse 50% 35% at 50% 40%, #22d3ee06 0%, transparent 70%);
    transition: opacity 1.5s;
  }
  .ambient-focus { opacity: 0.35; }

  .grid-dots {
    position: absolute;
    inset: 0;
    opacity: 0.025;
    background-image: radial-gradient(circle, #fff 0.5px, transparent 0.5px);
    background-size: 32px 32px;
  }

  /* ── Progressive stack ──────────────────────── */
  .stack {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 28px;
    z-index: 2;
    padding: 40px;
  }

  /* ── Mission text ───────────────────────────── */
  .mission {
    text-align: center;
    max-width: 520px;
    transition: transform 0.8s cubic-bezier(0.4, 0, 0.2, 1),
                margin-bottom 0.8s cubic-bezier(0.4, 0, 0.2, 1);
  }
  .mission-settled {
    transform: translateY(-8px);
  }
  .mission-label {
    font-family: ui-monospace, monospace;
    font-size: 10px;
    letter-spacing: 0.2em;
    color: #5c6378;
    margin-bottom: 14px;
  }
  .mission-text {
    font-size: 17px;
    line-height: 1.7;
    color: #e8ecf4;
    font-weight: 500;
    margin: 0;
  }

  .word {
    display: inline;
    opacity: 0;
    filter: blur(3px);
    transition: opacity 0.35s ease, filter 0.35s ease, color 0.5s ease;
  }
  .word-visible {
    opacity: 1;
    filter: blur(0);
  }
  .word-key {
    color: #22d3ee;
    text-decoration: underline;
    text-decoration-color: #22d3ee30;
    text-underline-offset: 3px;
  }

  .cursor {
    display: inline-block;
    width: 2px;
    height: 17px;
    background: #22d3ee;
    border-radius: 1px;
    margin-left: 2px;
    vertical-align: text-bottom;
    animation: blink 0.9s step-end infinite;
    transition: opacity 0.6s;
  }
  .cursor-done {
    animation-iteration-count: 4;
    opacity: 0;
    transition: opacity 0.8s 2s;
  }
  @keyframes blink {
    0%, 100% { opacity: 1; }
    50% { opacity: 0; }
  }

  /* ── Dimension cards ────────────────────────── */
  .dims {
    display: flex;
    gap: 14px;
  }
  .dim {
    width: 136px;
    padding: 14px 16px;
    background: #12151c;
    border: 1px solid #1c2030;
    border-radius: 11px;
    opacity: 0;
    transform: translateY(10px) scale(0.95);
    transition: opacity 0.5s cubic-bezier(0.4, 0, 0.2, 1),
                transform 0.5s cubic-bezier(0.4, 0, 0.2, 1);
  }
  .dim-visible {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
  .dim-top {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 10px;
  }
  .dim-icon {
    font-size: 13px;
    line-height: 1;
  }
  .dim-name {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    color: #9aa0b4;
    letter-spacing: 0.04em;
    flex: 1;
  }
  .dim-score {
    font-family: ui-monospace, monospace;
    font-size: 14px;
    font-weight: 700;
    line-height: 1;
    animation: popIn 0.4s cubic-bezier(0.34, 1.4, 0.64, 1) forwards;
  }
  @keyframes popIn {
    0% { opacity: 0; transform: scale(0.5); }
    100% { opacity: 1; transform: scale(1); }
  }
  .dim-track {
    height: 4px;
    background: #1c2030;
    border-radius: 2px;
    overflow: hidden;
  }
  .dim-fill {
    height: 100%;
    border-radius: 2px;
    width: 0%;
    transition: width 1.4s cubic-bezier(0.4, 0, 0.2, 1);
  }

  /* ── Badge + routing ────────────────────────── */
  .result {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    animation: slideUp 0.6s cubic-bezier(0.4, 0, 0.2, 1) forwards;
  }
  @keyframes slideUp {
    0% { opacity: 0; transform: translateY(12px) scale(0.94); }
    100% { opacity: 1; transform: translateY(0) scale(1); }
  }

  .badge {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px 36px;
    border-radius: 14px;
    border: 1.5px solid var(--c);
    background: color-mix(in srgb, var(--c) 5%, transparent);
    animation: badgePop 0.5s cubic-bezier(0.34, 1.25, 0.64, 1) forwards;
  }
  @keyframes badgePop {
    0% { opacity: 0; transform: scale(0.75); filter: blur(6px); }
    50% { opacity: 1; filter: blur(0); }
    75% { transform: scale(1.03); }
    100% { transform: scale(1); }
  }
  .badge-label {
    font-family: ui-monospace, 'Cascadia Code', monospace;
    font-size: 22px;
    font-weight: 700;
    letter-spacing: 0.14em;
    color: var(--c);
  }
  .badge-glow {
    position: absolute;
    inset: -1px;
    border-radius: 14px;
    box-shadow: 0 0 24px color-mix(in srgb, var(--c) 18%, transparent),
                0 0 48px color-mix(in srgb, var(--c) 6%, transparent);
    animation: pulse 2.5s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 1; }
  }

  .route {
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: ui-monospace, monospace;
    font-size: 12px;
    color: #9aa0b4;
    animation: fadeIn 0.5s forwards;
  }
  .route-arrow { font-size: 16px; }

  /* ── Status bar ─────────────────────────────── */
  .status {
    position: absolute;
    bottom: 28px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 10px;
    z-index: 3;
  }
  .status-label {
    font-family: ui-monospace, monospace;
    font-size: 11px;
    letter-spacing: 0.06em;
    color: #5c6378;
    text-transform: uppercase;
    transition: color 0.5s;
  }
  .pip-active {
    width: 6px; height: 6px; border-radius: 2px; flex-shrink: 0;
    background: #22d3ee;
    box-shadow: 0 0 6px rgba(34, 211, 238, 0.5);
    animation: breathe 2s ease-in-out infinite;
  }
  .pip-done {
    width: 6px; height: 6px; border-radius: 2px; flex-shrink: 0;
    background: #34d399;
    box-shadow: 0 0 4px rgba(52, 211, 153, 0.3);
  }
  @keyframes breathe {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.4; transform: scale(0.65); }
  }

  @keyframes fadeIn {
    0% { opacity: 0; }
    100% { opacity: 1; }
  }

  /* ── Reduced motion ─────────────────────────── */
  @media (prefers-reduced-motion: reduce) {
    .word { filter: none; transition: opacity 0.1s; }
    .cursor { animation: none; opacity: 1; }
    .dim-fill { transition: width 0.3s; }
    .badge { animation: none; opacity: 1; }
    .badge-glow { animation: none; opacity: 0.6; }
    .result { animation: none; opacity: 1; }
    .pip-active { animation: none; }
  }
</style>
