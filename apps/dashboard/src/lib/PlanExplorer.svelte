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

  const INV = 'opacity:0;transition:all 0.6s ease;transform:scale(0.85);'
  const VIS = 'opacity:1;transform:scale(1);'

  // Common node style
  const ns = (c: string, bright: boolean, extra = '') =>
    `background:${bright ? c+'10' : '#0d0f14'};border:1.5px solid ${c}${bright?'':'40'};color:${bright?'#d0d6e0':'#5c6378'};border-radius:10px;padding:10px 16px;font-size:13px;min-width:140px;text-align:center;${extra}`

  function buildGraph() {
    const nodes: Node[] = []
    const edges: Edge[] = []

    // Origin
    nodes.push({
      id: 'origin', position: { x: 0, y: 0 }, data: { label: 'Task' }, type: 'input',
      style: INV + 'background:#1c2030;border:2px solid #4b8df8;color:#e8ecf4;border-radius:10px;padding:12px 20px;font-size:15px;font-family:ui-monospace,monospace;text-align:center;',
      sourcePosition: Position.Right,
    })

    // Speed (top)
    speed.steps.forEach((step, i) => {
      const c = rc[step.role] ?? '#5c6378'
      const w = speed.winner
      nodes.push({
        id: `sp-${i}`, position: { x: 0, y: 0 }, data: { label: step.name },
        style: INV + ns(c, w),
        sourcePosition: Position.Right, targetPosition: Position.Left,
      })
      edges.push({
        id: `esp-${i}`,
        source: i===0 ? 'origin' : `sp-${i-1}`, target: `sp-${i}`,
        animated: false,
        style: `opacity:0;transition:all 0.6s;stroke:${c}${w?'':'40'};stroke-width:${w?2:1};`,
      })
    })

    // Score summary nodes (shown during scoring phase)
    const spTok = speed.steps.reduce((a, s) => a + s.tokens, 0)
    const saTok = safety.steps.reduce((a, s) => a + s.tokens, 0)
    const spV = speed.steps.filter(s => s.verify).length
    const saV = safety.steps.filter(s => s.verify).length

    nodes.push({
      id: 'score-sp', position: { x: 0, y: 0 },
      data: { label: `${speed.steps.length} steps · ${spV} verify · ${(spTok/1000).toFixed(0)}k tok` },
      style: INV + `background:#0d0f14;border:1px dashed #5c637840;color:#5c6378;border-radius:8px;padding:6px 12px;font-size:10px;font-family:ui-monospace,monospace;text-align:center;`,
      sourcePosition: Position.Right, targetPosition: Position.Left,
    })
    nodes.push({
      id: 'score-sa', position: { x: 0, y: 0 },
      data: { label: `${safety.steps.length} steps · ${saV} verify · ${(saTok/1000).toFixed(0)}k tok` },
      style: INV + `background:#0d0f14;border:1px dashed #22d3ee30;color:#22d3ee90;border-radius:8px;padding:6px 12px;font-size:10px;font-family:ui-monospace,monospace;text-align:center;`,
      sourcePosition: Position.Right, targetPosition: Position.Left,
    })

    // Safety (bottom)
    safety.steps.forEach((step, i) => {
      const c = rc[step.role] ?? '#5c6378'
      const w = safety.winner
      const vg = step.verify ? `box-shadow:0 0 0 2px ${c}20;` : ''
      nodes.push({
        id: `sa-${i}`, position: { x: 0, y: 0 }, data: { label: step.name },
        style: INV + ns(c, w, vg),
        sourcePosition: Position.Right, targetPosition: Position.Left,
      })
      const deps = step.depends_on
      if (deps.length === 0) {
        edges.push({ id: `esa-${i}o`, source: 'origin', target: `sa-${i}`, animated: false, style: `opacity:0;transition:all 0.6s;stroke:${c}${w?'':'40'};stroke-width:${w?2:1};` })
      } else {
        deps.forEach((dep, di) => {
          const depIdx = safety.steps.findIndex(s => s.name === dep)
          if (depIdx >= 0) edges.push({ id: `esa-${i}d${di}`, source: `sa-${depIdx}`, target: `sa-${i}`, animated: false, style: `opacity:0;transition:all 0.6s;stroke:${c}${w?'':'40'};stroke-width:${w?2:1};` })
        })
      }
    })

    // Merge with score display
    nodes.push({
      id: 'merge', position: { x: 0, y: 0 },
      data: { label: '?' },
      type: 'output',
      style: INV + 'background:#12151c;border:2px solid #2a3546;color:#5c6378;border-radius:10px;padding:12px 20px;font-size:15px;font-family:ui-monospace,monospace;text-align:center;',
      targetPosition: Position.Left,
    })

    // Edges to merge
    edges.push(
      { id: 'esp-m', source: `sp-${speed.steps.length-1}`, target: 'score-sp', animated: false, style: `opacity:0;transition:all 0.6s;stroke:#5c637830;stroke-width:1;` },
      { id: 'esp-m2', source: 'score-sp', target: 'merge', animated: false, style: `opacity:0;transition:all 0.6s;stroke:#5c637830;stroke-width:1;` },
      { id: 'esa-m', source: `sa-${safety.steps.length-1}`, target: 'score-sa', animated: false, style: `opacity:0;transition:all 0.6s;stroke:#22d3ee40;stroke-width:1;` },
      { id: 'esa-m2', source: 'score-sa', target: 'merge', animated: false, style: `opacity:0;transition:all 0.6s;stroke:#22d3ee40;stroke-width:1;` },
    )

    // Dagre
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

  // Reveal schedule
  const sched: string[][] = [['origin']]
  const maxL = Math.max(speed.steps.length, safety.steps.length)
  for (let i = 0; i < maxL; i++) {
    const b: string[] = []
    if (i < speed.steps.length) b.push(`sp-${i}`)
    if (i < safety.steps.length) b.push(`sa-${i}`)
    sched.push(b)
  }
  // Score nodes appear during scoring phase, not here
  sched.push(['merge'])

  let nodes = $state.raw<Node[]>(graph.nodes)
  let edges = $state.raw<Edge[]>(graph.edges)
  let statusText = $state('')
  let timers: ReturnType<typeof setTimeout>[] = []

  function show(ids: Set<string>) {
    nodes = nodes.map(n => ids.has(n.id)
      ? { ...n, style: (n.style??'').replace(INV, VIS) }
      : n
    )
    edges = edges.map(e => {
      if (ids.has(e.source) && ids.has(e.target)) {
        const win = (safety.winner && e.id.startsWith('esa')) || (speed.winner && e.id.startsWith('esp'))
        return { ...e, animated: win, style: (e.style??'').replace('opacity:0;','opacity:1;') }
      }
      return e
    })
  }

  function startReveal() {
    cleanup()
    nodes = graph.nodes.map(n => ({ ...n }))
    edges = graph.edges.map(e => ({ ...e }))
    const shown = new Set<string>()
    sched.forEach((batch, idx) => {
      timers.push(setTimeout(() => {
        batch.forEach(id => shown.add(id))
        show(shown)
        if (idx === 0) statusText = 'Analyzing task...'
        else if (idx <= 2) statusText = 'Exploring strategies...'
        else if (idx < sched.length - 1) statusText = 'Building plans...'
        else statusText = 'Converging...'
      }, idx * 650))
    })
  }

  function showScoring() {
    statusText = 'Evaluating fitness...'
    // Reveal score summary nodes + their edges
    const scoreIds = new Set(['score-sp', 'score-sa', ...Array.from(nodes.filter(n => n.style?.includes(VIS)).map(n => n.id))])
    show(scoreIds)

    // Update merge node to show "?"  with subtle animation
    nodes = nodes.map(n => n.id === 'merge'
      ? { ...n, style: (n.style??'').replace('border:2px solid #2a3546','border:2px solid #22d3ee50') }
      : n
    )
  }

  function selectWinner() {
    const lp = safety.winner ? 'sp' : 'sa'
    const wp = safety.winner ? 'sa' : 'sp'
    const winner = safety.winner ? safety : speed
    const winScore = Math.round(winner.score * 100)

    // Dim loser branch + its score node
    nodes = nodes.map(n => {
      if (n.id.startsWith(lp) || n.id === `score-${lp}`) {
        return { ...n, style: (n.style??'').replace(VIS, 'opacity:0.07;transform:scale(1);') }
      }
      if (n.id === 'merge') {
        return { ...n, data: { label: `${winScore}%` }, style: (n.style??'')
          .replace('background:#12151c','background:#34d39912')
          .replace('border:2px solid #22d3ee50','border:2px solid #34d399')
          .replace('color:#5c6378','color:#34d399')
        }
      }
      return n
    })
    edges = edges.map(e => {
      if (e.id.startsWith(`e${lp}`) || e.id === `e${lp}-m` || e.id === `e${lp}-m2`) {
        return { ...e, animated: false, style: (e.style??'').replace('opacity:1;','opacity:0.05;') }
      }
      return e
    })
    statusText = `${winner.strategy.charAt(0).toUpperCase() + winner.strategy.slice(1)} plan selected — ${winScore}% fitness`
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
      else if (phase === 'done' || phase === 'idle') cleanup()
    }, 80)
    return () => { clearInterval(iv); cleanup() }
  })

  let visible = $derived(phase !== 'idle' && phase !== 'done')
</script>

{#if visible}
  <div class="absolute inset-0 z-20 bg-bg/95" style="animation: fadeIn 0.3s ease-out;">
    <div class="absolute top-3 left-1/2 -translate-x-1/2 z-30 flex items-center gap-2">
      <div class="pip {phase === 'selecting' ? 'pip-done' : 'pip-active'}"></div>
      <span class="text-xs font-mono text-text-3 uppercase tracking-[0.15em]">{statusText}</span>
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
  @keyframes fadeIn { from{opacity:0} to{opacity:1} }
  :global(.svelte-flow) { background: transparent !important; }
  /* Hide the ugly default handles */
  :global(.svelte-flow__handle) { opacity: 0 !important; width: 1px !important; height: 1px !important; }
  /* Smooth edge transitions */
  :global(.svelte-flow__edge-path) { transition: opacity 0.6s, stroke 0.6s; }
</style>
