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
  import type { StepRow } from './types'
  import type { Thought } from './demo'
  import DagNode from './DagNode.svelte'
  import VerifyGate from './VerifyGate.svelte'
  import TeamGroup from './TeamGroup.svelte'
  import PulseEdge from './PulseEdge.svelte'
  import ThoughtBubble from './ThoughtBubble.svelte'
  import type { TeammateMessage } from './event-player'

  /** Sub-plan state driven by events. Map: parentStepId → array of sub-step states. */
  export interface SubPlanEntry {
    subSteps: Array<{ id: string; name: string; status: 'pending' | 'running' | 'passed' | 'failed' }>
    completed: boolean
  }

  let { steps, selectedId, onSelect, thoughts = [], stepSummaries = [], teamInfo = null, teammateMessages = [], subPlanState = new Map() }: {
    steps: StepRow[]
    selectedId: string | null
    onSelect: (id: string) => void
    thoughts?: Thought[]
    stepSummaries?: Array<{
      id: string; name: string; depends_on: string[]; verify_after: boolean; execution_mode: string
    }>
    teamInfo?: { name: string; topology: string; member_count: number } | null
    teammateMessages?: TeammateMessage[]
    /** Event-driven sub-plan state. When provided, overrides synthetic sub-activity expansion. */
    subPlanState?: Map<string, SubPlanEntry>
  } = $props()

  const nodeTypes = { dagNode: DagNode, verifyGate: VerifyGate, teamGroup: TeamGroup }
  const edgeTypes = { pulse: PulseEdge }

  // ── Dependency map ─────────────────────────────────────────
  function getDepsMap(): Map<string, { depends_on: string[]; verify_after: boolean; execution_mode: string }> {
    const map = new Map<string, { depends_on: string[]; verify_after: boolean; execution_mode: string }>()
    if (stepSummaries.length > 0) {
      for (const s of stepSummaries) map.set(s.id, { depends_on: s.depends_on, verify_after: s.verify_after, execution_mode: s.execution_mode })
    } else {
      for (let i = 0; i < steps.length; i++) map.set(steps[i].id, { depends_on: i > 0 ? [steps[i - 1].id] : [], verify_after: steps[i].verify_passed != null, execution_mode: steps[i].execution_mode })
    }
    return map
  }

  // ── Teammate color palette ──────────────────────────────────
  const TEAMMATE_COLORS = ['#a78bfa', '#22d3ee', '#f472b6', '#fbbf24', '#34d399', '#fb923c']
  // Map: stepId → assigned color
  let teammateColorMap = $derived.by(() => {
    const map = new Map<string, string>()
    const deps = getDepsMap()
    let idx = 0
    for (const step of steps) {
      const d = deps.get(step.id)
      if (d?.execution_mode === 'teammate') {
        map.set(step.id, TEAMMATE_COLORS[idx % TEAMMATE_COLORS.length])
        idx++
      }
    }
    return map
  })

  // ── Layout constants ───────────────────────────────────────
  const NODE_W = 190
  const NODE_H_BASE = 76
  const GATE_SIZE = 32
  const RANKSEP = 100
  const NODESEP = 60
  const GROUP_PAD = 40

  // ── Adaptive node height ──────────────────────────────────
  // Single source of truth: computes expected pixel height from node data.
  // Used by Dagre layout, TeamGroup bounds, and fitView.
  const SUB_STRIP_OVERHEAD = 30  // divider + counter + margins
  const SUB_LINE_H = 20         // each sub-step row
  const STATS_ROW_H = 20        // duration + tokens row (shown on completed nodes)

  function nodeHeight(data: Record<string, unknown>): number {
    let h = NODE_H_BASE
    // Sub-steps expand the node
    const subs = data?.subSteps as Array<unknown> | null
    if (subs && subs.length > 0) h += SUB_STRIP_OVERHEAD + subs.length * SUB_LINE_H
    // Stats row appears on completed nodes with duration
    if (data?.duration_ms != null) h += STATS_ROW_H
    return h
  }

  function gateHeight(): number { return GATE_SIZE }

  // ── Sub-activity labels per step name pattern ──────────────
  function subLabelsFor(stepName: string): string[] {
    const lower = stepName.toLowerCase()
    if (lower.includes('auth')) return ['Scanning auth flow', 'Writing middleware', 'Adding token refresh']
    if (lower.includes('api')) return ['Mapping endpoints', 'Generating handlers', 'Wiring routes']
    if (lower.includes('ui') || lower.includes('frontend')) return ['Scaffolding components', 'Styling views', 'Binding state']
    if (lower.includes('schema') || lower.includes('migration')) return ['Reading schema', 'Writing migrations', 'Validating constraints']
    if (lower.includes('test')) return ['Generating fixtures', 'Writing assertions', 'Running suite']
    if (lower.includes('benchmark') || lower.includes('perf')) return ['Setting up harness', 'Running load test', 'Collecting metrics']
    return ['Analyzing scope', 'Implementing changes', 'Self-checking']
  }

  // ── Edge style helpers ─────────────────────────────────────
  function edgeStyleForMode(sourceStatus: string, targetMode: string): string {
    const base = 'transition: all 0.6s;'
    const statusStroke = sourceStatus === 'passed' ? '#34d39960' : sourceStatus === 'running' ? '#22d3ee40' : sourceStatus === 'failed' ? '#f8717140' : '#1c203060'
    const width = sourceStatus === 'passed' ? 2 : sourceStatus === 'running' ? 1.5 : 1
    if (targetMode === 'subagent') {
      const stroke = sourceStatus === 'passed' ? '#fbbf2450' : sourceStatus === 'pending' ? '#fbbf2420' : statusStroke
      return `stroke: ${stroke}; stroke-width: ${width}; stroke-dasharray: 6,4; ${base}`
    }
    if (targetMode === 'teammate') {
      const stroke = sourceStatus === 'passed' ? '#a78bfa50' : sourceStatus === 'pending' ? '#a78bfa20' : statusStroke
      return `stroke: ${stroke}; stroke-width: ${width}; ${base}`
    }
    return `stroke: ${statusStroke}; stroke-width: ${width}; ${base}`
  }

  // ── Build main graph (Dagre layout) ────────────────────────
  function buildGraph(stepsData: StepRow[]): { nodes: Node[]; edges: Edge[] } {
    if (stepsData.length === 0) return { nodes: [], edges: [] }
    const deps = getDepsMap()
    const nodes: Node[] = []
    const edges: Edge[] = []

    for (const step of stepsData) {
      const d = deps.get(step.id)
      const mode = d?.execution_mode ?? step.execution_mode
      const tmColor = teammateColorMap.get(step.id) ?? null
      // Resolve sub-steps: event-driven first, synthetic fallback
      let nodeSubSteps: Array<{ id: string; name: string; status: string }> | null = null
      const eventSub = subPlanState.get(step.id)
      if (eventSub && !eventSub.completed) {
        nodeSubSteps = eventSub.subSteps
      } else {
        const synth = syntheticStates.get(step.id)
        if (synth) {
          const labels = subLabelsFor(step.name)
          nodeSubSteps = synth.map((st, i) => ({ id: `synth-${i}`, name: labels[i] ?? `Task ${i + 1}`, status: st }))
        }
      }
      nodes.push({
        id: step.id, type: 'dagNode', position: { x: 0, y: 0 },
        data: { name: step.name, role: step.role, status: step.status, execution_mode: mode, verify_passed: step.verify_passed, duration_ms: step.duration_ms, tokens_used: step.tokens_used, teammateColor: tmColor, subSteps: nodeSubSteps },
        sourcePosition: Position.Right, targetPosition: Position.Left,
      })
      if (d?.verify_after) {
        const gateId = `gate-${step.id}`
        nodes.push({ id: gateId, type: 'verifyGate', position: { x: 0, y: 0 }, data: { passed: step.verify_passed }, sourcePosition: Position.Right, targetPosition: Position.Left })
        edges.push({ id: `e-${step.id}-gate`, source: step.id, target: gateId, animated: step.status === 'passed' || step.status === 'failed', style: edgeStyleForMode(step.status, 'inline') })
      }
      if (d) {
        for (const depId of d.depends_on) {
          const depStep = stepsData.find(s => s.id === depId)
          const depInfo = deps.get(depId)
          const sourceId = depInfo?.verify_after ? `gate-${depId}` : depId
          const sourceStatus = depStep?.status ?? 'pending'

          // Detect teammate-to-teammate edges → use pulse edge type
          const bothTeammates = mode === 'teammate' && depInfo?.execution_mode === 'teammate'
          const bothRunning = step.status === 'running' && depStep?.status === 'running'
          // Find flash message for this edge (most recent teammate_message between these two steps)
          const flash = teammateMessages.find(m =>
            (m.fromStepId === depId && m.toStepId === step.id) ||
            (m.fromStepId === step.id && m.toStepId === depId)
          )

          if (bothTeammates) {
            const cSrc = teammateColorMap.get(depId) ?? '#a78bfa'
            const cTgt = teammateColorMap.get(step.id) ?? '#22d3ee'
            const flashIsRev = flash ? flash.fromStepId === step.id : false
            const flashSdrColor = flash ? (teammateColorMap.get(flash.fromStepId) ?? '#e8ecf4') : '#e8ecf4'
            edges.push({
              id: `e-${sourceId}-${step.id}`,
              source: sourceId,
              target: step.id,
              type: 'pulse',
              animated: false,
              data: {
                isActive: bothRunning,
                sourceColor: cSrc,
                targetColor: cTgt,
                flashMessage: flash?.summary ?? '',
                flashSender: flash?.fromName ?? '',
                flashColor: flashSdrColor,
                flashReverse: flashIsRev,
              },
            })
          } else {
            edges.push({ id: `e-${sourceId}-${step.id}`, source: sourceId, target: step.id, animated: sourceStatus === 'passed', style: edgeStyleForMode(sourceStatus, mode) })
          }
        }
      }
    }

    const g = new dagre.graphlib.Graph()
    g.setDefaultEdgeLabel(() => ({}))
    g.setGraph({ rankdir: 'LR', ranksep: RANKSEP, nodesep: NODESEP })

    // Adaptive layout: each node's Dagre height matches its actual content
    const nodeHeights = new Map<string, number>()
    for (const n of nodes) {
      const isGate = n.type === 'verifyGate'
      const h = isGate ? gateHeight() : nodeHeight(n.data as Record<string, unknown>)
      nodeHeights.set(n.id, h)
      g.setNode(n.id, { width: isGate ? GATE_SIZE : NODE_W, height: h })
    }
    for (const e of edges) g.setEdge(e.source, e.target)
    dagre.layout(g)

    for (const n of nodes) {
      const pos = g.node(n.id)
      const w = n.type === 'verifyGate' ? GATE_SIZE : NODE_W
      const h = nodeHeights.get(n.id) ?? NODE_H_BASE
      n.position = { x: pos.x - w / 2, y: pos.y - h / 2 }
    }
    return { nodes, edges }
  }

  // ── Sub-activity expansion (dual mode: event-driven or synthetic) ──
  //
  // Priority: subPlanState prop (from events) > synthetic fallback (timer-based)
  // The synthetic mode activates only for subagent steps that have NO event-driven sub-plan.

  let syntheticStates = $state<Map<string, ('pending' | 'running' | 'passed')[]>>(new Map())
  let subTimers: ReturnType<typeof setTimeout>[] = []

  // Detect running subagents that need synthetic expansion (no event-driven sub-plan)
  let runningSubagentKey = $derived.by(() => {
    const deps = getDepsMap()
    const ids: string[] = []
    for (const step of steps) {
      const d = deps.get(step.id)
      if (d?.execution_mode === 'subagent' && step.status === 'running') {
        // Only synthetic if no event-driven sub-plan exists for this step
        if (!subPlanState.has(step.id)) ids.push(step.id)
      }
    }
    return ids.sort().join(',')
  })

  let _prevSubKey = ''

  $effect(() => {
    const key = runningSubagentKey
    if (key === _prevSubKey) return
    const prevIds = new Set(_prevSubKey ? _prevSubKey.split(',') : [])
    const currIds = new Set(key ? key.split(',') : [])
    _prevSubKey = key
    for (const id of currIds) { if (!prevIds.has(id)) expandSynthetic(id) }
    for (const id of prevIds) { if (!currIds.has(id)) collapseSynthetic(id) }
  })

  function expandSynthetic(stepId: string) {
    const step = steps.find(s => s.id === stepId)
    const labels = subLabelsFor(step?.name ?? '')
    const states: ('pending' | 'running' | 'passed')[] = labels.map(() => 'pending')
    syntheticStates = new Map(syntheticStates).set(stepId, states)
    for (let i = 0; i < labels.length; i++) {
      subTimers.push(setTimeout(() => {
        const c = syntheticStates.get(stepId); if (!c) return
        const n = [...c]; n[i] = 'running'
        syntheticStates = new Map(syntheticStates).set(stepId, n)
      }, 600 + i * 1800))
      subTimers.push(setTimeout(() => {
        const c = syntheticStates.get(stepId); if (!c) return
        const n = [...c]; n[i] = 'passed'
        syntheticStates = new Map(syntheticStates).set(stepId, n)
      }, 1800 + i * 1800))
    }
  }

  function collapseSynthetic(stepId: string) {
    subTimers.push(setTimeout(() => {
      const next = new Map(syntheticStates); next.delete(stepId); syntheticStates = next
    }, 600))
  }

  // ── Team group as real Svelte Flow node ────────────────────
  function buildTeamGroupNode(graphNodes: Node[], depsMap: Map<string, { depends_on: string[]; verify_after: boolean; execution_mode: string }>): Node | null {
    const teammateIds = new Set<string>()
    for (const [id, info] of depsMap) { if (info.execution_mode === 'teammate') teammateIds.add(id) }
    if (teammateIds.size < 2) return null

    const teammateNodes = graphNodes.filter(n => teammateIds.has(n.id))
    if (teammateNodes.length < 2) return null

    const label = teamInfo ? `${teamInfo.name} (${teamInfo.topology})` : 'Agent Team (mesh)'

    // Build mini-feed from teammate messages with assigned colors
    const feedMessages = teammateMessages.map(m => ({
      from: m.fromName.split(' ').slice(0, 2).join(' '),
      to: m.toName.split(' ').slice(0, 2).join(' '),
      summary: m.summary,
      fromColor: teammateColorMap.get(m.fromStepId) ?? '#a78bfa',
      toColor: teammateColorMap.get(m.toStepId) ?? '#22d3ee',
    }))

    // Adaptive bounds: use actual node height (accounts for sub-steps, stats, etc.)
    const minX = Math.min(...teammateNodes.map(n => n.position.x)) - GROUP_PAD
    const minY = Math.min(...teammateNodes.map(n => n.position.y)) - GROUP_PAD - 24
    const maxX = Math.max(...teammateNodes.map(n => n.position.x + NODE_W)) + GROUP_PAD
    const feedHeight = feedMessages.length > 0 ? Math.min(feedMessages.length, 3) * 24 + 28 : 0
    const maxY = Math.max(...teammateNodes.map(n => {
      const h = nodeHeight(n.data as Record<string, unknown>)
      return n.position.y + h
    })) + GROUP_PAD + feedHeight

    return {
      id: 'team-group',
      type: 'teamGroup',
      position: { x: minX, y: minY },
      data: { label, width: maxX - minX, height: maxY - minY, messages: feedMessages, messageCount: feedMessages.length },
      zIndex: -1,
      sourcePosition: Position.Right,
      targetPosition: Position.Left,
      selectable: false,
      draggable: false,
    }
  }

  // ── Merge everything ───────────────────────────────────────
  function mergeAll(mainGraph: { nodes: Node[]; edges: Edge[] }): { nodes: Node[]; edges: Edge[] } {
    const extraNodes: Node[] = []
    const extraEdges: Edge[] = []

    // Team group node
    const depsMap = getDepsMap()
    const groupNode = buildTeamGroupNode(mainGraph.nodes, depsMap)
    if (groupNode) extraNodes.push(groupNode)

    // Virtual communication edges between all teammate pairs
    // These don't exist as DAG dependencies — they're visual-only pulse edges
    const teammateStepIds: string[] = []
    for (const [id, info] of depsMap) { if (info.execution_mode === 'teammate') teammateStepIds.push(id) }
    for (let a = 0; a < teammateStepIds.length; a++) {
      for (let b = a + 1; b < teammateStepIds.length; b++) {
        const idA = teammateStepIds[a], idB = teammateStepIds[b]
        // Skip if a real DAG edge already exists between these two
        const hasRealEdge = mainGraph.edges.some(e =>
          (e.source === idA && e.target === idB) || (e.source === idB && e.target === idA))
        if (hasRealEdge) continue

        const stepA = steps.find(s => s.id === idA)
        const stepB = steps.find(s => s.id === idB)
        const bothRunning = stepA?.status === 'running' && stepB?.status === 'running'
        const colorA = teammateColorMap.get(idA) ?? '#a78bfa'
        const colorB = teammateColorMap.get(idB) ?? '#22d3ee'
        const flash = teammateMessages.find(m =>
          (m.fromStepId === idA && m.toStepId === idB) || (m.fromStepId === idB && m.toStepId === idA))
        // Determine flash direction: is sender the source or target of this edge?
        const flashIsReverse = flash ? flash.fromStepId === idB : false
        const flashSenderColor = flash ? (teammateColorMap.get(flash.fromStepId) ?? '#e8ecf4') : '#e8ecf4'

        extraEdges.push({
          id: `e-comm-${idA}-${idB}`,
          source: idA,
          target: idB,
          type: 'pulse',
          animated: false,
          data: {
            isActive: bothRunning,
            sourceColor: colorA,
            targetColor: colorB,
            flashMessage: flash?.summary ?? '',
            flashSender: flash?.fromName ?? '',
            flashColor: flashSenderColor,
            flashReverse: flashIsReverse,
          },
        })
      }
    }

    // Sub-steps are now rendered INSIDE DagNode (no separate SubActivity nodes)

    return {
      nodes: [...extraNodes, ...mainGraph.nodes],
      edges: [...mainGraph.edges, ...extraEdges],
    }
  }

  // ── Reactive graph ─────────────────────────────────────────
  let mainGraph = $derived(buildGraph(steps))
  let fullGraph = $derived(mergeAll(mainGraph))
  let nodes = $state<Node[]>([])
  let edges = $state<Edge[]>([])

  let revealedIds = $state<Set<string>>(new Set())
  let revealTimers: ReturnType<typeof setTimeout>[] = []

  function computeRevealBatches(g: { nodes: Node[]; edges: Edge[] }): string[][] {
    if (g.nodes.length === 0) return []
    const inDegree = new Map<string, number>()
    const outEdges = new Map<string, string[]>()
    for (const n of g.nodes) { inDegree.set(n.id, 0); outEdges.set(n.id, []) }
    for (const e of g.edges) { inDegree.set(e.target, (inDegree.get(e.target) ?? 0) + 1); outEdges.get(e.source)?.push(e.target) }
    const batches: string[][] = []
    let current = g.nodes.filter(n => (inDegree.get(n.id) ?? 0) === 0).map(n => n.id)
    while (current.length > 0) {
      batches.push(current)
      const next: string[] = []
      for (const id of current) { for (const t of (outEdges.get(id) ?? [])) { const d = (inDegree.get(t) ?? 1) - 1; inDegree.set(t, d); if (d === 0) next.push(t) } }
      current = next
    }
    return batches
  }

  function startReveal(g: { nodes: Node[]; edges: Edge[] }) {
    revealTimers.forEach(clearTimeout); revealTimers = []
    const batches = computeRevealBatches(g)
    const newRevealed = new Set<string>()
    // Always reveal special nodes immediately
    g.nodes.filter(n => n.type === 'teamGroup').forEach(n => newRevealed.add(n.id))

    if (batches.length <= 1) { g.nodes.forEach(n => newRevealed.add(n.id)); revealedIds = newRevealed; nodes = g.nodes; edges = g.edges; return }
    nodes = g.nodes.map(n => newRevealed.has(n.id) ? n : { ...n, style: 'opacity:0; transform: scale(0.85) translateX(-10px); transition: all 0.5s cubic-bezier(0.4, 0, 0.2, 1);' })
    edges = g.edges.map(e => ({ ...e, style: (e.style ?? '') + (newRevealed.has(e.source) && newRevealed.has(e.target) ? '' : ' opacity: 0;') }))

    batches.forEach((batch, idx) => {
      revealTimers.push(setTimeout(() => {
        batch.forEach(id => newRevealed.add(id)); revealedIds = new Set(newRevealed)
        nodes = g.nodes.map(n => newRevealed.has(n.id) ? n : { ...n, style: 'opacity:0; transform: scale(0.85) translateX(-10px); transition: all 0.5s cubic-bezier(0.4, 0, 0.2, 1);' })
        edges = g.edges.map(e => newRevealed.has(e.source) && newRevealed.has(e.target) ? e : { ...e, style: (e.style ?? '') + ' opacity: 0;' })
      }, idx * 350))
    })
  }

  let lastStepCount = 0
  let initialized = false

  $effect(() => {
    const g = fullGraph
    if (g.nodes.length === 0) { nodes = []; edges = []; initialized = false; lastStepCount = 0; return }
    const mainCount = g.nodes.filter(n => n.type === 'dagNode' || n.type === 'verifyGate').length

    if (!initialized || Math.abs(mainCount - lastStepCount) > 2) {
      initialized = true; lastStepCount = mainCount; startReveal(g); return
    }
    lastStepCount = mainCount

    const allRevealed = new Set(revealedIds)
    g.nodes.filter(n => n.type === 'teamGroup').forEach(n => allRevealed.add(n.id))

    nodes = g.nodes.map(n => allRevealed.has(n.id) ? n : { ...n, style: 'opacity:0; transform: scale(0.85) translateX(-10px); transition: all 0.5s cubic-bezier(0.4, 0, 0.2, 1);' })
    edges = g.edges.map(e => {
      if (e.id.startsWith('e-sub-')) return e
      return allRevealed.has(e.source) && allRevealed.has(e.target) ? e : { ...e, style: (e.style ?? '') + ' opacity: 0;' }
    })
  })

  onMount(() => () => { revealTimers.forEach(clearTimeout); subTimers.forEach(clearTimeout) })

  function thoughtsFor(stepId: string): Thought[] { return thoughts.filter(t => t.stepId === stepId) }
  let activeStepId = $derived(steps.find(s => s.status === 'running')?.id ?? null)
  let hasSubagents = $derived(stepSummaries.some(s => s.execution_mode === 'subagent'))
  let hasTeammates = $derived(stepSummaries.some(s => s.execution_mode === 'teammate'))

  // Adaptive fitView: more padding for small graphs, less for dense ones
  let fitPadding = $derived(steps.length <= 3 ? 0.25 : steps.length <= 6 ? 0.15 : 0.08)
  let fitMaxZoom = $derived(steps.length <= 3 ? 1.0 : steps.length <= 8 ? 0.9 : 0.75)
