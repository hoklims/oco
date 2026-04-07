import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { connectSSE, DASHBOARD_EVENT_TYPES, type SSEStatus } from './sse'
import type { DashboardEvent } from './types'

// ---------------------------------------------------------------------------
// EventSource mock
// ---------------------------------------------------------------------------

interface MockListener {
  (e: { data: string }): void
}

class MockEventSource {
  static instances: MockEventSource[] = []
  url: string
  onopen: (() => void) | null = null
  onerror: (() => void) | null = null
  private listeners = new Map<string, Set<MockListener>>()
  closed = false

  constructor(url: string) {
    this.url = url
    MockEventSource.instances.push(this)
  }

  addEventListener(type: string, listener: MockListener) {
    let set = this.listeners.get(type)
    if (!set) {
      set = new Set()
      this.listeners.set(type, set)
    }
    set.add(listener)
  }

  close() {
    this.closed = true
  }

  // Test helpers ────────────────────────────────
  fireOpen() {
    this.onopen?.()
  }

  fireError() {
    this.onerror?.()
  }

  fire(type: string, data: unknown) {
    const listeners = this.listeners.get(type)
    if (!listeners) return
    const payload = typeof data === 'string' ? data : JSON.stringify(data)
    for (const l of listeners) l({ data: payload })
  }

  static reset() {
    MockEventSource.instances = []
  }

  static latest(): MockEventSource {
    const last = MockEventSource.instances[MockEventSource.instances.length - 1]
    if (!last) throw new Error('no EventSource was created')
    return last
  }
}

// Install mock on global.
beforeEach(() => {
  MockEventSource.reset()
  ;(globalThis as unknown as { EventSource: typeof MockEventSource }).EventSource = MockEventSource
})

afterEach(() => {
  vi.useRealTimers()
})

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

