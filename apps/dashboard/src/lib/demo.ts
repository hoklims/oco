/**
 * Demo scenario — simulates a real competitive planning run.
 * 2 candidate plans (Speed vs Safety) with structural differences,
 * then execution of the winner.
 */
import type { DashboardEvent, BudgetSnapshot } from './types'

function ts(offsetMs: number): string {
  return new Date(Date.now() + offsetMs).toISOString()
}

const budgetAt = (tokUsed: number, toolUsed: number, retr: number, verify: number, elapsed: number): BudgetSnapshot => ({
  tokens_used: tokUsed,
  tokens_remaining: 50000 - tokUsed,
  tool_calls_used: toolUsed,
  tool_calls_remaining: 30 - toolUsed,
  retrievals_used: retr,
  verify_cycles_used: verify,
  elapsed_secs: elapsed,
})

const sid = '00000000-0000-0000-0000-000000000000'
const rid = '11111111-1111-1111-1111-111111111111'
const stepIds = [
  'aaaa0001-0000-0000-0000-000000000000',
  'aaaa0002-0000-0000-0000-000000000000',
  'aaaa0003-0000-0000-0000-000000000000',
  'aaaa0004-0000-0000-0000-000000000000',
  'aaaa0005-0000-0000-0000-000000000000',
]

function evt(seq: number, offsetMs: number, planVersion: number, kind: Record<string, unknown>): DashboardEvent {
  return {
    schema_version: 1, seq, ts: ts(offsetMs),
    session_id: sid, run_id: rid, plan_version: planVersion,
    kind: kind as DashboardEvent['kind'],
  }
}

/** The 2 real candidate plans for the exploration visualization. */
export interface CompetitivePlan {
  strategy: string
  steps: Array<{ name: string; role: string; verify: boolean; tokens: number; depends_on: string[] }>
  score: number
  winner: boolean
  // Scoring breakdown
  scores: { verify: number; cost: number; parallel: number; depth: number }
}

export const DEMO_PLAN_SPEED: CompetitivePlan = {
  strategy: 'speed',
  steps: [
    { name: 'Analyze & design', role: 'architect', verify: false, tokens: 5000, depends_on: [] },
    { name: 'Implement JWT + refresh', role: 'implementer', verify: true, tokens: 15000, depends_on: ['Analyze & design'] },
    { name: 'Quick smoke test', role: 'tester', verify: true, tokens: 4000, depends_on: ['Implement JWT + refresh'] },
  ],
  score: 0.62,
  winner: false,
  scores: { verify: 0.67, cost: 0.52, parallel: 0.0, depth: 0.0 },
}

export const DEMO_PLAN_SAFETY: CompetitivePlan = {
  strategy: 'safety',
  steps: [
    { name: 'Analyze current auth', role: 'scout', verify: false, tokens: 3000, depends_on: [] },
    { name: 'Design JWT schema', role: 'architect', verify: false, tokens: 5000, depends_on: ['Analyze current auth'] },
    { name: 'Implement middleware', role: 'implementer', verify: true, tokens: 12000, depends_on: ['Design JWT schema'] },
    { name: 'Implement refresh', role: 'implementer', verify: true, tokens: 8000, depends_on: ['Design JWT schema'] },
    { name: 'Integration tests', role: 'tester', verify: true, tokens: 7000, depends_on: ['Implement middleware', 'Implement refresh'] },
  ],
  score: 0.78,
  winner: true,
  scores: { verify: 0.60, cost: 0.30, parallel: 0.40, depth: 0.20 },
}

/** Exploration phase events */
export interface ExplorationPhase {
  phase: 'idle' | 'generating' | 'comparing' | 'scoring' | 'selecting' | 'done'
  offsetMs: number
}

export const DEMO_EXPLORATION: ExplorationPhase[] = [
  { phase: 'generating', offsetMs: 200 },    // Plans appearing
  { phase: 'comparing', offsetMs: 3000 },     // Structural comparison
  { phase: 'scoring', offsetMs: 5000 },        // Score bars appear
  { phase: 'selecting', offsetMs: 6500 },      // Winner highlighted
  { phase: 'done', offsetMs: 8000 },           // Transition to execution
]

