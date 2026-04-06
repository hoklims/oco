/**
 * Demo scenario — simulates a real competitive planning run.
 * 2 candidate plans (Speed vs Safety) with structural differences,
 * then execution of the winner.
 */
import type { DashboardEvent, BudgetSnapshot, ReviewPacket } from './types'

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
  'aaaa0001-0000-0000-0000-000000000000', // research-oss
  'aaaa0002-0000-0000-0000-000000000000', // research-papers
  'aaaa0003-0000-0000-0000-000000000000', // synthesize-research
  'aaaa0004-0000-0000-0000-000000000000', // Design JWT schema
  'aaaa0005-0000-0000-0000-000000000000', // Implement middleware
  'aaaa0006-0000-0000-0000-000000000000', // Implement refresh
  'aaaa0007-0000-0000-0000-000000000000', // Integration tests
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
    { name: 'Search OSS solutions', role: 'researcher', verify: false, tokens: 2000, depends_on: [] },
    { name: 'Search research papers', role: 'researcher', verify: false, tokens: 1500, depends_on: [] },
    { name: 'Synthesize findings', role: 'analyst', verify: false, tokens: 2500, depends_on: ['Search OSS solutions', 'Search research papers'] },
    { name: 'Design JWT schema', role: 'architect', verify: false, tokens: 5000, depends_on: ['Synthesize findings'] },
    { name: 'Implement middleware', role: 'implementer', verify: true, tokens: 12000, depends_on: ['Design JWT schema'] },
    { name: 'Implement refresh', role: 'implementer', verify: true, tokens: 8000, depends_on: ['Design JWT schema'] },
    { name: 'Integration tests', role: 'tester', verify: true, tokens: 7000, depends_on: ['Implement middleware', 'Implement refresh'] },
  ],
  score: 0.81,
  winner: true,
  scores: { verify: 0.60, cost: 0.25, parallel: 0.50, depth: 0.15 },
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
      { strategy: 'safety', step_count: 7, estimated_tokens: 38000, verify_count: 3, parallel_groups: 4, score: 0.81, winner: true },
    ],
    winner_strategy: 'safety',
    winner_score: 0.81,
  }),

  evt(3, 8000, 1, {
    type: 'plan_generated',
    plan_id: 'plan-0001', step_count: 7, parallel_group_count: 4,
    critical_path_length: 5, estimated_total_tokens: 38000,
    strategy: 'Competitive (safety won)', team: null,
    steps: [
      { id: stepIds[0], name: 'Search OSS solutions', description: 'Find existing JWT/auth libraries on crates.io', role: 'researcher', execution_mode: 'subagent', depends_on: [], verify_after: false, estimated_tokens: 2000, preferred_model: 'haiku' },
      { id: stepIds[1], name: 'Search research papers', description: 'Check recent papers on token-based auth patterns', role: 'researcher', execution_mode: 'subagent', depends_on: [], verify_after: false, estimated_tokens: 1500, preferred_model: 'haiku' },
      { id: stepIds[2], name: 'Synthesize findings', description: 'Evaluate prior art and recommend build vs adopt', role: 'analyst', execution_mode: 'inline', depends_on: [stepIds[0], stepIds[1]], verify_after: false, estimated_tokens: 2500, preferred_model: null },
      { id: stepIds[3], name: 'Design JWT schema', description: 'Design token structure informed by research', role: 'architect', execution_mode: 'inline', depends_on: [stepIds[2]], verify_after: false, estimated_tokens: 5000, preferred_model: 'opus' },
      { id: stepIds[4], name: 'Implement middleware', description: 'JWT validation middleware with jsonwebtoken crate', role: 'implementer', execution_mode: 'inline', depends_on: [stepIds[3]], verify_after: true, estimated_tokens: 12000, preferred_model: null },
      { id: stepIds[5], name: 'Implement refresh', description: 'Refresh token rotation with single-use invalidation', role: 'implementer', execution_mode: 'inline', depends_on: [stepIds[3]], verify_after: true, estimated_tokens: 8000, preferred_model: null },
      { id: stepIds[6], name: 'Integration tests', description: 'Full auth flow: register, login, access, refresh, expire', role: 'tester', execution_mode: 'inline', depends_on: [stepIds[4], stepIds[5]], verify_after: true, estimated_tokens: 7000, preferred_model: null },
    ],
  }),

  // Prior art research — parallel subagent execution
  evt(4, 9000, 1, { type: 'step_started', step_id: stepIds[0], step_name: 'Search OSS solutions', role: 'researcher', execution_mode: 'subagent' }),
  evt(5, 9200, 1, { type: 'step_started', step_id: stepIds[1], step_name: 'Search research papers', role: 'researcher', execution_mode: 'subagent' }),

  evt(6, 11000, 1, { type: 'step_completed', step_id: stepIds[1], step_name: 'Search research papers', success: true, duration_ms: 1800, tokens_used: 1200, detail_ref: null }),
  evt(7, 11500, 1, { type: 'step_completed', step_id: stepIds[0], step_name: 'Search OSS solutions', success: true, duration_ms: 2500, tokens_used: 1800, detail_ref: null }),
  evt(8, 11700, 1, { type: 'progress', completed: 2, total: 7, active_steps: [], budget: budgetAt(3000, 4, 3, 0, 4) }),

  // Synthesis
  evt(9, 12000, 1, { type: 'step_started', step_id: stepIds[2], step_name: 'Synthesize findings', role: 'analyst', execution_mode: 'inline' }),
  evt(10, 14000, 1, { type: 'step_completed', step_id: stepIds[2], step_name: 'Synthesize findings', success: true, duration_ms: 2000, tokens_used: 2200, detail_ref: null }),
  evt(11, 14200, 1, { type: 'progress', completed: 3, total: 7, active_steps: [], budget: budgetAt(5200, 6, 3, 0, 7) }),

  // Design
  evt(12, 14500, 1, { type: 'step_started', step_id: stepIds[3], step_name: 'Design JWT schema', role: 'architect', execution_mode: 'inline' }),
  evt(13, 17500, 1, { type: 'step_completed', step_id: stepIds[3], step_name: 'Design JWT schema', success: true, duration_ms: 3000, tokens_used: 4600, detail_ref: null }),
  evt(14, 17700, 1, { type: 'progress', completed: 4, total: 7, active_steps: [], budget: budgetAt(9800, 8, 3, 0, 11) }),

  // Parallel implementation
  evt(15, 18000, 1, { type: 'step_started', step_id: stepIds[4], step_name: 'Implement middleware', role: 'implementer', execution_mode: 'inline' }),
  evt(16, 18200, 1, { type: 'step_started', step_id: stepIds[5], step_name: 'Implement refresh', role: 'implementer', execution_mode: 'inline' }),

  evt(17, 24500, 1, { type: 'step_completed', step_id: stepIds[4], step_name: 'Implement middleware', success: true, duration_ms: 6500, tokens_used: 11500, detail_ref: null }),
  evt(18, 25000, 1, { type: 'verify_gate_result', step_id: stepIds[4], step_name: 'Implement middleware', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '6 tests passed' }], overall_passed: true, replan_triggered: false }),

  evt(19, 26500, 1, { type: 'step_completed', step_id: stepIds[5], step_name: 'Implement refresh', success: true, duration_ms: 8300, tokens_used: 7500, detail_ref: null }),
  evt(20, 27000, 1, { type: 'verify_gate_result', step_id: stepIds[5], step_name: 'Implement refresh', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '4 tests passed' }], overall_passed: true, replan_triggered: false }),
  evt(21, 27200, 1, { type: 'progress', completed: 6, total: 7, active_steps: [], budget: budgetAt(31000, 20, 5, 2, 22) }),

  // Integration tests
  evt(22, 27500, 1, { type: 'step_started', step_id: stepIds[6], step_name: 'Integration tests', role: 'tester', execution_mode: 'inline' }),
  evt(23, 32500, 1, { type: 'step_completed', step_id: stepIds[6], step_name: 'Integration tests', success: true, duration_ms: 5000, tokens_used: 6800, detail_ref: null }),
  evt(24, 33000, 1, { type: 'verify_gate_result', step_id: stepIds[6], step_name: 'Integration tests', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '12 tests passed' }, { check_type: 'lint', passed: true, summary: '0 warnings' }], overall_passed: true, replan_triggered: false }),
  evt(25, 33200, 1, { type: 'progress', completed: 7, total: 7, active_steps: [], budget: budgetAt(38000, 24, 5, 3, 28) }),

  evt(26, 33500, 1, {
    type: 'run_stopped',
    reason: { type: 'task_complete' },
    total_steps: 26, total_tokens: 38000,
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
  // Research phase
  { text: 'Searching crates.io for JWT libraries...', variant: 'action', stepId: stepIds[0], offsetMs: 9500 },
  { text: 'Found jsonwebtoken (42M downloads), jwt-simple, alcoholic-jwt', variant: 'thought', stepId: stepIds[0], offsetMs: 10500 },
  { text: 'Checking arXiv for recent token auth patterns...', variant: 'action', stepId: stepIds[1], offsetMs: 9800 },
  { text: 'Paper: "Refresh Token Rotation" (2024) — single-use invalidation', variant: 'thought', stepId: stepIds[1], offsetMs: 10800 },
  // Synthesis
  { text: 'jsonwebtoken crate covers HS256/RS256 — no need to build from scratch', variant: 'thought', stepId: stepIds[2], offsetMs: 12500 },
  { text: 'Recommendation: adopt jsonwebtoken + implement rotation per paper', variant: 'success', stepId: stepIds[2], offsetMs: 13500 },
  // Design & implementation
  { text: 'Choosing HS256 — jsonwebtoken already in Cargo.toml', variant: 'action', stepId: stepIds[3], offsetMs: 16000 },
  { text: 'Replacing session check in 4 route handlers', variant: 'action', stepId: stepIds[4], offsetMs: 19500 },
  { text: 'Writing POST /auth/refresh with token rotation', variant: 'action', stepId: stepIds[5], offsetMs: 20000 },
  { text: 'All 6 tests passing', variant: 'success', stepId: stepIds[4], offsetMs: 24800 },
  { text: 'Testing full flow: register, login, access, refresh, expire', variant: 'action', stepId: stepIds[6], offsetMs: 29500 },
  { text: '12 tests passed — all clear', variant: 'success', stepId: stepIds[6], offsetMs: 32700 },
]

/** Demo ReviewPacket — shown after run_stopped. */
export const DEMO_REVIEW_PACKET: ReviewPacket = {
  schema_version: 1,
  generated_at: new Date().toISOString(),
  run_id: rid,
  merge_readiness: 'Ready',
  trust_verdict: 'High',
  gate_verdict: 'Pass',
  changes: {
    modified_files: ['src/auth/middleware.rs', 'src/auth/jwt.rs', 'src/auth/refresh.rs', 'src/routes/mod.rs', 'tests/auth_integration.rs'],
    key_decisions: [
      'Prior art research: jsonwebtoken crate (42M downloads) covers HS256/RS256 — adopted instead of building from scratch',
      'Paper-informed: refresh token rotation with single-use invalidation (arXiv 2024)',
      'Chose HS256 over RS256 — jsonwebtoken crate already in Cargo.toml',
      'Preserved backward-compatible session fallback for 1 release cycle',
    ],
    narrative: 'Prior art search identified jsonwebtoken as the standard crate and a 2024 paper on refresh rotation. Replaced session-based auth with JWT tokens using the recommended approach. All 22 tests passing, zero lint warnings. Migration path preserved for existing sessions.',
  },
  verification: {
    trust_verdict: 'High',
    checks_passed: ['cargo build', 'cargo test (22 passed)', 'cargo clippy (0 warnings)', 'cargo fmt --check'],
    checks_failed: [],
    unverified_files: [],
  },
  open_risks: {
    risks: ['Token secret rotation strategy not yet implemented for production'],
    open_questions: ['Should refresh tokens have a max lifetime cap?'],
    unavailable_data: [],
  },
  scorecard: {
    run_id: rid,
    computed_at: new Date().toISOString(),
    dimensions: [
      { dimension: 'Success', score: 0.95, detail: 'All objectives met, all tests passing' },
      { dimension: 'TrustVerdict', score: 0.92, detail: 'High confidence — full verification suite passed' },
      { dimension: 'VerificationCoverage', score: 0.88, detail: '22 tests covering auth, refresh, and middleware' },
      { dimension: 'MissionContinuity', score: 0.85, detail: 'Clear handoff with session fallback path' },
      { dimension: 'CostEfficiency', score: 0.70, detail: '38k tokens used (budget: 50k), 3k saved by prior art research' },
      { dimension: 'ReplanStability', score: 1.0, detail: 'No replans needed — first plan succeeded' },
      { dimension: 'ErrorRate', score: 0.96, detail: '0 errors in 22 tool calls' },
    ],
    overall_score: 0.88,
    cost: { steps: 7, tokens: 38000, duration_ms: 28000, tool_calls: 24, verify_cycles: 3, replans: 0 },
  },
  gate_result: {
    baseline_id: 'v0.5-stable',
    candidate_id: rid,
    policy: {
      thresholds: [
        { dimension: 'Success', min_score: 0.7, max_regression: 0.15 },
        { dimension: 'TrustVerdict', min_score: 0.6, max_regression: 0.2 },
        { dimension: 'VerificationCoverage', min_score: 0.5, max_regression: 0.2 },
        { dimension: 'MissionContinuity', min_score: 0.4, max_regression: 0.25 },
        { dimension: 'CostEfficiency', min_score: 0.3, max_regression: 0.3 },
        { dimension: 'ReplanStability', min_score: 0.5, max_regression: 0.25 },
        { dimension: 'ErrorRate', min_score: 0.7, max_regression: 0.15 },
      ],
      strategy: 'Balanced',
      min_overall_score: 0.6,
      max_overall_regression: 0.15,
    },
    dimension_checks: [
      { dimension: 'Success', candidate_score: 0.95, baseline_score: 0.85, delta: 0.10, min_score: 0.7, max_regression: 0.15, verdict: 'Pass', reason: 'Above threshold, improved from baseline' },
      { dimension: 'TrustVerdict', candidate_score: 0.92, baseline_score: 0.80, delta: 0.12, min_score: 0.6, max_regression: 0.2, verdict: 'Pass', reason: 'Strong improvement' },
      { dimension: 'VerificationCoverage', candidate_score: 0.88, baseline_score: 0.75, delta: 0.13, min_score: 0.5, max_regression: 0.2, verdict: 'Pass', reason: 'Coverage improved' },
      { dimension: 'MissionContinuity', candidate_score: 0.85, baseline_score: 0.82, delta: 0.03, min_score: 0.4, max_regression: 0.25, verdict: 'Pass', reason: 'Stable' },
      { dimension: 'CostEfficiency', candidate_score: 0.70, baseline_score: 0.68, delta: 0.02, min_score: 0.3, max_regression: 0.3, verdict: 'Pass', reason: 'Efficient — prior art research avoided reinventing JWT handling' },
      { dimension: 'ReplanStability', candidate_score: 1.0, baseline_score: 0.90, delta: 0.10, min_score: 0.5, max_regression: 0.25, verdict: 'Pass', reason: 'Perfect stability' },
      { dimension: 'ErrorRate', candidate_score: 0.96, baseline_score: 0.88, delta: 0.08, min_score: 0.7, max_regression: 0.15, verdict: 'Pass', reason: 'Near-zero errors' },
    ],
    baseline_overall: 0.81,
    candidate_overall: 0.88,
    overall_delta: 0.07,
    verdict: 'Pass',
    reasons: ['All dimensions pass', 'Overall score 88% exceeds minimum 60%', 'No regressions detected'],
  },
  baseline_freshness: {
    freshness: 'Fresh',
    age_days: 3,
    fresh_threshold_days: 14,
    stale_threshold_days: 30,
    recommendation: 'Baseline is recent and representative',
  },
}

/**
 * Play the demo with exploration + execution.
 */
export function playDemo(
  onEvent: (event: DashboardEvent) => void,
  onThought?: (thought: Thought) => void,
  onExploration?: (phase: ExplorationPhase['phase']) => void,
  onReviewPacket?: (review: ReviewPacket) => void,
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

  // Review packet — 2s after run_stopped
  if (onReviewPacket) {
    const lastEventDelay = new Date(DEMO_EVENTS[DEMO_EVENTS.length - 1].ts).getTime() - baseTime + EXPLORATION_DURATION
    timeouts.push(setTimeout(() => { if (!cancelled) onReviewPacket(DEMO_REVIEW_PACKET) }, lastEventDelay + 2000))
  }

  return () => { cancelled = true; timeouts.forEach(clearTimeout) }
}
