<script lang="ts">
  import { onMount } from 'svelte'
  import {
    SvelteFlow,
    Background,
    BackgroundVariant,
    type Node,
    type Edge,
    Position,
  } from '@xyflow/svelte'
  import '@xyflow/svelte/dist/style.css'
  import dagre from 'dagre'
  import { DEMO_PLAN_SPEED, DEMO_PLAN_SAFETY } from './demo'

  let { phase }: {
    phase: 'idle' | 'generating' | 'comparing' | 'scoring' | 'selecting' | 'done'
  } = $props()

  const speed = DEMO_PLAN_SPEED
  const safety = DEMO_PLAN_SAFETY
  const rc: Record<string, string> = {
    scout: '#4b8df8', architect: '#a78bfa',
    implementer: '#22d3ee', tester: '#fbbf24', verifier: '#34d399',
  }

  // --- Animation states ---
  const INV = 'opacity:0;transition:all 0.7s cubic-bezier(0.4,0,0.2,1);transform:scale(0.8) translateY(8px);'
  const VIS = 'opacity:1;transform:scale(1) translateY(0);'
  const DIM = 'opacity:0.04;transform:scale(0.92) translateY(0);'

  // Both branches look IDENTICAL during generating/comparing — no winner hint.
  // Neutral style: muted border, subtle bg, same for both branches.
  const neutralNode = (c: string, extra = '') =>
    `background:#0d0f1480;border:1.5px solid ${c}35;color:#8890a4;border-radius:10px;padding:10px 16px;font-size:13px;min-width:140px;text-align:center;transition:all 0.8s cubic-bezier(0.4,0,0.2,1);${extra}`

  // Winner style: revealed during selecting phase — bright, glowing.
  const winnerNode = (c: string, extra = '') =>
    `background:${c}12;border:1.5px solid ${c};color:#e8ecf4;border-radius:10px;padding:10px 16px;font-size:13px;min-width:140px;text-align:center;box-shadow:0 0 20px ${c}15;transition:all 0.8s cubic-bezier(0.4,0,0.2,1);${extra}`

  function buildGraph() {
    const nodes: Node[] = []
    const edges: Edge[] = []

    // Origin
    nodes.push({
      id: 'origin', position: { x: 0, y: 0 }, data: { label: 'Task' }, type: 'input',
      style: INV + 'background:#1c2030;border:2px solid #4b8df8;color:#e8ecf4;border-radius:10px;padding:12px 20px;font-size:15px;font-family:ui-monospace,monospace;text-align:center;',
      sourcePosition: Position.Right,
    })

    // Speed branch — NEUTRAL look (no winner hint)
    speed.steps.forEach((step, i) => {
      const c = rc[step.role] ?? '#5c6378'
      nodes.push({
        id: `sp-${i}`, position: { x: 0, y: 0 }, data: { label: step.name },
        style: INV + neutralNode(c),
        sourcePosition: Position.Right, targetPosition: Position.Left,
      })
      edges.push({
        id: `esp-${i}`,
        source: i === 0 ? 'origin' : `sp-${i-1}`, target: `sp-${i}`,
        animated: false,
        style: `opacity:0;transition:all 0.7s cubic-bezier(0.4,0,0.2,1);stroke:${c}30;stroke-width:1.5;`,
      })
    })

    // Score summary nodes (hidden until scoring)
    const spTok = speed.steps.reduce((a, s) => a + s.tokens, 0)
    const saTok = safety.steps.reduce((a, s) => a + s.tokens, 0)
    const spV = speed.steps.filter(s => s.verify).length
    const saV = safety.steps.filter(s => s.verify).length

    nodes.push({
      id: 'score-sp', position: { x: 0, y: 0 },
      data: { label: `${speed.steps.length} steps · ${spV} verify · ${(spTok/1000).toFixed(0)}k tok` },
      style: INV + `background:#0d0f14;border:1px dashed #5c637840;color:#5c6378;border-radius:8px;padding:6px 12px;font-size:10px;font-family:ui-monospace,monospace;text-align:center;transition:all 0.7s;`,
      sourcePosition: Position.Right, targetPosition: Position.Left,
    })
    nodes.push({
      id: 'score-sa', position: { x: 0, y: 0 },
      data: { label: `${safety.steps.length} steps · ${saV} verify · ${(saTok/1000).toFixed(0)}k tok` },
      style: INV + `background:#0d0f14;border:1px dashed #5c637840;color:#5c6378;border-radius:8px;padding:6px 12px;font-size:10px;font-family:ui-monospace,monospace;text-align:center;transition:all 0.7s;`,
      sourcePosition: Position.Right, targetPosition: Position.Left,
    })

    // Safety branch — also NEUTRAL (same as speed)
    safety.steps.forEach((step, i) => {
      const c = rc[step.role] ?? '#5c6378'
      nodes.push({
        id: `sa-${i}`, position: { x: 0, y: 0 }, data: { label: step.name },
        style: INV + neutralNode(c),
        sourcePosition: Position.Right, targetPosition: Position.Left,
      })
      const deps = step.depends_on
      if (deps.length === 0) {
        edges.push({ id: `esa-${i}o`, source: 'origin', target: `sa-${i}`, animated: false, style: `opacity:0;transition:all 0.7s cubic-bezier(0.4,0,0.2,1);stroke:${c}30;stroke-width:1.5;` })
      } else {
        deps.forEach((dep, di) => {
          const depIdx = safety.steps.findIndex(s => s.name === dep)
          if (depIdx >= 0) edges.push({ id: `esa-${i}d${di}`, source: `sa-${depIdx}`, target: `sa-${i}`, animated: false, style: `opacity:0;transition:all 0.7s cubic-bezier(0.4,0,0.2,1);stroke:${c}30;stroke-width:1.5;` })
        })
      }
    })

    // Merge node
    nodes.push({
      id: 'merge', position: { x: 0, y: 0 },
      data: { label: '?' },
      type: 'output',
      style: INV + 'background:#12151c;border:2px solid #2a3546;color:#5c6378;border-radius:10px;padding:12px 20px;font-size:15px;font-family:ui-monospace,monospace;text-align:center;transition:all 0.8s cubic-bezier(0.4,0,0.2,1);',
      targetPosition: Position.Left,
    })

    // Edges to merge
    edges.push(
      { id: 'esp-m', source: `sp-${speed.steps.length-1}`, target: 'score-sp', animated: false, style: `opacity:0;transition:all 0.7s;stroke:#5c637830;stroke-width:1;` },
      { id: 'esp-m2', source: 'score-sp', target: 'merge', animated: false, style: `opacity:0;transition:all 0.7s;stroke:#5c637830;stroke-width:1;` },
      { id: 'esa-m', source: `sa-${safety.steps.length-1}`, target: 'score-sa', animated: false, style: `opacity:0;transition:all 0.7s;stroke:#5c637830;stroke-width:1;` },
      { id: 'esa-m2', source: 'score-sa', target: 'merge', animated: false, style: `opacity:0;transition:all 0.7s;stroke:#5c637830;stroke-width:1;` },
    )

    // Dagre layout
    const g = new dagre.graphlib.Graph()
    g.setDefaultEdgeLabel(() => ({}))
    g.setGraph({ rankdir: 'LR', ranksep: 70, nodesep: 30 })
    nodes.forEach(n => g.setNode(n.id, { width: 180, height: 55 }))
    edges.forEach(e => g.setEdge(e.source, e.target))
    dagre.layout(g)
    nodes.forEach(n => { const p = g.node(n.id); n.position = { x: p.x - 90, y: p.y - 27 } })

    return { nodes, edges }
  }

  const graph = buildGraph()

  // Reveal schedule — both branches appear simultaneously at same pace
  const sched: string[][] = [['origin']]
  const maxL = Math.max(speed.steps.length, safety.steps.length)
  for (let i = 0; i < maxL; i++) {
    const b: string[] = []
    if (i < speed.steps.length) b.push(`sp-${i}`)
    if (i < safety.steps.length) b.push(`sa-${i}`)
    sched.push(b)
  }
  sched.push(['merge'])

  let nodes = $state.raw<Node[]>(graph.nodes)
  let edges = $state.raw<Edge[]>(graph.edges)
  let statusText = $state('')
  let timers: ReturnType<typeof setTimeout>[] = []
  let overlayClass = $state('explorer-enter')
  let exiting = $state(false)

  function show(ids: Set<string>) {
    nodes = nodes.map(n => ids.has(n.id)
      ? { ...n, style: (n.style ?? '').replace(INV, VIS) }
      : n
    )
    // Show edges when both source and target are visible — no winner preference
    edges = edges.map(e => {
      if (ids.has(e.source) && ids.has(e.target)) {
        return { ...e, animated: false, style: (e.style ?? '').replace('opacity:0;', 'opacity:0.6;') }
      }
      return e
    })
  }

  function startReveal() {
    cleanup()
    overlayClass = 'explorer-enter'
    exiting = false
    nodes = graph.nodes.map(n => ({ ...n }))
    edges = graph.edges.map(e => ({ ...e }))
    const shown = new Set<string>()
    sched.forEach((batch, idx) => {
      timers.push(setTimeout(() => {
        batch.forEach(id => shown.add(id))
        show(shown)
        if (idx === 0) statusText = 'Analyzing task...'
        else if (idx <= 2) statusText = 'Exploring strategies...'
        else if (idx < sched.length - 1) statusText = 'Building candidate plans...'
        else statusText = 'Plans ready — evaluating...'
      }, idx * 700))
    })
  }

  function showScoring() {
    statusText = 'Scoring fitness...'
    // Reveal score summary nodes + edges to merge
    const scoreIds = new Set(['score-sp', 'score-sa', ...nodes.filter(n => (n.style ?? '').includes(VIS)).map(n => n.id)])
    show(scoreIds)

    // Merge node starts pulsing — suspense
    nodes = nodes.map(n => n.id === 'merge'
      ? { ...n, style: (n.style ?? '').replace('border:2px solid #2a3546', 'border:2px solid #fbbf2460'), data: { label: '...' } }
      : n
    )
  }

  function selectWinner() {
    const lp = safety.winner ? 'sp' : 'sa'
    const wp = safety.winner ? 'sa' : 'sp'
    const winner = safety.winner ? safety : speed
    const winScore = Math.round(winner.score * 100)

    // Phase 1: Brief suspense — both branches pulse
    statusText = 'Selecting optimal plan...'
    nodes = nodes.map(n => {
      if (n.id === 'merge') {
        return { ...n, data: { label: '...' }, style: (n.style ?? '')
          .replace('border:2px solid #fbbf2460', 'border:2px solid #fbbf24')
          .replace('color:#5c6378', 'color:#fbbf24')
        }
      }
      return n
    })

    // Phase 2: After 800ms — reveal winner, dim loser
    timers.push(setTimeout(() => {
      // Winner branch lights up with full color + glow
      nodes = nodes.map(n => {
        if (n.id.startsWith(wp) || n.id === `score-${wp}`) {
          const c = (() => {
            const idx = parseInt(n.id.split('-')[1])
            const steps = safety.winner ? safety.steps : speed.steps
            if (n.id.startsWith('score')) return '#22d3ee'
            return rc[steps[idx]?.role] ?? '#22d3ee'
          })()
          return { ...n, style: (n.style ?? '').replace(neutralNode(c), winnerNode(c)) }
        }
        // Loser branch fades dramatically
        if (n.id.startsWith(lp) || n.id === `score-${lp}`) {
          return { ...n, style: (n.style ?? '').replace(VIS, DIM) }
        }
        // Merge node → winner score
        if (n.id === 'merge') {
          return { ...n, data: { label: `${winScore}%` }, style: (n.style ?? '')
            .replace('background:#12151c', 'background:#34d39915')
            .replace('border:2px solid #fbbf24', 'border:2px solid #34d399')
            .replace('color:#fbbf24', 'color:#34d399')
          }
        }
        return n
      })

      // Winner edges glow, loser edges fade
      edges = edges.map(e => {
        if (e.id.startsWith(`e${lp}`) || e.id === `e${lp}-m` || e.id === `e${lp}-m2`) {
          return { ...e, animated: false, style: (e.style ?? '').replace(/opacity:[0-9.]+;/, 'opacity:0.03;') }
        }
        if (e.id.startsWith(`e${wp}`) || e.id === `e${wp}-m` || e.id === `e${wp}-m2`) {
          return { ...e, animated: true, style: (e.style ?? '').replace(/opacity:[0-9.]+;/, 'opacity:1;').replace(/stroke-width:[0-9.]+;/, 'stroke-width:2;') }
        }
        return e
      })

      statusText = `${winner.strategy.charAt(0).toUpperCase() + winner.strategy.slice(1)} plan selected — ${winScore}% fitness`
    }, 800))
  }

  function startExit() {
    exiting = true
    overlayClass = 'explorer-exit'
  }

  function cleanup() { timers.forEach(clearTimeout); timers = [] }

  let lastPhase = ''
  onMount(() => {
    const iv = setInterval(() => {
      if (phase === lastPhase) return
      lastPhase = phase
      if (phase === 'generating') startReveal()
      else if (phase === 'scoring') showScoring()
      else if (phase === 'selecting') selectWinner()
      else if (phase === 'done') {
        // Smooth exit — wait for CSS animation before removing from DOM
        startExit()
        timers.push(setTimeout(cleanup, 1200))
      }
      else if (phase === 'idle') cleanup()
    }, 80)
    return () => { clearInterval(iv); cleanup() }
  })

  let visible = $derived(phase !== 'idle' && !(phase === 'done' && !exiting))
  // Keep visible during exit animation, hide after it completes
  let showOverlay = $state(false)
  $effect(() => {
    if (phase !== 'idle' && phase !== 'done') {
      showOverlay = true
    } else if (phase === 'done') {
      // Start exit, then hide after animation
      showOverlay = true
      const t = setTimeout(() => { showOverlay = false }, 1200)
      return () => clearTimeout(t)
    } else {
      showOverlay = false
    }
  })