// Exploration duration before normal events start
const EXPLORATION_DURATION = 8500

export const DEMO_EVENTS: DashboardEvent[] = [
  evt(1, 0, 0, {
    type: 'run_started',
    provider: 'claude-code', model: 'sonnet',
    request_summary: 'Refactor the auth module to use JWT tokens with refresh flow',
  }),

  // Plan exploration event (real data from competitive planning)
  evt(2, 200, 0, {
    type: 'plan_exploration',
    candidates: [
      { strategy: 'speed', step_count: 3, estimated_tokens: 24000, verify_count: 2, parallel_groups: 3, score: 0.62, winner: false },
      { strategy: 'safety', step_count: 5, estimated_tokens: 35000, verify_count: 3, parallel_groups: 3, score: 0.78, winner: true },
    ],
    winner_strategy: 'safety',
    winner_score: 0.78,
  }),

  evt(3, 8000, 1, {
    type: 'plan_generated',
    plan_id: 'plan-0001', step_count: 5, parallel_group_count: 3,
    critical_path_length: 4, estimated_total_tokens: 35000,
    strategy: 'Competitive (safety won)', team: null,
    steps: [
      { id: stepIds[0], name: 'Analyze current auth', description: '', role: 'scout', execution_mode: 'inline', depends_on: [], verify_after: false, estimated_tokens: 3000, preferred_model: null },
      { id: stepIds[1], name: 'Design JWT schema', description: '', role: 'architect', execution_mode: 'inline', depends_on: [stepIds[0]], verify_after: false, estimated_tokens: 5000, preferred_model: 'opus' },
      { id: stepIds[2], name: 'Implement middleware', description: '', role: 'implementer', execution_mode: 'inline', depends_on: [stepIds[1]], verify_after: true, estimated_tokens: 12000, preferred_model: null },
      { id: stepIds[3], name: 'Implement refresh', description: '', role: 'implementer', execution_mode: 'inline', depends_on: [stepIds[1]], verify_after: true, estimated_tokens: 8000, preferred_model: null },
      { id: stepIds[4], name: 'Integration tests', description: '', role: 'tester', execution_mode: 'inline', depends_on: [stepIds[2], stepIds[3]], verify_after: true, estimated_tokens: 7000, preferred_model: null },
    ],
  }),

  // Execution
  evt(4, 9000, 1, { type: 'step_started', step_id: stepIds[0], step_name: 'Analyze current auth', role: 'scout', execution_mode: 'inline' }),
  evt(5, 11500, 1, { type: 'step_completed', step_id: stepIds[0], step_name: 'Analyze current auth', success: true, duration_ms: 2500, tokens_used: 2800, detail_ref: null }),
  evt(6, 11700, 1, { type: 'progress', completed: 1, total: 5, active_steps: [], budget: budgetAt(2800, 3, 2, 0, 4) }),

  evt(7, 12000, 1, { type: 'step_started', step_id: stepIds[1], step_name: 'Design JWT schema', role: 'architect', execution_mode: 'inline' }),
  evt(8, 15000, 1, { type: 'step_completed', step_id: stepIds[1], step_name: 'Design JWT schema', success: true, duration_ms: 3000, tokens_used: 4600, detail_ref: null }),
  evt(9, 15200, 1, { type: 'progress', completed: 2, total: 5, active_steps: [], budget: budgetAt(7400, 5, 2, 0, 8) }),

  // Parallel implementation
  evt(10, 15500, 1, { type: 'step_started', step_id: stepIds[2], step_name: 'Implement middleware', role: 'implementer', execution_mode: 'inline' }),
  evt(11, 15700, 1, { type: 'step_started', step_id: stepIds[3], step_name: 'Implement refresh', role: 'implementer', execution_mode: 'inline' }),

  evt(12, 22000, 1, { type: 'step_completed', step_id: stepIds[2], step_name: 'Implement middleware', success: true, duration_ms: 6500, tokens_used: 11500, detail_ref: null }),
  evt(13, 22500, 1, { type: 'verify_gate_result', step_id: stepIds[2], step_name: 'Implement middleware', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '6 tests passed' }], overall_passed: true, replan_triggered: false }),

  evt(14, 24000, 1, { type: 'step_completed', step_id: stepIds[3], step_name: 'Implement refresh', success: true, duration_ms: 8300, tokens_used: 7500, detail_ref: null }),
  evt(15, 24500, 1, { type: 'verify_gate_result', step_id: stepIds[3], step_name: 'Implement refresh', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '4 tests passed' }], overall_passed: true, replan_triggered: false }),
  evt(16, 24700, 1, { type: 'progress', completed: 4, total: 5, active_steps: [], budget: budgetAt(29200, 18, 4, 2, 20) }),

  // Integration tests
  evt(17, 25000, 1, { type: 'step_started', step_id: stepIds[4], step_name: 'Integration tests', role: 'tester', execution_mode: 'inline' }),
  evt(18, 30000, 1, { type: 'step_completed', step_id: stepIds[4], step_name: 'Integration tests', success: true, duration_ms: 5000, tokens_used: 6800, detail_ref: null }),
  evt(19, 30500, 1, { type: 'verify_gate_result', step_id: stepIds[4], step_name: 'Integration tests', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '12 tests passed' }, { check_type: 'lint', passed: true, summary: '0 warnings' }], overall_passed: true, replan_triggered: false }),
  evt(20, 30700, 1, { type: 'progress', completed: 5, total: 5, active_steps: [], budget: budgetAt(36000, 22, 4, 3, 26) }),

  evt(21, 31000, 1, {
    type: 'run_stopped',
    reason: { type: 'task_complete' },
    total_steps: 21, total_tokens: 36000,
  }),
]

