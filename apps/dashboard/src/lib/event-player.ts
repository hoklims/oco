/**
 * EventPlayer — choreographed playback of dashboard events.
 *
 * Events arrive from SSE into a buffer. The player dequeues them one at a time,
 * waits for each event's animation to complete, then moves to the next.
 * This creates the "always one step behind" effect where users have time
 * to admire each phase with its full animation.
 *
 * Architecture:
 *   SSE → buffer (immediate) → player (paced) → onEvent callback → Svelte state
 *
 * Each event type has a defined choreography duration. The player adds
 * organic timing variation (±10%) to avoid robotic regularity.
 */

import type { DashboardEvent } from './types'
import type { Thought } from './demo'

export type ExplorationPhase = 'idle' | 'generating' | 'comparing' | 'scoring' | 'selecting' | 'done'

/** Animation duration per event type (ms). */
const CHOREOGRAPHY: Record<string, number> = {
  run_started:         800,    // Header appears, mission text fades in
  plan_exploration:    0,      // DYNAMIC — calculated from candidate complexity
  plan_generated:      2000,   // PlanMap nodes stagger-reveal
  step_started:        600,    // Node glow activates
  step_completed:      1000,   // Node completes, checkmark, edge draws
  flat_step_completed: 500,    // Stepper phase update
  progress:            400,    // Budget bar interpolation
  verify_gate_result:  1200,   // Verification badge reveal
  replan_triggered:    1500,   // Replan warning animation
  budget_warning:      800,    // Warning pulse
  run_stopped:         1200,   // Final celebration or error
  heartbeat:           0,      // Skip
  index_progress:      0,      // Skip
  budget_snapshot:     200,    // Quick budget update
}

/**
 * Calculate dynamic exploration duration from plan_exploration event data.
 *
 * Formula: node_reveal + comparing_pause + scoring_pause + suspense + exit
 *   - node_reveal: max(steps) * 700ms per batch (stagger)
 *   - comparing: 2s to let user see both branches
 *   - scoring: 2s for stats reveal + merge node pulse
 *   - suspense: 1.5s amber pulse before winner reveal
 *   - selection: 1.5s winner glow + loser fade
 *   - exit: 1.2s smooth fade-out
 *
 * Simple plan (3+3 steps): ~10s
 * Medium plan (5+3 steps): ~13s
 * Complex plan (7+5 steps): ~16s
 */
function explorationDuration(kind: Record<string, unknown>): {
  total: number
  comparingAt: number
  scoringAt: number
  selectingAt: number
  doneAt: number
} {
  const candidates = (kind.candidates as Array<Record<string, unknown>>) ?? []
  const maxSteps = Math.max(...candidates.map(c => (c.step_count as number) ?? 3), 3)

  // Phase durations
  const revealDuration = (maxSteps + 2) * 700  // +2 for origin + merge nodes
  const comparingPause = 2000
  const scoringPause = 2000
  const suspensePause = 1500
  const selectionReveal = 1500
  const exitDuration = 1200

  const comparingAt = revealDuration
  const scoringAt = comparingAt + comparingPause
  const selectingAt = scoringAt + scoringPause + suspensePause
  const doneAt = selectingAt + selectionReveal + exitDuration
  const total = doneAt

  return { total, comparingAt, scoringAt, selectingAt, doneAt }
}

/** Default duration for unknown event types. */
const DEFAULT_DURATION = 500

/** Add organic variation to timing (±10%). */
function jitter(ms: number): number {
  if (ms === 0) return 0
  const variation = 0.1
  const factor = 1 + (Math.random() * 2 - 1) * variation
  return Math.round(ms * factor)
}

/** Pause between events within the same phase. */
const INTER_EVENT_PAUSE = 150

/** Extra pause before a phase-changing event (plan_generated, run_stopped). */
const PHASE_TRANSITION_PAUSE = 600

/** Events that mark major phase transitions — get extra pause before them. */
const PHASE_EVENTS = new Set([
  'plan_exploration', 'plan_generated', 'run_stopped',
])

export interface EventPlayerCallbacks {
  /** Called when an event should be applied to state. */
  onEvent: (event: DashboardEvent) => void
  /** Called for exploration phase transitions (from plan_exploration events). */
  onExploration?: (phase: ExplorationPhase) => void
  /** Called for thought bubbles (generated from step events). */
  onThought?: (thought: Thought) => void
}