</script>

{#if showOverlay}
  <div class="absolute inset-0 z-20 {overlayClass}">
    <div class="absolute top-4 left-1/2 -translate-x-1/2 z-30 flex items-center gap-2.5">
      <div class="pip {phase === 'selecting' || phase === 'done' ? 'pip-done' : 'pip-active'}"></div>
      <span class="text-xs font-mono text-text-2 uppercase tracking-[0.15em] status-text">{statusText}</span>
    </div>

    <SvelteFlow
      bind:nodes
      bind:edges
      fitView
      fitViewOptions={{ padding: 0.12, minZoom: 0.35, maxZoom: 1 }}
      panOnDrag={false}
      zoomOnScroll={false}
      zoomOnPinch={false}
      nodesDraggable={false}
      nodesConnectable={false}
      elementsSelectable={false}
      proOptions={{ hideAttribution: true }}
    >
      <Background variant={BackgroundVariant.Dots} gap={24} size={0.4} bgColor="#08090c" color="#1c203030" />
    </SvelteFlow>
  </div>
{/if}

<style>
  /* --- Entrance: blur + fade over 0.8s --- */
  .explorer-enter {
    animation: explorerIn 0.8s cubic-bezier(0.4, 0, 0.2, 1) forwards;
    background: rgba(8, 9, 12, 0);
  }
  @keyframes explorerIn {
    0% { opacity: 0; backdrop-filter: blur(0px); background: rgba(8, 9, 12, 0); }
    40% { opacity: 0.6; }
    100% { opacity: 1; backdrop-filter: blur(3px); background: rgba(8, 9, 12, 0.94); }
  }

  /* --- Exit: fade + blur dissolve over 1s --- */
  .explorer-exit {
    animation: explorerOut 1s cubic-bezier(0.4, 0, 0.2, 1) forwards;
  }
  @keyframes explorerOut {
    0% { opacity: 1; backdrop-filter: blur(3px); background: rgba(8, 9, 12, 0.94); }
    60% { opacity: 0.4; }
    100% { opacity: 0; backdrop-filter: blur(0px); background: rgba(8, 9, 12, 0); transform: scale(0.98); }
  }

  /* --- Status text fade --- */
  .status-text {
    transition: opacity 0.4s ease;
  }

  :global(.svelte-flow) { background: transparent !important; }
  :global(.svelte-flow__handle) { opacity: 0 !important; width: 1px !important; height: 1px !important; }
  :global(.svelte-flow__edge-path) { transition: opacity 0.7s cubic-bezier(0.4,0,0.2,1), stroke 0.7s, stroke-width 0.5s; }
</style>
