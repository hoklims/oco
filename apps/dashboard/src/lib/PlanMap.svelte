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
  // graph-animator removed — xyflow manages node positions via transform:translate()
  // inline; animating transform externally causes jank. Entry/exit uses opacity only.

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

  /**
   * Shared animation clock for all running nodes. Each step gets ONE phase
   * at first render and keeps it forever — so the glow animation never
   * shifts between renders. All nodes born near the same instant share a
   * close phase, giving parallel `running` steps a visually in-phase pulse.
   *
   * The phase is `(now - epoch) % period`, applied as a negative
   * `animation-delay` so the CSS animation is "already" at that offset.
   */
  const GLOW_PERIOD_MS = 2500
  const planAnimEpoch = performance.now()
  const nodePhaseCache = new Map<string, number>()
  function phaseFor(stepId: string): number {
    let p = nodePhaseCache.get(stepId)
    if (p === undefined) {
      p = (performance.now() - planAnimEpoch) % GLOW_PERIOD_MS
      nodePhaseCache.set(stepId, p)
    }
    return p
  }

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

  /** Sub-steps count reserved for subagent nodes before `sub_plan_started`. */
  const SUBAGENT_RESERVED_SUBSTEPS = 3

  function nodeHeight(data: Record<string, unknown>): number {
    let h = NODE_H_BASE
    // Sub-steps expand the node
    const subs = data?.subSteps as Array<unknown> | null
    const mode = data?.execution_mode as string | undefined
    if (subs && subs.length > 0) {
      h += SUB_STRIP_OVERHEAD + subs.length * SUB_LINE_H
    } else if (mode === 'subagent') {
      // Subagent nodes reserve space for sub-steps BEFORE the `sub_plan_started`
      // event arrives. Otherwise the node grows 90px when sub-steps appear,
      // forcing Dagre to relayout and making the node "jump".
      h += SUB_STRIP_OVERHEAD + SUBAGENT_RESERVED_SUBSTEPS * SUB_LINE_H
    }
    // Stats row space is ALWAYS reserved (even before completion) so the node
    // height stays stable across the pending→running→passed transition.
    h += STATS_ROW_H
    return h
  }

  function gateHeight(): number { return GATE_SIZE }

  // ── Layout memoization ─────────────────────────────────────
  //
  // Dagre re-runs on every steps/subPlanState change because `buildGraph` is
  // `$derived`. For high-frequency updates (sub_step_progress fires many
  // times per second), this causes node positions to be recalculated from
  // scratch and the DAG to "jump" visually by a few pixels each time.
  //
  // The topology (nodes + edges + per-node height) only changes on a handful
  // of events (plan_generated, sub_plan_started, replan_triggered). Status
  // changes, duration updates, and sub-step progress do NOT change
  // topology — so we memoize layout positions by a topology hash and reuse
  // them when the hash matches.
  type PositionMap = Map<string, { x: number; y: number }>
  let layoutCache = new Map<string, PositionMap>()

  function topologyHash(
    nodesForHash: Array<{ id: string; type?: string; data?: unknown }>,
    edgesForHash: Array<{ source: string; target: string }>,
  ): string {
    const parts: string[] = []
    for (const n of nodesForHash) {
      const h = n.type === 'verifyGate' ? gateHeight() : nodeHeight(n.data as Record<string, unknown>)
      parts.push(`${n.id}:${h}`)
    }
    parts.sort()
    const edgeParts = edgesForHash.map(e => `${e.source}->${e.target}`).sort()
    return `N:${parts.join('|')}#E:${edgeParts.join('|')}`
  }

  // ── Sub-activity labels for the synthetic fallback ──────────
  //
  // These are only shown when a subagent has been running for 2.5s+
  // without ever emitting `sub_plan_started`. Using neutral placeholders
  // (not topic-specific guesses) prevents the user from mistaking them
  // for real data and wondering "wait, these names don't match what the
  // agent is supposed to do".
  function subLabelsFor(_stepName: string): string[] {
    return ['Working...', 'Working...', 'Working...']
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
      // Resolve sub-steps: event-driven takes priority (including after
      // completion, so the user keeps seeing the real names). Synthetic
      // fallback only when we never received any sub_plan_started event.
      let nodeSubSteps: Array<{ id: string; name: string; status: string }> | null = null
      const eventSub = subPlanState.get(step.id)
      if (eventSub) {
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
        data: { name: step.name, role: step.role, status: step.status, execution_mode: mode, verify_passed: step.verify_passed, duration_ms: step.duration_ms, tokens_used: step.tokens_used, teammateColor: tmColor, subSteps: nodeSubSteps, animPhaseMs: phaseFor(step.id) },
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

    // Topology hash: if identical to a previous build, reuse cached positions
    // instead of relaunching Dagre. This eliminates micro-jumps on every
    // sub_step_progress or status change.
    const hash = topologyHash(nodes, edges)
    const cached = layoutCache.get(hash)
    if (cached) {
      for (const n of nodes) {
        const pos = cached.get(n.id)
        if (pos) n.position = pos
      }
      return { nodes, edges }
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

    const positions: PositionMap = new Map()
    for (const n of nodes) {
      const pos = g.node(n.id)
      const w = n.type === 'verifyGate' ? GATE_SIZE : NODE_W
      const h = nodeHeights.get(n.id) ?? NODE_H_BASE
      const finalPos = { x: pos.x - w / 2, y: pos.y - h / 2 }
      n.position = finalPos
      positions.set(n.id, finalPos)
    }
    layoutCache.set(hash, positions)

    // Bound the cache so it doesn't grow unbounded across replans.
    if (layoutCache.size > 32) {
      const firstKey = layoutCache.keys().next().value
      if (firstKey !== undefined) layoutCache.delete(firstKey)
    }

    return { nodes, edges }
  }

  // ── Sub-activity expansion (dual mode: event-driven or synthetic) ──
  //
  // Priority: subPlanState prop (from events) > synthetic fallback (timer-based)
  // The synthetic mode activates only for subagent steps that have NO event-driven sub-plan.

  let syntheticStates = $state<Map<string, ('pending' | 'running' | 'passed')[]>>(new Map())
  /** Timers keyed by stepId so we can kill them atomically when switching modes. */
  let syntheticTimers = new Map<string, ReturnType<typeof setTimeout>[]>()
  /**
   * Step IDs that have ever received an event-driven sub-plan. Once a step
   * is in this set, we NEVER fall back to synthetic again — even if the
   * entry is momentarily missing from `subPlanState`. This eliminates the
   * race where a late `sub_plan_started` causes synthetic to fire first,
   * then event arrives, and the two render modes fight each other.
   */
  let eventDrivenStepIds = $state<Set<string>>(new Set())

  // Track subPlanState entries to lock a stepId into event-driven mode.
  $effect(() => {
    if (subPlanState.size === 0) return
    let changed = false
    const next = new Set(eventDrivenStepIds)
    for (const id of subPlanState.keys()) {
      if (!next.has(id)) { next.add(id); changed = true }
    }
    if (changed) eventDrivenStepIds = next
  })

  // Detect running subagents that need synthetic expansion (no event-driven sub-plan)
  let runningSubagentKey = $derived.by(() => {
    const deps = getDepsMap()
    const ids: string[] = []
    for (const step of steps) {
      const d = deps.get(step.id)
      if (d?.execution_mode === 'subagent' && step.status === 'running') {
        // Skip synthetic for any step that has ever been event-driven.
        if (!subPlanState.has(step.id) && !eventDrivenStepIds.has(step.id)) {
          ids.push(step.id)
        }
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

  /**
   * Grace window before activating the synthetic fallback.
   *
   * Synthetic is a last-resort fallback for subagent nodes that will
   * NEVER receive `sub_plan_started` events. In all normal cases the
   * event arrives within a second, so we wait long enough that synthetic
   * never fires during a real run. Only if the orchestrator is genuinely
   * silent for 2.5s+ do we show placeholder sub-steps — at that point
   * the user WANTS to see something moving.
   */
  const SYNTHETIC_GRACE_MS = 2500

  function expandSynthetic(stepId: string) {
    // Never start synthetic for a step that is (or was) event-driven.
    if (eventDrivenStepIds.has(stepId) || subPlanState.has(stepId)) return
    killSyntheticTimers(stepId)

    // Deferred start: re-check the event-driven condition when the grace
    // window expires. If a `sub_plan_started` arrived in the meantime,
    // skip synthetic entirely — the user never sees fake names.
    const stepTimers: ReturnType<typeof setTimeout>[] = []
    stepTimers.push(setTimeout(() => {
      if (eventDrivenStepIds.has(stepId) || subPlanState.has(stepId)) return
      const step = steps.find(s => s.id === stepId)
      const labels = subLabelsFor(step?.name ?? '')
      const states: ('pending' | 'running' | 'passed')[] = labels.map(() => 'pending')
      syntheticStates = new Map(syntheticStates).set(stepId, states)
      for (let i = 0; i < labels.length; i++) {
        stepTimers.push(setTimeout(() => {
          const c = syntheticStates.get(stepId); if (!c) return
          const n = [...c]; n[i] = 'running'
          syntheticStates = new Map(syntheticStates).set(stepId, n)
        }, 600 + i * 1800))
        stepTimers.push(setTimeout(() => {
          const c = syntheticStates.get(stepId); if (!c) return
          const n = [...c]; n[i] = 'passed'
          syntheticStates = new Map(syntheticStates).set(stepId, n)
        }, 1800 + i * 1800))
      }
    }, SYNTHETIC_GRACE_MS))
    syntheticTimers.set(stepId, stepTimers)
  }

  /**
   * Collapse synchronously — no `setTimeout`. Delaying the delete created a
   * window where event-driven and synthetic state both affected the render
   * and produced visual flicker. The disappearance is smoothed by CSS
   * transitions on the sub-strip.
   */
  function collapseSynthetic(stepId: string) {
    killSyntheticTimers(stepId)
    if (!syntheticStates.has(stepId)) return
    const next = new Map(syntheticStates)
    next.delete(stepId)
    syntheticStates = next
  }

  function killSyntheticTimers(stepId: string) {
    const timers = syntheticTimers.get(stepId)
    if (!timers) return
    for (const t of timers) clearTimeout(t)
    syntheticTimers.delete(stepId)
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

        // All in-flight messages between this pair, each with a stable id
        // so PulseEdge can key them independently. A flash that is already
        // mid-animation is NEVER re-mounted when another message arrives.
        type FlashHint = { id: string; color: string; reverse: boolean }
        const flashes: FlashHint[] = []
        for (const m of teammateMessages) {
          const forward = m.fromStepId === idA && m.toStepId === idB
          const reverse = m.fromStepId === idB && m.toStepId === idA
          if (!forward && !reverse) continue
          const tagged = m as typeof m & { _ts?: number }
          const flashId = tagged._ts !== undefined
            ? `${idA}-${idB}-${tagged._ts}`
            : `${idA}-${idB}-${m.summary}-${flashes.length}`
          flashes.push({
            id: flashId,
            color: teammateColorMap.get(m.fromStepId) ?? '#e8ecf4',
            reverse,
          })
        }

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
            flashes,
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
  // $state.raw per xyflow docs — avoids deep proxy overhead on frequent updates.
  let nodes = $state.raw<Node[]>([])
  let edges = $state.raw<Edge[]>([])

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

  // ── Stagger reveal (opacity only — never touch transform/position) ──

  function startReveal(g: { nodes: Node[]; edges: Edge[] }) {
    revealTimers.forEach(clearTimeout); revealTimers = []
    const batches = computeRevealBatches(g)
    const newRevealed = new Set<string>()
    g.nodes.filter(n => n.type === 'teamGroup').forEach(n => newRevealed.add(n.id))

    if (batches.length <= 1) {
      g.nodes.forEach(n => newRevealed.add(n.id))
      revealedIds = newRevealed
      nodes = g.nodes
      edges = g.edges
      return
    }

    // Hide unrevealed nodes with opacity only (no transform — xyflow owns that).
    nodes = g.nodes.map(n => newRevealed.has(n.id) ? n : { ...n, style: 'opacity: 0; pointer-events: none;' })
    edges = g.edges.map(e => newRevealed.has(e.source) && newRevealed.has(e.target) ? e : { ...e, style: (e.style ?? '') + ' opacity: 0;' })

    batches.forEach((batch, idx) => {
      revealTimers.push(setTimeout(() => {
        batch.forEach(id => newRevealed.add(id))
        revealedIds = new Set(newRevealed)
        nodes = g.nodes.map(n => newRevealed.has(n.id) ? n : { ...n, style: 'opacity: 0; pointer-events: none;' })
        edges = g.edges.map(e => newRevealed.has(e.source) && newRevealed.has(e.target) ? e : { ...e, style: (e.style ?? '') + ' opacity: 0;' })
      }, idx * 350))
    })
  }

  let lastStepCount = 0
  let initialized = false
  let previousNodeIds = new Set<string>()

  $effect(() => {
    const g = fullGraph
    if (g.nodes.length === 0) { nodes = []; edges = []; initialized = false; lastStepCount = 0; previousNodeIds.clear(); return }
    const mainCount = g.nodes.filter(n => n.type === 'dagNode' || n.type === 'verifyGate').length

    if (!initialized || Math.abs(mainCount - lastStepCount) > 2) {
      initialized = true
      lastStepCount = mainCount
      previousNodeIds = new Set(g.nodes.map(n => n.id))
      startReveal(g)
      return
    }
    lastStepCount = mainCount

    // Detect new nodes (replan adds them)
    const currentIds = new Set(g.nodes.map(n => n.id))
    const newIds = new Set<string>()
    for (const id of currentIds) {
      if (!previousNodeIds.has(id)) newIds.add(id)
    }
    previousNodeIds = currentIds

    // Stagger-reveal new nodes if any appeared (replan)
    if (newIds.size > 0) {
      const newRevealed = new Set(revealedIds)
      // Reveal new nodes with stagger
      const newOnly = g.nodes.filter(n => newIds.has(n.id))
      const batches = computeRevealBatches({ nodes: newOnly, edges: [] })
      batches.forEach((batch, idx) => {
        revealTimers.push(setTimeout(() => {
          batch.forEach(id => newRevealed.add(id))
          revealedIds = new Set(newRevealed)
          // Re-apply visibility
          applyVisibility(g)
        }, 200 + idx * 300))
      })
    }

    applyVisibility(g)
  })

  /** Apply opacity-based visibility without touching positions. */
  function applyVisibility(g: { nodes: Node[]; edges: Edge[] }) {
    const allRevealed = new Set(revealedIds)
    g.nodes.filter(n => n.type === 'teamGroup').forEach(n => allRevealed.add(n.id))

    nodes = g.nodes.map(n =>
      allRevealed.has(n.id) ? n : { ...n, style: 'opacity: 0; pointer-events: none;' }
    )
    edges = g.edges.map(e => {
      if (e.id.startsWith('e-sub-')) return e
      return allRevealed.has(e.source) && allRevealed.has(e.target)
        ? e
        : { ...e, style: (e.style ?? '') + ' opacity: 0;' }
    })
  }

  onMount(() => () => {
    revealTimers.forEach(clearTimeout)
    for (const timers of syntheticTimers.values()) timers.forEach(clearTimeout)
    syntheticTimers.clear()
  })

  function thoughtsFor(stepId: string): Thought[] { return thoughts.filter(t => t.stepId === stepId) }
  let activeStepId = $derived(steps.find(s => s.status === 'running')?.id ?? null)
  let hasSubagents = $derived(stepSummaries.some(s => s.execution_mode === 'subagent'))
  let hasTeammates = $derived(stepSummaries.some(s => s.execution_mode === 'teammate'))
  let hasResearch = $derived(steps.some(s => s.role.toLowerCase() === 'researcher' || s.role.toLowerCase() === 'analyst'))

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

    {#if hasSubagents || hasTeammates || hasResearch}
      <div class="legend">
        <span class="legend-item"><span class="legend-line legend-inline"></span><span>inline</span></span>
        {#if hasSubagents}
          <span class="legend-item"><span class="legend-line legend-fork"></span><span>fork (subagent)</span></span>
        {/if}
        {#if hasTeammates}
          <span class="legend-item"><span class="legend-line legend-team"></span><span>team</span></span>
          <span class="legend-item"><span class="legend-dot legend-comm"></span><span>comm</span></span>
        {/if}
        {#if hasResearch}
          <span class="legend-item"><span class="legend-dot legend-research"></span><span>prior art</span></span>
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
  /* Edge transitions: only opacity + stroke color. Never animate 'd' attribute
     (xyflow recalculates paths on every node position change — transitioning it lags). */
  :global(.svelte-flow__edge-path) {
    transition: opacity 0.5s ease, stroke 0.4s ease, stroke-width 0.3s ease;
  }
  /* Node transitions: opacity only. Never transition transform — xyflow sets
     transform:translate() inline and competing CSS transitions cause jank. */
  :global(.svelte-flow__node) { transition: opacity 0.5s cubic-bezier(0.4, 0, 0.2, 1); }
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
  .legend-research { background: #f97316; box-shadow: 0 0 4px #f9731660; }
  @keyframes legend-pulse { 0%, 100% { opacity: 0.5; } 50% { opacity: 1; } }
</style>
