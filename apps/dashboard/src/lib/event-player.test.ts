import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { createEventPlayer, type EventPlayer } from './event-player'
import type { DashboardEvent, DashboardEventKind } from './types'

// Minimal event factory — we only care about `seq` and `kind.type` in tests.
function event(seq: number, kind: DashboardEventKind): DashboardEvent {
  return {
    schema_version: 1,
    seq,
    ts: new Date().toISOString(),
    session_id: 'session',
    run_id: 'run',
    plan_version: 0,
    kind,
  }
}

function progress(seq: number, completed = 1): DashboardEvent {
  return event(seq, {
    type: 'progress',
    completed,
    total: 10,
    active_steps: [],
    budget: {
      tokens_used: 0,
      tokens_remaining: 0,
      tool_calls_used: 0,
      tool_calls_remaining: 0,
      retrievals_used: 0,
      verify_cycles_used: 0,
      elapsed_secs: 0,
    },
  })
}

function step(seq: number, id: string, type: 'started' | 'completed'): DashboardEvent {
  if (type === 'started') {
    return event(seq, {
      type: 'step_started',
      step_id: id,
      step_name: id,
      role: 'implementer',
      execution_mode: 'inline',
    })
  }
  return event(seq, {
    type: 'step_completed',
    step_id: id,
    step_name: id,
    success: true,
    duration_ms: 100,
    tokens_used: 0,
    detail_ref: null,
  })
}