export interface EventPlayer {
  /** Push a new event into the buffer (called by SSE handler). */
  push: (event: DashboardEvent) => void
  /** Push a batch of events. */
  pushBatch: (events: DashboardEvent[]) => void
  /** Stop playback and clear the queue. */
  stop: () => void
  /** Number of events waiting in the buffer. */
  buffered: () => number
}

export function createEventPlayer(callbacks: EventPlayerCallbacks): EventPlayer {
  const buffer: DashboardEvent[] = []
  let playing = false
  let stopped = false
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  let dynamicDuration = 0 // Set by plan_exploration for dynamic timing

  function scheduleNext() {
    if (stopped || playing) return
    if (buffer.length === 0) return

    playing = true
    const event = buffer.shift()!
    const kind = event.kind as Record<string, unknown>
    const eventType = kind.type as string

    // Skip heartbeats and index_progress
    if (eventType === 'heartbeat' || eventType === 'index_progress') {
      playing = false
      scheduleNext()
      return
    }

    // Extra pause before phase transitions
    const prePause = PHASE_EVENTS.has(eventType) ? PHASE_TRANSITION_PAUSE : 0

    timeoutId = setTimeout(() => {
      if (stopped) return

      // Handle plan_exploration specially — trigger the PlanExplorer animation sequence
      // Timing is DYNAMIC based on plan complexity (more steps = longer animation)
      if (eventType === 'plan_exploration' && callbacks.onExploration) {
        const timing = explorationDuration(kind)
        callbacks.onExploration('generating')
        setTimeout(() => { if (!stopped) callbacks.onExploration?.('comparing') }, timing.comparingAt)
        setTimeout(() => { if (!stopped) callbacks.onExploration?.('scoring') }, timing.scoringAt)
        setTimeout(() => { if (!stopped) callbacks.onExploration?.('selecting') }, timing.selectingAt)
        setTimeout(() => { if (!stopped) callbacks.onExploration?.('done') }, timing.doneAt)
        // Override the fixed choreography duration with the dynamic one
        dynamicDuration = timing.total
      }

      // Emit the event to state
      callbacks.onEvent(event)

      // Generate synthetic thought for step_started events
      if (eventType === 'step_started' && callbacks.onThought) {
        const stepName = kind.step_name as string
        callbacks.onThought({
          text: `Working on: ${stepName}`,
          variant: 'action',
          stepId: kind.step_id as string,
          offsetMs: 0,
        })
      }

      // Generate thought for step_completed
      if (eventType === 'step_completed' && callbacks.onThought) {
        const success = kind.success as boolean
        const durationMs = kind.duration_ms as number
        const stepName = kind.step_name as string
        const durationSec = durationMs ? `${(durationMs / 1000).toFixed(1)}s` : ''
        callbacks.onThought({
          text: success ? `${stepName} done ${durationSec}` : `${stepName} failed`,
          variant: success ? 'success' : 'warning',
          stepId: kind.step_id as string,
          offsetMs: 0,
        })
      }

      // Generate thought for verify_gate_result
      if (eventType === 'verify_gate_result' && callbacks.onThought) {
        const passed = kind.overall_passed as boolean
        const checks = (kind.checks as Array<Record<string, unknown>>) || []
        const summary = checks.map(c => `${c.check_type}: ${c.passed ? 'pass' : 'FAIL'}`).join(', ')
        callbacks.onThought({
          text: passed ? `Verified: ${summary}` : `Verification failed: ${summary}`,
          variant: passed ? 'success' : 'warning',
          stepId: kind.step_id as string,
          offsetMs: 0,
        })
      }

      // Wait for animation duration, then dequeue next
      // Use dynamic duration for plan_exploration, fixed for everything else
      const baseDuration = dynamicDuration > 0 ? dynamicDuration : (CHOREOGRAPHY[eventType] ?? DEFAULT_DURATION)
      dynamicDuration = 0 // Reset for next event
      const duration = jitter(baseDuration)
      const pause = jitter(INTER_EVENT_PAUSE)

      timeoutId = setTimeout(() => {
        playing = false
        scheduleNext()
      }, duration + pause)

    }, prePause)
  }

  return {
    push(event: DashboardEvent) {
      if (stopped) return
      buffer.push(event)
      scheduleNext()
    },

    pushBatch(events: DashboardEvent[]) {
      if (stopped) return
      buffer.push(...events)
      scheduleNext()
    },

    stop() {
      stopped = true
      if (timeoutId) clearTimeout(timeoutId)
      buffer.length = 0
    },

    buffered() {
      return buffer.length
    },
  }
}