/** Thought bubbles */
export interface Thought {
  text: string
  variant: 'thought' | 'action' | 'warning' | 'success'
  stepId: string | null
  offsetMs: number
}

export const DEMO_THOUGHTS: Thought[] = [
  { text: 'Scanning auth.rs, session.rs, middleware.rs...', variant: 'action', stepId: stepIds[0], offsetMs: 10000 },
  { text: 'Found SessionManager with 3 methods', variant: 'thought', stepId: stepIds[0], offsetMs: 11000 },
  { text: 'Choosing HS256 — jsonwebtoken already in Cargo.toml', variant: 'action', stepId: stepIds[1], offsetMs: 13500 },
  { text: 'Replacing session check in 4 route handlers', variant: 'action', stepId: stepIds[2], offsetMs: 17000 },
  { text: 'Writing POST /auth/refresh with token rotation', variant: 'action', stepId: stepIds[3], offsetMs: 17500 },
  { text: 'All 6 tests passing', variant: 'success', stepId: stepIds[2], offsetMs: 22200 },
  { text: 'Testing full flow: register, login, access, refresh, expire', variant: 'action', stepId: stepIds[4], offsetMs: 27000 },
  { text: '12 tests passed — all clear', variant: 'success', stepId: stepIds[4], offsetMs: 30200 },
]

/**
 * Play the demo with exploration + execution.
 */
export function playDemo(
  onEvent: (event: DashboardEvent) => void,
  onThought?: (thought: Thought) => void,
  onExploration?: (phase: ExplorationPhase['phase']) => void,
): () => void {
  let cancelled = false
  const timeouts: ReturnType<typeof setTimeout>[] = []

  // Exploration phases
  if (onExploration) {
    for (const ep of DEMO_EXPLORATION) {
      timeouts.push(setTimeout(() => { if (!cancelled) onExploration(ep.phase) }, ep.offsetMs))
    }
  }

  // Events — offset by exploration duration
  const baseTime = new Date(DEMO_EVENTS[0].ts).getTime()
  for (const event of DEMO_EVENTS) {
    const delay = new Date(event.ts).getTime() - baseTime + EXPLORATION_DURATION
    timeouts.push(setTimeout(() => { if (!cancelled) onEvent(event) }, delay))
  }

  // Thoughts
  if (onThought) {
    for (const t of DEMO_THOUGHTS) {
      timeouts.push(setTimeout(() => { if (!cancelled) onThought(t) }, t.offsetMs + EXPLORATION_DURATION))
    }
  }

  return () => { cancelled = true; timeouts.forEach(clearTimeout) }
}
