import type { DashboardEvent } from './types'

export type SSEStatus = 'connecting' | 'connected' | 'reconnecting' | 'closed'

export interface SSEClient {
  /** Subscribe to events. Returns unsubscribe function. */
  onEvent: (handler: (event: DashboardEvent) => void) => () => void
  /** Subscribe to status changes. */
  onStatus: (handler: (status: SSEStatus) => void) => () => void
  /** Close the connection. */
  close: () => void
}

/**
 * Connect to an SSE stream with auto-reconnect and cursor-based resumption.
 *
 * Uses `Last-Event-ID` for reconnect via `after_seq` query param
 * (native EventSource doesn't support custom headers reliably).
 */
export function connectSSE(baseUrl: string, afterSeq?: number): SSEClient {
  let lastSeq = afterSeq ?? 0
  let eventSource: EventSource | null = null
  let closed = false

  const eventHandlers = new Set<(event: DashboardEvent) => void>()
  const statusHandlers = new Set<(status: SSEStatus) => void>()

  function emitStatus(status: SSEStatus) {
    for (const handler of statusHandlers) handler(status)
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
    const eventTypes = [
      'run_started', 'run_stopped', 'plan_exploration', 'plan_generated',
      'step_started', 'step_completed', 'flat_step_completed',
      'progress', 'verify_gate_result', 'replan_triggered',
      'budget_warning', 'budget_snapshot', 'index_progress',
      'heartbeat', 'finished', 'lagged',
    ]

    for (const type of eventTypes) {
      eventSource.addEventListener(type, (e: MessageEvent) => {
        try {
          const parsed: DashboardEvent = JSON.parse(e.data)
          if (parsed.seq) lastSeq = parsed.seq
          for (const handler of eventHandlers) handler(parsed)
        } catch {
          // Ignore parse errors (e.g. lagged/finished are not DashboardEvent)
        }
      })
    }

    eventSource.onerror = () => {
      eventSource?.close()
      if (!closed) {
        emitStatus('reconnecting')
        setTimeout(connect, 2000)
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
      eventSource?.close()
      emitStatus('closed')
    },
  }
}