describe('EventPlayer', () => {
  let received: DashboardEvent[]
  let player: EventPlayer

  beforeEach(() => {
    vi.useFakeTimers()
    received = []
    player = createEventPlayer({
      onEvent: (e) => received.push(e),
    })
  })

  afterEach(() => {
    player.stop()
    vi.useRealTimers()
  })

  describe('coalescing', () => {
    it('replaces consecutive progress events in the buffer', () => {
      player.push(progress(1, 1))
      player.push(progress(2, 2))
      player.push(progress(3, 3))

      // Only one entry in the buffer because progress events coalesce.
      expect(player.buffered()).toBe(1)
    })

    it('does not coalesce across a non-coalescable event', () => {
      player.push(progress(1, 1))
      player.push(step(2, 'a', 'started'))
      player.push(progress(3, 2))

      // [progress_latest_1, step_started, progress_3] — the two progress
      // events are separated by a step, so neither coalesces away.
      expect(player.buffered()).toBeGreaterThanOrEqual(2)
    })

    it('coalesces budget_snapshot but NOT index_progress', () => {
      // budget_snapshot is coalescable — a stale value adds no info.
      player.push(event(1, {
        type: 'budget_snapshot',
        tokens_used: 100,
        tokens_remaining: 900,
        tool_calls_used: 0,
        tool_calls_remaining: 0,
        retrievals_used: 0,
        verify_cycles_used: 0,
        elapsed_secs: 0,
      } as DashboardEventKind))
      player.push(event(2, {
        type: 'budget_snapshot',
        tokens_used: 200,
        tokens_remaining: 800,
        tool_calls_used: 0,
        tool_calls_remaining: 0,
        retrievals_used: 0,
        verify_cycles_used: 0,
        elapsed_secs: 0,
      } as DashboardEventKind))
      expect(player.buffered()).toBe(1)

      // index_progress drives the IndexingScene counters — each event
      // ticks the scan display forward, so coalescing would swallow the
      // intermediate steps. It must NOT coalesce.
      player.push(event(3, {
        type: 'index_progress',
        files_done: 1,
        symbols_so_far: 10,
      }))
      player.push(event(4, {
        type: 'index_progress',
        files_done: 2,
        symbols_so_far: 20,
      }))
      // budget_snapshot + 2 × index_progress = 3 entries
      expect(player.buffered()).toBe(3)
    })
  })

  describe('buffer cap', () => {
    it('drops non-phase events when exceeding MAX_BUFFER_SIZE', () => {
      // Push >500 non-coalescable, non-phase events.
      // step_started/step_completed are non-phase and non-coalescable.
      for (let i = 0; i < 600; i++) {
        player.push(step(i + 1, `s${i}`, i % 2 === 0 ? 'started' : 'completed'))
      }

      expect(player.buffered()).toBeLessThanOrEqual(500)
    })

    it('falls back to dropping phase events when the buffer is entirely phase-events', () => {
      // Pathological: push >500 phase events only. The fallback branch
      // must kick in — otherwise the buffer would grow unbounded.
      for (let i = 0; i < 600; i++) {
        player.push(event(i + 1, {
          type: 'run_stopped',
          reason: { type: 'task_complete' },
          total_steps: 0,
          total_tokens: 0,
        } as DashboardEventKind))
      }

      expect(player.buffered()).toBeLessThanOrEqual(500)
    })
  })

  describe('stop()', () => {
    it('clears the buffer and prevents further dispatches', () => {
      player.push(step(1, 'a', 'started'))
      player.push(step(2, 'b', 'started'))
      player.push(step(3, 'c', 'started'))

      player.stop()
      expect(player.buffered()).toBe(0)

      // After stop, pushing more events is a no-op.
      player.push(step(4, 'd', 'started'))
      expect(player.buffered()).toBe(0)
    })

    it('cancels pending timers so no late callbacks fire', () => {
      let callbackCount = 0
      const p = createEventPlayer({
        onEvent: () => { callbackCount += 1 },
      })
      p.push(step(1, 'a', 'started'))
      p.push(step(2, 'b', 'started'))
      p.push(step(3, 'c', 'started'))

      // Advance past the pre-pause but stop before the first dispatch.
      vi.advanceTimersByTime(100)
      p.stop()

      // Flush all remaining timers. No dispatch should happen after stop.
      const beforeFlush = callbackCount
      vi.runAllTimers()
      expect(callbackCount).toBe(beforeFlush)
    })

    it('cancels exploration sub-timers when stopped mid-sequence', () => {
      const phases: string[] = []
      const p = createEventPlayer({
        onEvent: () => {},
        onExploration: (phase) => phases.push(phase),
      })
      // Exploration animation is triggered by plan_generated (not plan_exploration).
      p.push(event(1, {
        type: 'plan_generated',
        plan_id: '00000000-0000-0000-0000-000000000000',
        step_count: 3,
        parallel_group_count: 1,
        critical_path_length: 3,
        estimated_total_tokens: 1000,
        strategy: 'safety',
        team: null,
        steps: [],
        candidates: [
          { strategy: 'a', step_count: 3, estimated_tokens: 1000, score: 0.8 },
          { strategy: 'b', step_count: 3, estimated_tokens: 1100, score: 0.7 },
        ],
      } as DashboardEventKind))

      // Let the exploration sequence start (first schedule fires 'generating').
      vi.advanceTimersByTime(700)
      const phasesBeforeStop = phases.length
      expect(phasesBeforeStop).toBeGreaterThan(0)

      p.stop()

      // Flush all remaining timers. After stop, NO new phases should fire.
      vi.runAllTimers()
      expect(phases.length).toBe(phasesBeforeStop)
    })
  })

  describe('playback', () => {
    it('dispatches non-phase events in order', () => {
      player.push(step(1, 'a', 'started'))
      player.push(step(2, 'a', 'completed'))

      vi.runAllTimers()

      expect(received.length).toBe(2)
      expect(received[0].seq).toBe(1)
      expect(received[1].seq).toBe(2)
    })

    it('skips heartbeats without delay', () => {
      player.push(event(1, { type: 'heartbeat' }))
      player.push(step(2, 'a', 'started'))

      vi.runAllTimers()
      expect(received.length).toBe(1)
      expect(received[0].seq).toBe(2)
    })
  })
})
