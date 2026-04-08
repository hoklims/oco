<script lang="ts">
  /**
   * IndexingScene — workspace scan visualization.
   *
   * Shown between classification and plan exploration. Represents OCO
   * reading files, extracting symbols, and building the retrieval index.
   *
   * Driven by `index_progress` events with `files_done` and
   * `symbols_so_far`. The component animates the counters smoothly and
   * streams a synthetic file list that simulates the scanner output.
   *
   * All CSS-native, GPU-accelerated (opacity + transform only).
   */

  let { filesDone = 0, symbolsSoFar = 0 }: {
    filesDone?: number
    symbolsSoFar?: number
  } = $props()

  // ── Animated counters (ease toward target values) ──────────
  let displayedFiles = $state(0)
  let displayedSymbols = $state(0)

  $effect(() => {
    // Smooth catch-up animation — clamp step so fast updates don't jitter
    const targetF = filesDone
    const targetS = symbolsSoFar
    const raf = requestAnimationFrame(function tick() {
      let changed = false
      if (displayedFiles < targetF) {
        displayedFiles = Math.min(targetF, displayedFiles + Math.max(1, Math.ceil((targetF - displayedFiles) / 6)))
        changed = true
      }
      if (displayedSymbols < targetS) {
        displayedSymbols = Math.min(targetS, displayedSymbols + Math.max(1, Math.ceil((targetS - displayedSymbols) / 6)))
        changed = true
      }
      if (changed) requestAnimationFrame(tick)
    })
    return () => cancelAnimationFrame(raf)
  })

  // ── Synthetic file stream (for the terminal column) ────────
  const FILE_POOL = [
    'src/lib/types.ts',
    'src/routes/+layout.svelte',
    'crates/shared-types/src/lib.rs',
    'crates/orchestrator-core/src/loop.rs',
    'apps/dashboard/src/App.svelte',
    'src/lib/PlanMap.svelte',
    'src/lib/event-player.ts',
    'src/lib/DagNode.svelte',
    'crates/planner/src/lib.rs',
    'crates/mcp-server/src/dashboard.rs',
    'src/lib/sse.ts',
    'src/lib/playground-data.ts',
    'crates/retrieval/src/index.rs',
    'src/lib/Timeline.svelte',
    'crates/policy-engine/src/classifier.rs',
    'src/lib/ClassifyingScene.svelte',
    'README.md',
    'crates/code-intel/src/parser.rs',
    'src/lib/demo.ts',
    'Cargo.toml',
    'package.json',
    'crates/context-engine/src/assembler.rs',
    'src/lib/DetailPanel.svelte',
    'crates/verifier/src/runner.rs',
  ]

  type FileEntry = { id: number; path: string }
  let visibleFiles = $state<FileEntry[]>([])
  let nextFileId = 0
  let totalPushed = 0
  const MAX_VISIBLE = 6

  $effect(() => {
    // One new file per increment of filesDone. Use a stable ID so Svelte
    // doesn't unmount/remount existing rows when we append a new one
    // (which would retrigger their entry animation every push).
    if (totalPushed >= displayedFiles) return
    while (totalPushed < displayedFiles) {
      const path = FILE_POOL[totalPushed % FILE_POOL.length]
      visibleFiles = [...visibleFiles, { id: nextFileId++, path }].slice(-MAX_VISIBLE)
      totalPushed += 1
    }
  })
</script>

<div class="scene">
  <div class="grid-dots"></div>

  <div class="stack">
    <div class="label">SCANNING WORKSPACE</div>

    <div class="counters">
      <div class="counter">
        <div class="counter-icon">▸</div>
        <div class="counter-value">{displayedFiles}</div>
        <div class="counter-label">files</div>
      </div>
      <div class="counter-sep">·</div>
      <div class="counter">
        <div class="counter-icon">◈</div>
        <div class="counter-value sym">{displayedSymbols.toLocaleString()}</div>
        <div class="counter-label">symbols</div>
      </div>
    </div>

    <div class="terminal">
      {#each visibleFiles as file (file.id)}
        <div class="line">
          <span class="prompt">›</span>
          <span class="path">{file.path}</span>
          <span class="tick">✓</span>
        </div>
      {/each}
    </div>

    <div class="status">
      <span class="status-pip"></span>
      Building symbol index…
    </div>
  </div>
</div>

<style>
  .scene {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: radial-gradient(ellipse at center, rgba(34, 211, 238, 0.04), transparent 60%);
    overflow: hidden;
  }

  .grid-dots {
    position: absolute;
    inset: 0;
    background-image: radial-gradient(rgba(34, 211, 238, 0.08) 1px, transparent 1px);
    background-size: 24px 24px;
    opacity: 0.4;
    pointer-events: none;
  }

  .stack {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 20px;
    z-index: 1;
    min-width: 420px;
  }

  .label {
    font-size: 11px;
    font-family: ui-monospace, monospace;
    letter-spacing: 0.18em;
    color: #22d3ee;
    text-transform: uppercase;
    animation: pulse-label 2s ease-in-out infinite;
  }
  @keyframes pulse-label {
    0%, 100% { opacity: 0.7; }
    50%      { opacity: 1; }
  }

  .counters {
    display: flex;
    align-items: baseline;
    gap: 24px;
    font-family: ui-monospace, monospace;
  }

  .counter {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
  }

  .counter-icon {
    font-size: 10px;
    color: #22d3ee80;
    line-height: 1;
  }

  .counter-value {
    font-size: 42px;
    font-weight: 700;
    color: #e8ecf4;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    min-width: 80px;
    text-align: center;
  }
  .counter-value.sym { color: #a78bfa; }

  .counter-label {
    font-size: 10px;
    color: #5c6378;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .counter-sep {
    color: #2a3546;
    font-size: 32px;
    line-height: 1;
  }

  .terminal {
    width: 100%;
    background: rgba(13, 15, 20, 0.75);
    border: 1px solid #1c203050;
    border-radius: 6px;
    padding: 10px 14px;
    font-family: ui-monospace, monospace;
    font-size: 11px;
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-height: 144px;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    animation: line-in 0.35s ease-out both;
  }
  @keyframes line-in {
    from { opacity: 0; transform: translateX(-6px); }
    to   { opacity: 1; transform: translateX(0); }
  }

  .prompt { color: #22d3ee80; }
  .path { color: #8890a4; flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tick { color: #34d39980; font-size: 10px; }

  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    font-family: ui-monospace, monospace;
    font-size: 10px;
    color: #5c6378;
    letter-spacing: 0.08em;
  }

  .status-pip {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: #22d3ee;
    box-shadow: 0 0 6px rgba(34, 211, 238, 0.6);
    animation: pip-pulse 1.2s ease-in-out infinite;
  }
  @keyframes pip-pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50%      { opacity: 0.4; transform: scale(0.7); }
  }
</style>
