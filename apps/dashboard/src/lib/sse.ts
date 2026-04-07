import type { DashboardEvent } from './types'

export type SSEStatus = 'connecting' | 'connected' | 'reconnecting' | 'desynced' | 'closed'

export interface SSEOptions {
  /** Base delay for error-based reconnects (ms). Default: 2000. */
  reconnectDelayMs?: number
}

export interface SSEClient {
  /** Subscribe to events. Returns unsubscribe function. */
  onEvent: (handler: (event: DashboardEvent) => void) => () => void
  /** Subscribe to status changes. */
  onStatus: (handler: (status: SSEStatus) => void) => () => void
  /** Close the connection. */
  close: () => void
}

/** All event types registered on the SSE EventSource. */
export const DASHBOARD_EVENT_TYPES = [
  'run_started', 'run_stopped', 'plan_exploration', 'plan_generated',
  'step_started', 'step_completed', 'flat_step_completed',
  'progress', 'verify_gate_result', 'replan_triggered',
  'budget_warning', 'budget_snapshot', 'index_progress',
  'sub_plan_started', 'sub_step_progress', 'sub_plan_completed',
  'teammate_message', 'teammate_idle',
  'heartbeat',
] as const

/** Number of stable events needed to reset the lagged backoff streak. */
const STABLE_THRESHOLD = 3

/**
 * Connect to an SSE stream with auto-reconnect and cursor-based resumption.
 *
 * Uses `Last-Event-ID` for reconnect via `after_seq` query param
 * (native EventSource doesn't support custom headers reliably).
 */
export function connectSSE(baseUrl: string, afterSeq?: number, options?: SSEOptions): SSEClient {
  const reconnectDelayMs = options?.reconnectDelayMs ?? 2000
  let lastSeq = afterSeq ?? 0
  let eventSource: EventSource | null = null
  let closed = false
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null

  // Lagged backoff state
  let laggedStreak = 0
  let stableCount = 0

  const eventHandlers = new Set<(event: DashboardEvent) => void>()
  const statusHandlers = new Set<(status: SSEStatus) => void>()

  function emitStatus(status: SSEStatus) {
    for (const handler of statusHandlers) handler(status)
  }

  function scheduleReconnect(delayMs: number) {
    reconnectTimer = setTimeout(connect, delayMs)
  }

  function connect() {
    if (closed) return

    const url = lastSeq > 0 ? `${baseUrl}?after_seq=${lastSeq}` : baseUrl
    emitStatus(lastSeq > 0 ? 'reconnecting' : 'connecting')

    eventSource = new EventSource(url)

    eventSource.onopen = () => {
      emitStatus('connected')
    }

    // Listen to all named event types.
    for (const type of DASHBOARD_EVENT_TYPES) {
      eventSource.addEventListener(type, (e: MessageEvent) => {
        try {
          const parsed: DashboardEvent = JSON.parse(e.data)
          if (parsed.seq) lastSeq = parsed.seq
          stableCount++
          if (stableCount >= STABLE_THRESHOLD) {
            laggedStreak = 0
            stableCount = 0
          }
          for (const handler of eventHandlers) handler(parsed)
        } catch {
          // Ignore parse errors
        }
      })
    }

    // Handle 'lagged' — broadcast buffer overflow, need to resync.
    eventSource.addEventListener('lagged', () => {
      laggedStreak++
      stableCount = 0
      emitStatus('desynced')
      eventSource?.close()

      if (!closed) {
        // First lagged: immediate reconnect. Subsequent: exponential backoff.
        const backoff = laggedStreak <= 1 ? 0 : (laggedStreak - 1) * 500
        scheduleReconnect(backoff)
      }
    })

    // Handle 'finished' — stream is complete, close cleanly.
    eventSource.addEventListener('finished', () => {
      eventSource?.close()
      emitStatus('closed')
    })

    eventSource.onerror = () => {
      eventSource?.close()
      if (!closed) {
        emitStatus('reconnecting')
        scheduleReconnect(reconnectDelayMs)
      }
    }
  }

  connect()

  return {
    onEvent(handler) {
      eventHandlers.add(handler)
      return () => eventHandlers.delete(handler)
    },
    onStatus(handler) {
      statusHandlers.add(handler)
      return () => statusHandlers.delete(handler)
    },
    close() {
      closed = true
      if (reconnectTimer) clearTimeout(reconnectTimer)
      eventSource?.close()
      emitStatus('closed')
    },
  }
}