</script>

<div class="w-full h-full relative">
  {#if steps.length === 0}
    <div class="flex items-center justify-center h-full">
      <div class="text-center opacity-40">
        <div class="text-text-3 text-sm">Flat execution</div>
        <div class="text-text-3 text-xs mt-1">No plan DAG</div>
      </div>
    </div>
  {:else}
    <SvelteFlow
      bind:nodes
      bind:edges
      {nodeTypes}
      {edgeTypes}
      fitView
      fitViewOptions={{ padding: fitPadding, minZoom: 0.2, maxZoom: fitMaxZoom }}
      panOnDrag={true}
      zoomOnScroll={true}
      zoomOnPinch={true}
      nodesDraggable={false}
      nodesConnectable={false}
      elementsSelectable={true}
      onnodeclick={({ node }: { node: Node; event: MouseEvent | TouchEvent }) => {
        if (node.id && !node.id.startsWith('gate-') && !node.id.startsWith('sub-') && node.id !== 'team-group') onSelect(node.id)
      }}
      proOptions={{ hideAttribution: true }}
    >
      <Background variant={BackgroundVariant.Dots} gap={24} size={0.4} bgColor="transparent" />
    </SvelteFlow>

    {#if hasSubagents || hasTeammates}
      <div class="legend">
        <span class="legend-item"><span class="legend-line legend-inline"></span><span>inline</span></span>
        {#if hasSubagents}
          <span class="legend-item"><span class="legend-line legend-fork"></span><span>fork (subagent)</span></span>
        {/if}
        {#if hasTeammates}
          <span class="legend-item"><span class="legend-line legend-team"></span><span>team</span></span>
          <span class="legend-item"><span class="legend-dot legend-comm"></span><span>comm</span></span>
        {/if}
      </div>
    {/if}

    {#if activeStepId && thoughtsFor(activeStepId).length > 0}
      <div class="absolute bottom-3 left-3 right-3 flex flex-col gap-1 z-10 pointer-events-none">
        {#each thoughtsFor(activeStepId).slice(-2) as thought (thought.text)}
          <ThoughtBubble text={thought.text} variant={thought.variant} />
        {/each}
      </div>
    {/if}
  {/if}
</div>

<style>
  :global(.svelte-flow) { background: transparent !important; }
  :global(.svelte-flow__handle) { opacity: 0 !important; width: 1px !important; height: 1px !important; }
  :global(.svelte-flow__edge-path) { transition: d 0.5s cubic-bezier(0.4, 0, 0.2, 1), opacity 0.6s cubic-bezier(0.4, 0, 0.2, 1), stroke 0.6s, stroke-width 0.5s; }
  :global(.svelte-flow__node) { transition: opacity 0.5s cubic-bezier(0.4, 0, 0.2, 1), transform 0.5s cubic-bezier(0.4, 0, 0.2, 1); }
  :global(.svelte-flow__node.selected) { box-shadow: 0 0 0 2px #4b8df840 !important; border-radius: 10px; }

  .legend {
    position: absolute; bottom: 10px; left: 10px; display: flex; gap: 14px;
    padding: 6px 12px; background: rgba(13,15,20,0.85); border: 1px solid #1c2030; border-radius: 8px; z-index: 5;
  }
  .legend-item { display: flex; align-items: center; gap: 6px; font-family: ui-monospace, monospace; font-size: 10px; color: #5c6378; }
  .legend-line { display: inline-block; width: 18px; height: 2px; border-radius: 1px; }
  .legend-inline { background: #5c637860; }
  .legend-fork { background: repeating-linear-gradient(90deg, #fbbf24 0, #fbbf24 4px, transparent 4px, transparent 7px); opacity: 0.6; }
  .legend-team { background: #a78bfa; opacity: 0.5; }
  .legend-dot { display: inline-block; width: 6px; height: 6px; border-radius: 50%; }
  .legend-comm { background: #a78bfa; box-shadow: 0 0 4px #a78bfa60; animation: legend-pulse 2s ease-in-out infinite; }
  @keyframes legend-pulse { 0%, 100% { opacity: 0.5; } 50% { opacity: 1; } }
</style>