function mkEvent(seq: number, type: string): DashboardEvent {
  return {
    schema_version: 1,
    seq,
    ts: new Date().toISOString(),
    session_id: 'session',
    run_id: 'run',
    plan_version: 0,
    kind: { type } as DashboardEvent['kind'],
  }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe('connectSSE', () => {
  it('dispatches every event type listed in DASHBOARD_EVENT_TYPES', () => {
    const received: DashboardEvent[] = []
    const client = connectSSE('/stream')
    client.onEvent((e) => received.push(e))

    const es = MockEventSource.latest()
    es.fireOpen()

    // Fire one event per declared type.
    DASHBOARD_EVENT_TYPES.forEach((type, i) => {
      es.fire(type, mkEvent(i + 1, type))
    })

    expect(received).toHaveLength(DASHBOARD_EVENT_TYPES.length)
    const types = received.map((e) => (e.kind as { type: string }).type)
    expect(types).toEqual([...DASHBOARD_EVENT_TYPES])

    client.close()
  })

  it('dispatches teammate/sub-plan events that were previously dropped (regression)', () => {
    const received: DashboardEvent[] = []
    const client = connectSSE('/stream')
    client.onEvent((e) => received.push(e))

    const es = MockEventSource.latest()
    es.fireOpen()

    const previouslyDropped = [
      'sub_plan_started', 'sub_step_progress', 'sub_plan_completed', 'teammate_message',
    ]
    previouslyDropped.forEach((type, i) => es.fire(type, mkEvent(i + 1, type)))

    expect(received.map((e) => (e.kind as { type: string }).type)).toEqual(previouslyDropped)
    client.close()
  })

  it('tracks lastSeq and uses it on reconnect via after_seq query param', () => {
    vi.useFakeTimers()
    const client = connectSSE('/stream', undefined, { reconnectDelayMs: 10 })
    const es1 = MockEventSource.latest()
    es1.fireOpen()

    es1.fire('progress', mkEvent(42, 'progress'))
    es1.fireError()

    // Reconnect is scheduled, advance the timer.
    vi.advanceTimersByTime(20)
    const es2 = MockEventSource.latest()
    expect(es2).not.toBe(es1)
    expect(es2.url).toBe('/stream?after_seq=42')

    client.close()
  })

  it('surfaces `desynced` status and reconnects immediately on lagged', () => {
    vi.useFakeTimers()
    const statuses: SSEStatus[] = []
    const client = connectSSE('/stream', undefined, { reconnectDelayMs: 1000 })
    client.onStatus((s) => statuses.push(s))

    const es1 = MockEventSource.latest()
    es1.fireOpen()
    es1.fire('step_completed', mkEvent(10, 'step_completed'))

    // Server broadcast overflowed.
    es1.fire('lagged', '{"skipped": 42}')

    expect(statuses).toContain('desynced')
    expect(es1.closed).toBe(true)

    // lagged reconnect uses delay=0, not reconnectDelayMs.
    vi.advanceTimersByTime(5)
    const es2 = MockEventSource.latest()
    expect(es2).not.toBe(es1)
    // Reconnect URL resumes from lastSeq=10.
    expect(es2.url).toBe('/stream?after_seq=10')

    client.close()
  })

  it('closes cleanly on `finished` and emits `closed` status', () => {
    const statuses: SSEStatus[] = []
    const client = connectSSE('/stream')
    client.onStatus((s) => statuses.push(s))

    const es = MockEventSource.latest()
    es.fireOpen()
    es.fire('finished', '{}')

    expect(es.closed).toBe(true)
    expect(statuses).toContain('closed')

    client.close()
  })

  it('close() cancels pending reconnect timers', () => {
    vi.useFakeTimers()
    const client = connectSSE('/stream', undefined, { reconnectDelayMs: 500 })
    const es = MockEventSource.latest()
    es.fireOpen()
    es.fireError()

    // A reconnect is now scheduled.
    client.close()

    // Let the timer fire — no new EventSource should be created.
    const before = MockEventSource.instances.length
    vi.advanceTimersByTime(1000)
    expect(MockEventSource.instances.length).toBe(before)
  })

  it('unsubscribe handlers correctly stops event delivery', () => {
    const received: DashboardEvent[] = []
    const client = connectSSE('/stream')
    const unsubscribe = client.onEvent((e) => received.push(e))

    const es = MockEventSource.latest()
    es.fire('progress', mkEvent(1, 'progress'))
    expect(received).toHaveLength(1)

    unsubscribe()
    es.fire('progress', mkEvent(2, 'progress'))
    expect(received).toHaveLength(1)

    client.close()
  })

  it('ignores malformed JSON payloads without breaking the stream', () => {
    const received: DashboardEvent[] = []
    const client = connectSSE('/stream')
    client.onEvent((e) => received.push(e))

    const es = MockEventSource.latest()
    es.fireOpen()

    // Garbage payload.
    es.fire('progress', 'not-json-at-all')
    // Valid payload afterward is still delivered.
    es.fire('progress', mkEvent(5, 'progress'))

    expect(received).toHaveLength(1)
    expect(received[0].seq).toBe(5)

    client.close()
  })

  it('connects with initial afterSeq cursor', () => {
    connectSSE('/stream', 99)
    const es = MockEventSource.latest()
    expect(es.url).toBe('/stream?after_seq=99')
  })

  it('applies exponential backoff on consecutive lagged events', () => {
    vi.useFakeTimers()
    const client = connectSSE('/stream', undefined, { reconnectDelayMs: 10_000 })

    // First lagged: reconnect immediate (delay=0).
    const es1 = MockEventSource.latest()
    es1.fireOpen()
    es1.fire('lagged', '{"skipped": 10}')
    vi.advanceTimersByTime(5)
    const es2 = MockEventSource.latest()
    expect(es2).not.toBe(es1)

    // Second consecutive lagged (no stable events in between): backoff = 500ms.
    es2.fireOpen()
    es2.fire('lagged', '{"skipped": 10}')
    vi.advanceTimersByTime(100)
    // Not yet reconnected at t=100ms.
    expect(MockEventSource.instances.length).toBe(2)
    vi.advanceTimersByTime(500)
    const es3 = MockEventSource.latest()
    expect(es3).not.toBe(es2)

    // Third consecutive: backoff = 1000ms.
    es3.fireOpen()
    es3.fire('lagged', '{"skipped": 10}')
    vi.advanceTimersByTime(500)
    expect(MockEventSource.instances.length).toBe(3)
    vi.advanceTimersByTime(600)
    const es4 = MockEventSource.latest()
    expect(es4).not.toBe(es3)

    client.close()
  })

  it('resets lagged streak after stable event delivery', () => {
    vi.useFakeTimers()
    const client = connectSSE('/stream', undefined, { reconnectDelayMs: 10_000 })

    // One lagged to bump the streak.
    const es1 = MockEventSource.latest()
    es1.fireOpen()
    es1.fire('lagged', '{"skipped": 1}')
    vi.advanceTimersByTime(5)

    // Deliver >=3 events to mark the connection "stable".
    const es2 = MockEventSource.latest()
    es2.fireOpen()
    es2.fire('progress', mkEvent(1, 'progress'))
    es2.fire('progress', mkEvent(2, 'progress'))
    es2.fire('progress', mkEvent(3, 'progress'))

    // Now a new lagged should reset to delay=0 (streak=1).
    es2.fire('lagged', '{"skipped": 2}')
    vi.advanceTimersByTime(5)
    const es3 = MockEventSource.latest()
    expect(es3).not.toBe(es2)

    client.close()
  })
})
