/**
 * Graph Animator — organic spring-based position interpolation for xyflow nodes.
 *
 * Each node gets its own spring simulation. When the layout changes (replan,
 * new nodes added), target positions update and nodes glide organically to
 * their new spots via requestAnimationFrame.
 *
 * Design: no external dependencies — uses a minimal critically-damped spring
 * implementation inspired by svelte/motion internals. This avoids importing
 * Svelte's Spring class (which requires runes context) into a plain TS module.
 */

import type { Node } from '@xyflow/svelte'

// ── Spring physics ──────────────────────────────────────────

interface SpringState {
  x: number
  y: number
  vx: number
  vy: number
  tx: number // target x
  ty: number // target y
}

/** Spring parameters — low stiffness + high damping = organic, slow settle. */
const STIFFNESS = 0.04
const DAMPING = 0.72
/** Below this threshold (px), snap to target. */
const EPSILON = 0.3

function stepSpring(s: SpringState, dt: number): boolean {
  const dx = s.tx - s.x
  const dy = s.ty - s.y

  // Spring force + damping
  s.vx = s.vx * (1 - DAMPING) + dx * STIFFNESS * dt
  s.vy = s.vy * (1 - DAMPING) + dy * STIFFNESS * dt
  s.x += s.vx * dt
  s.y += s.vy * dt

  // Check convergence
  const moving = Math.abs(dx) > EPSILON || Math.abs(dy) > EPSILON ||
    Math.abs(s.vx) > EPSILON * 0.1 || Math.abs(s.vy) > EPSILON * 0.1
  if (!moving) {
    s.x = s.tx
    s.y = s.ty
    s.vx = 0
    s.vy = 0
  }
  return moving
}

// ── Animator ────────────────────────────────────────────────

export interface GraphAnimator {
  /**
   * Set new target positions for nodes. New node IDs get a delayed entrance
   * (stagger by DAG depth). Returns immediately; positions update via rAF.
   */
  setTargets: (
    targetNodes: Node[],
    onFrame: (animated: Node[]) => void,
    depthMap?: Map<string, number>,
  ) => void

  /** Remove a node from tracking (on delete). */
  remove: (id: string) => void

  /** Stop all animations and clean up. */
  destroy: () => void
}

export function createGraphAnimator(): GraphAnimator {
  const springs = new Map<string, SpringState>()
  let rafId: number | null = null
  let destroyed = false
  let currentNodes: Node[] = []
  let frameCallback: ((nodes: Node[]) => void) | null = null
  /** Nodes waiting for stagger delay before becoming visible. */
  const pendingReveal = new Map<string, ReturnType<typeof setTimeout>>()

  function tick() {
    if (destroyed) return

    let anyMoving = false
    for (const s of springs.values()) {
      if (stepSpring(s, 1)) anyMoving = true
    }

    // Build frame output — apply spring positions to nodes
    const frame = currentNodes.map(n => {
      const s = springs.get(n.id)
      if (!s) return n
      return { ...n, position: { x: s.x, y: s.y } }
    })

    frameCallback?.(frame)

    if (anyMoving) {
      rafId = requestAnimationFrame(tick)
    } else {
      rafId = null
    }
  }

  function ensureRunning() {
    if (rafId === null && !destroyed) {
      rafId = requestAnimationFrame(tick)
    }
  }

  return {
    setTargets(targetNodes, onFrame, depthMap) {
      frameCallback = onFrame
      const previousIds = new Set(springs.keys())
      const incomingIds = new Set(targetNodes.map(n => n.id))

      // Update or create springs for each target node
      for (const node of targetNodes) {
        const existing = springs.get(node.id)

        if (existing) {
          // Existing node — update target (spring will animate there)
          existing.tx = node.position.x
          existing.ty = node.position.y
        } else {
          // New node — determine if it should stagger
          const depth = depthMap?.get(node.id) ?? 0
          const delay = depth * 100 // 100ms per DAG level

          if (delay > 0) {
            // Start at target position but invisible; PlanMap handles opacity
            springs.set(node.id, {
              x: node.position.x,
              y: node.position.y,
              vx: 0, vy: 0,
              tx: node.position.x,
              ty: node.position.y,
            })
          } else {
            springs.set(node.id, {
              x: node.position.x,
              y: node.position.y,
              vx: 0, vy: 0,
              tx: node.position.x,
              ty: node.position.y,
            })
          }
        }
      }

      // Clean up springs for removed nodes
      for (const id of previousIds) {
        if (!incomingIds.has(id)) {
          springs.delete(id)
        }
      }

      currentNodes = targetNodes
      ensureRunning()
    },

    remove(id) {
      springs.delete(id)
      const timer = pendingReveal.get(id)
      if (timer) { clearTimeout(timer); pendingReveal.delete(id) }
    },

    destroy() {
      destroyed = true
      if (rafId !== null) cancelAnimationFrame(rafId)
      for (const timer of pendingReveal.values()) clearTimeout(timer)
      pendingReveal.clear()
      springs.clear()
    },
  }
}
