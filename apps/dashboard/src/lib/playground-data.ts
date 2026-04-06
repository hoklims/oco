/**
 * Playground test scenarios — varied DAG topologies for visual testing.
 *
 * Each scenario provides:
 *  - steps (StepSummary[]) for PlanMap
 *  - events (DashboardEvent[]) for full flow replay
 */

import type { DashboardEvent, StepSummary, BudgetSnapshot } from './types'
import type { Thought, CompetitivePlan } from './demo'

// ── Helpers ──────────────────────────────────────────────────

let _seq = 0
function resetSeq() { _seq = 0 }

function uid(prefix: string, n: number): string {
  return `${prefix}-${String(n).padStart(4, '0')}-0000-0000-000000000000`
}

function ts(offsetMs: number): string {
  return new Date(Date.now() + offsetMs).toISOString()
}

const budgetAt = (tokUsed: number, toolUsed: number, elapsed: number): BudgetSnapshot => ({
  tokens_used: tokUsed, tokens_remaining: 80000 - tokUsed,
  tool_calls_used: toolUsed, tool_calls_remaining: 50 - toolUsed,
  retrievals_used: 0, verify_cycles_used: 0, elapsed_secs: elapsed,
})

function evt(offsetMs: number, planVersion: number, kind: Record<string, unknown>): DashboardEvent {
  return {
    schema_version: 1, seq: ++_seq, ts: ts(offsetMs),
    session_id: 'playground-session', run_id: 'playground-run',
    plan_version: planVersion, kind: kind as DashboardEvent['kind'],
  }
}

// ── Scenario interface ───────────────────────────────────────

export interface PlaygroundScenario {
  id: string
  name: string
  description: string
  steps: StepSummary[]
  events: DashboardEvent[]
  thoughts: Thought[]
  /**
   * Tailored candidate plans shown during the exploration phase.
   * When present, PlanExplorer renders these instead of the generic demo.
   */
  explorationPlans?: { loser: CompetitivePlan; winner: CompetitivePlan }
}

// ── 1. Linear (3 steps, pure sequential) ─────────────────────

function buildLinear(): PlaygroundScenario {
  resetSeq()
  const ids = [uid('lin', 1), uid('lin', 2), uid('lin', 3)]

  const steps: StepSummary[] = [
    { id: ids[0], name: 'Root config', description: 'Set up project configuration', role: 'scout', execution_mode: 'inline', depends_on: [], verify_after: false, estimated_tokens: 2000, preferred_model: null },
    { id: ids[1], name: 'Shared schemas', description: 'Define shared type schemas', role: 'architect', execution_mode: 'inline', depends_on: [ids[0]], verify_after: false, estimated_tokens: 4000, preferred_model: 'opus' },
    { id: ids[2], name: 'API endpoint', description: 'Implement the REST endpoint', role: 'implementer', execution_mode: 'inline', depends_on: [ids[1]], verify_after: true, estimated_tokens: 8000, preferred_model: null },
  ]

  const events: DashboardEvent[] = [
    evt(0, 0, { type: 'run_started', provider: 'claude-code', model: 'sonnet', request_summary: 'Add GET /api/health endpoint' }),
    evt(200, 1, { type: 'plan_generated', plan_id: 'p-lin', step_count: 3, parallel_group_count: 1, critical_path_length: 3, estimated_total_tokens: 14000, strategy: 'linear', team: null, steps }),
    evt(1000, 1, { type: 'step_started', step_id: ids[0], step_name: 'Root config', role: 'scout', execution_mode: 'inline' }),
    evt(3000, 1, { type: 'step_completed', step_id: ids[0], step_name: 'Root config', success: true, duration_ms: 2000, tokens_used: 1800, detail_ref: null }),
    evt(3200, 1, { type: 'progress', completed: 1, total: 3, active_steps: [], budget: budgetAt(1800, 1, 3) }),
    evt(3500, 1, { type: 'step_started', step_id: ids[1], step_name: 'Shared schemas', role: 'architect', execution_mode: 'inline' }),
    evt(6500, 1, { type: 'step_completed', step_id: ids[1], step_name: 'Shared schemas', success: true, duration_ms: 3000, tokens_used: 3600, detail_ref: null }),
    evt(6700, 1, { type: 'progress', completed: 2, total: 3, active_steps: [], budget: budgetAt(5400, 2, 7) }),
    evt(7000, 1, { type: 'step_started', step_id: ids[2], step_name: 'API endpoint', role: 'implementer', execution_mode: 'inline' }),
    evt(12000, 1, { type: 'step_completed', step_id: ids[2], step_name: 'API endpoint', success: true, duration_ms: 5000, tokens_used: 7200, detail_ref: null }),
    evt(12500, 1, { type: 'verify_gate_result', step_id: ids[2], step_name: 'API endpoint', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '3 tests passed' }], overall_passed: true, replan_triggered: false }),
    evt(13000, 1, { type: 'run_stopped', reason: { type: 'task_complete' }, total_steps: 3, total_tokens: 12600 }),
  ]

  return { id: 'linear', name: 'Linear (3 steps)', description: 'Pure sequential pipeline — scout → architect → implementer', steps, events, thoughts: [] }
}

// ── 2. Parallel fork/join ────────────────────────────────────

function buildParallel(): PlaygroundScenario {
  resetSeq()
  const ids = [uid('par', 1), uid('par', 2), uid('par', 3), uid('par', 4), uid('par', 5), uid('par', 6)]

  const steps: StepSummary[] = [
    { id: ids[0], name: 'Root config', description: 'Project setup', role: 'scout', execution_mode: 'inline', depends_on: [], verify_after: false, estimated_tokens: 2000, preferred_model: null },
    { id: ids[1], name: 'Shared schemas', description: 'Common types', role: 'architect', execution_mode: 'inline', depends_on: [ids[0]], verify_after: false, estimated_tokens: 4000, preferred_model: 'opus' },
    { id: ids[2], name: 'API backend', description: 'REST API routes', role: 'implementer', execution_mode: 'subagent', depends_on: [ids[1]], verify_after: true, estimated_tokens: 12000, preferred_model: null },
    { id: ids[3], name: 'Web frontend', description: 'Svelte UI components', role: 'implementer', execution_mode: 'subagent', depends_on: [ids[1]], verify_after: true, estimated_tokens: 10000, preferred_model: null },
    { id: ids[4], name: 'Docker config', description: 'Dockerfile + compose', role: 'implementer', execution_mode: 'inline', depends_on: [ids[2], ids[3]], verify_after: false, estimated_tokens: 3000, preferred_model: null },
    { id: ids[5], name: 'CI pipeline', description: 'GitHub Actions', role: 'verifier', execution_mode: 'inline', depends_on: [ids[4]], verify_after: true, estimated_tokens: 5000, preferred_model: null },
  ]

  const events: DashboardEvent[] = [
    evt(0, 0, { type: 'run_started', provider: 'claude-code', model: 'sonnet', request_summary: 'Bootstrap full-stack app with Docker + CI' }),
    evt(200, 0, { type: 'plan_exploration', candidates: [
      { strategy: 'speed', step_count: 4, estimated_tokens: 24000, score: 0.61, strengths: ['Fast'], weaknesses: ['No parallel'] },
      { strategy: 'safety', step_count: 6, estimated_tokens: 36000, score: 0.82, strengths: ['Parallel fork', 'Full verify'], weaknesses: ['Slower'] },
    ], winner_strategy: 'safety', winner_score: 0.82 }),
    evt(12000, 1, { type: 'plan_generated', plan_id: 'p-par', step_count: 6, parallel_group_count: 3, critical_path_length: 5, estimated_total_tokens: 36000, strategy: 'safety (parallel fork)', team: null, steps }),
    evt(13000, 1, { type: 'step_started', step_id: ids[0], step_name: 'Root config', role: 'scout', execution_mode: 'inline' }),
    evt(15000, 1, { type: 'step_completed', step_id: ids[0], step_name: 'Root config', success: true, duration_ms: 2000, tokens_used: 1800, detail_ref: null }),
    evt(15500, 1, { type: 'step_started', step_id: ids[1], step_name: 'Shared schemas', role: 'architect', execution_mode: 'inline' }),
    evt(18500, 1, { type: 'step_completed', step_id: ids[1], step_name: 'Shared schemas', success: true, duration_ms: 3000, tokens_used: 3600, detail_ref: null }),
    // Parallel fork
    evt(19000, 1, { type: 'step_started', step_id: ids[2], step_name: 'API backend', role: 'implementer', execution_mode: 'subagent' }),
    evt(19200, 1, { type: 'step_started', step_id: ids[3], step_name: 'Web frontend', role: 'implementer', execution_mode: 'subagent' }),
    evt(25000, 1, { type: 'step_completed', step_id: ids[2], step_name: 'API backend', success: true, duration_ms: 6000, tokens_used: 11000, detail_ref: null }),
    evt(25500, 1, { type: 'verify_gate_result', step_id: ids[2], step_name: 'API backend', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '8 tests passed' }], overall_passed: true, replan_triggered: false }),
    evt(27000, 1, { type: 'step_completed', step_id: ids[3], step_name: 'Web frontend', success: true, duration_ms: 7800, tokens_used: 9500, detail_ref: null }),
    evt(27500, 1, { type: 'verify_gate_result', step_id: ids[3], step_name: 'Web frontend', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '5 tests passed' }], overall_passed: true, replan_triggered: false }),
    // Join
    evt(28000, 1, { type: 'step_started', step_id: ids[4], step_name: 'Docker config', role: 'implementer', execution_mode: 'inline' }),
    evt(31000, 1, { type: 'step_completed', step_id: ids[4], step_name: 'Docker config', success: true, duration_ms: 3000, tokens_used: 2800, detail_ref: null }),
    evt(31500, 1, { type: 'step_started', step_id: ids[5], step_name: 'CI pipeline', role: 'verifier', execution_mode: 'inline' }),
    evt(35000, 1, { type: 'step_completed', step_id: ids[5], step_name: 'CI pipeline', success: true, duration_ms: 3500, tokens_used: 4500, detail_ref: null }),
    evt(35500, 1, { type: 'verify_gate_result', step_id: ids[5], step_name: 'CI pipeline', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'lint', passed: true, summary: '0 warnings' }], overall_passed: true, replan_triggered: false }),
    evt(36000, 1, { type: 'run_stopped', reason: { type: 'task_complete' }, total_steps: 6, total_tokens: 33200 }),
  ]

  const explorationPlans = {
    loser: {
      strategy: 'speed',
      steps: [
        { name: 'Bootstrap', role: 'scout', verify: false, tokens: 3000, depends_on: [] },
        { name: 'Build everything', role: 'implementer', verify: true, tokens: 18000, depends_on: ['Bootstrap'] },
        { name: 'Ship', role: 'verifier', verify: true, tokens: 3000, depends_on: ['Build everything'] },
      ],
      score: 0.61,
      winner: false,
      scores: { verify: 0.55, cost: 0.65, parallel: 0.00, depth: 0.00 },
    },
    winner: {
      strategy: 'safety',
      steps: [
        { name: 'Root config', role: 'scout', verify: false, tokens: 2000, depends_on: [] },
        { name: 'Shared schemas', role: 'architect', verify: false, tokens: 4000, depends_on: ['Root config'] },
        { name: 'API backend', role: 'implementer', verify: true, tokens: 12000, depends_on: ['Shared schemas'] },
        { name: 'Web frontend', role: 'implementer', verify: true, tokens: 10000, depends_on: ['Shared schemas'] },
        { name: 'Docker config', role: 'implementer', verify: false, tokens: 3000, depends_on: ['API backend', 'Web frontend'] },
        { name: 'CI pipeline', role: 'verifier', verify: true, tokens: 5000, depends_on: ['Docker config'] },
      ],
      score: 0.82,
      winner: true,
      scores: { verify: 0.75, cost: 0.40, parallel: 0.60, depth: 0.50 },
    },
  }
  return { id: 'parallel', name: 'Parallel (fork/join)', description: 'Fork after schemas → API + Frontend in parallel → Docker → CI', steps, events, thoughts: [], explorationPlans }
}

// ── 3. Team (3 agents) ───────────────────────────────────────

function buildTeam(): PlaygroundScenario {
  resetSeq()
  const ids = [uid('team', 1), uid('team', 2), uid('team', 3), uid('team', 4), uid('team', 5), uid('team', 6)]

  const steps: StepSummary[] = [
    { id: ids[0], name: 'Analyze codebase', description: 'Scan repo structure and dependencies', role: 'scout', execution_mode: 'inline', depends_on: [], verify_after: false, estimated_tokens: 3000, preferred_model: null },
    { id: ids[1], name: 'Implement auth', description: 'JWT auth middleware', role: 'implementer', execution_mode: 'subagent', depends_on: [ids[0]], verify_after: true, estimated_tokens: 15000, preferred_model: 'opus' },
    { id: ids[2], name: 'Implement API', description: 'REST endpoints', role: 'implementer', execution_mode: 'teammate', depends_on: [ids[0]], verify_after: true, estimated_tokens: 12000, preferred_model: null },
    { id: ids[3], name: 'Implement UI', description: 'Frontend components', role: 'implementer', execution_mode: 'teammate', depends_on: [ids[0]], verify_after: true, estimated_tokens: 10000, preferred_model: null },
    { id: ids[4], name: 'Integration tests', description: 'End-to-end flow testing', role: 'tester', execution_mode: 'inline', depends_on: [ids[1], ids[2], ids[3]], verify_after: true, estimated_tokens: 8000, preferred_model: null },
    { id: ids[5], name: 'Final verify', description: 'Build + lint + full test suite', role: 'verifier', execution_mode: 'inline', depends_on: [ids[4]], verify_after: true, estimated_tokens: 4000, preferred_model: null },
  ]

  const events: DashboardEvent[] = [
    evt(0, 0, { type: 'run_started', provider: 'claude-code', model: 'opus', request_summary: 'Build auth + API + UI with agent team' }),
    evt(100, 0, { type: 'plan_exploration', candidates: [
      { strategy: 'solo', step_count: 4, estimated_tokens: 42000, score: 0.62, strengths: ['Sequential'], weaknesses: ['No collaboration'] },
      { strategy: 'team-mesh', step_count: 6, estimated_tokens: 52000, score: 0.88, strengths: ['Mesh collaboration', '3 parallel agents'], weaknesses: ['Coordination overhead'] },
    ], winner_strategy: 'team-mesh', winner_score: 0.88 }),
    evt(200, 1, { type: 'plan_generated', plan_id: 'p-team', step_count: 6, parallel_group_count: 3, critical_path_length: 4, estimated_total_tokens: 52000, strategy: 'team (3 agents)', team: { name: 'alpha-team', topology: 'mesh', member_count: 3 }, steps }),
    evt(1000, 1, { type: 'step_started', step_id: ids[0], step_name: 'Analyze codebase', role: 'scout', execution_mode: 'inline' }),
    evt(3500, 1, { type: 'step_completed', step_id: ids[0], step_name: 'Analyze codebase', success: true, duration_ms: 2500, tokens_used: 2800, detail_ref: null }),
    // 3 agents in parallel
    evt(4000, 1, { type: 'step_started', step_id: ids[1], step_name: 'Implement auth', role: 'implementer', execution_mode: 'subagent' }),
    evt(4200, 1, { type: 'step_started', step_id: ids[2], step_name: 'Implement API', role: 'implementer', execution_mode: 'teammate' }),
    evt(4400, 1, { type: 'step_started', step_id: ids[3], step_name: 'Implement UI', role: 'implementer', execution_mode: 'teammate' }),
    evt(12000, 1, { type: 'step_completed', step_id: ids[3], step_name: 'Implement UI', success: true, duration_ms: 7600, tokens_used: 9200, detail_ref: null }),
    evt(12500, 1, { type: 'verify_gate_result', step_id: ids[3], step_name: 'Implement UI', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }], overall_passed: true, replan_triggered: false }),
    evt(14000, 1, { type: 'step_completed', step_id: ids[2], step_name: 'Implement API', success: true, duration_ms: 9800, tokens_used: 11000, detail_ref: null }),
    evt(14500, 1, { type: 'verify_gate_result', step_id: ids[2], step_name: 'Implement API', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '6 tests passed' }], overall_passed: true, replan_triggered: false }),
    evt(16000, 1, { type: 'step_completed', step_id: ids[1], step_name: 'Implement auth', success: true, duration_ms: 12000, tokens_used: 14000, detail_ref: null }),
    evt(16500, 1, { type: 'verify_gate_result', step_id: ids[1], step_name: 'Implement auth', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '10 tests passed' }], overall_passed: true, replan_triggered: false }),
    // Join
    evt(17000, 1, { type: 'step_started', step_id: ids[4], step_name: 'Integration tests', role: 'tester', execution_mode: 'inline' }),
    evt(22000, 1, { type: 'step_completed', step_id: ids[4], step_name: 'Integration tests', success: true, duration_ms: 5000, tokens_used: 7500, detail_ref: null }),
    evt(22500, 1, { type: 'verify_gate_result', step_id: ids[4], step_name: 'Integration tests', checks: [{ check_type: 'test', passed: true, summary: '18 tests passed' }], overall_passed: true, replan_triggered: false }),
    evt(23000, 1, { type: 'step_started', step_id: ids[5], step_name: 'Final verify', role: 'verifier', execution_mode: 'inline' }),
    evt(26000, 1, { type: 'step_completed', step_id: ids[5], step_name: 'Final verify', success: true, duration_ms: 3000, tokens_used: 3500, detail_ref: null }),
    evt(26500, 1, { type: 'verify_gate_result', step_id: ids[5], step_name: 'Final verify', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '24 tests passed' }, { check_type: 'lint', passed: true, summary: '0 warnings' }], overall_passed: true, replan_triggered: false }),
    evt(27000, 1, { type: 'run_stopped', reason: { type: 'task_complete' }, total_steps: 6, total_tokens: 48000 }),
  ]

  const explorationPlans = {
    loser: {
      strategy: 'solo',
      steps: [
        { name: 'Analyze', role: 'scout', verify: false, tokens: 3000, depends_on: [] },
        { name: 'Build sequentially', role: 'implementer', verify: true, tokens: 35000, depends_on: ['Analyze'] },
        { name: 'Tests', role: 'tester', verify: true, tokens: 4000, depends_on: ['Build sequentially'] },
      ],
      score: 0.62,
      winner: false,
      scores: { verify: 0.55, cost: 0.60, parallel: 0.00, depth: 0.10 },
    },
    winner: {
      strategy: 'team-mesh',
      steps: [
        { name: 'Analyze codebase', role: 'scout', verify: false, tokens: 3000, depends_on: [] },
        { name: 'Auth middleware', role: 'implementer', verify: true, tokens: 15000, depends_on: ['Analyze codebase'] },
        { name: 'REST API', role: 'implementer', verify: true, tokens: 12000, depends_on: ['Analyze codebase'] },
        { name: 'Frontend UI', role: 'implementer', verify: true, tokens: 10000, depends_on: ['Analyze codebase'] },
        { name: 'Integration tests', role: 'tester', verify: true, tokens: 8000, depends_on: ['Auth middleware', 'REST API', 'Frontend UI'] },
        { name: 'Final verify', role: 'verifier', verify: true, tokens: 4000, depends_on: ['Integration tests'] },
      ],
      score: 0.88,
      winner: true,
      scores: { verify: 0.85, cost: 0.35, parallel: 0.75, depth: 0.65 },
    },
  }
  return { id: 'team', name: 'Team (3 agents)', description: 'Scout → 3 parallel agents (subagent + 2 teammates) → Integration → Verify', steps, events, thoughts: [], explorationPlans }
}

// ── 4. Complex (8 steps, 3 parallel groups, failure) ─────────

function buildComplex(): PlaygroundScenario {
  resetSeq()
  const ids = Array.from({ length: 8 }, (_, i) => uid('cpx', i + 1))

  const steps: StepSummary[] = [
    { id: ids[0], name: 'Scan dependencies', description: 'Audit current deps', role: 'scout', execution_mode: 'inline', depends_on: [], verify_after: false, estimated_tokens: 2000, preferred_model: null },
    { id: ids[1], name: 'Design migration', description: 'Plan DB migration strategy', role: 'architect', execution_mode: 'inline', depends_on: [ids[0]], verify_after: false, estimated_tokens: 5000, preferred_model: 'opus' },
    { id: ids[2], name: 'Schema migration', description: 'Write migration scripts', role: 'implementer', execution_mode: 'subagent', depends_on: [ids[1]], verify_after: true, estimated_tokens: 10000, preferred_model: null },
    { id: ids[3], name: 'API adapters', description: 'Update API to new schema', role: 'implementer', execution_mode: 'teammate', depends_on: [ids[1]], verify_after: true, estimated_tokens: 12000, preferred_model: null },
    { id: ids[4], name: 'Seed data', description: 'Generate test fixtures', role: 'implementer', execution_mode: 'inline', depends_on: [ids[2]], verify_after: false, estimated_tokens: 4000, preferred_model: null },
    { id: ids[5], name: 'E2E tests', description: 'End-to-end test suite', role: 'tester', execution_mode: 'inline', depends_on: [ids[3], ids[4]], verify_after: true, estimated_tokens: 8000, preferred_model: null },
    { id: ids[6], name: 'Perf benchmark', description: 'Run load tests', role: 'tester', execution_mode: 'subagent', depends_on: [ids[3], ids[4]], verify_after: false, estimated_tokens: 6000, preferred_model: null },
    { id: ids[7], name: 'Deploy staging', description: 'Deploy to staging env', role: 'verifier', execution_mode: 'inline', depends_on: [ids[5], ids[6]], verify_after: true, estimated_tokens: 5000, preferred_model: null },
  ]

  const events: DashboardEvent[] = [
    evt(0, 0, { type: 'run_started', provider: 'claude-code', model: 'opus', request_summary: 'Migrate database schema v2 → v3 with zero downtime' }),
    evt(200, 0, { type: 'plan_exploration', candidates: [
      { strategy: 'speed', step_count: 5, estimated_tokens: 30000, score: 0.55, strengths: ['Fast'], weaknesses: ['No perf check', 'Risky'] },
      { strategy: 'safety', step_count: 8, estimated_tokens: 52000, score: 0.85, strengths: ['Full coverage', 'Perf bench', 'Staged deploy'], weaknesses: ['Slower', 'Higher cost'] },
    ], winner_strategy: 'safety', winner_score: 0.85 }),
    evt(14000, 1, { type: 'plan_generated', plan_id: 'p-cpx', step_count: 8, parallel_group_count: 4, critical_path_length: 6, estimated_total_tokens: 52000, strategy: 'safety (comprehensive)', team: null, steps }),
    evt(15000, 1, { type: 'step_started', step_id: ids[0], step_name: 'Scan dependencies', role: 'scout', execution_mode: 'inline' }),
    evt(17000, 1, { type: 'step_completed', step_id: ids[0], step_name: 'Scan dependencies', success: true, duration_ms: 2000, tokens_used: 1800, detail_ref: null }),
    evt(17500, 1, { type: 'step_started', step_id: ids[1], step_name: 'Design migration', role: 'architect', execution_mode: 'inline' }),
    evt(21000, 1, { type: 'step_completed', step_id: ids[1], step_name: 'Design migration', success: true, duration_ms: 3500, tokens_used: 4500, detail_ref: null }),
    // Parallel: schema migration + API adapters
    evt(21500, 1, { type: 'step_started', step_id: ids[2], step_name: 'Schema migration', role: 'implementer', execution_mode: 'subagent' }),
    evt(21700, 1, { type: 'step_started', step_id: ids[3], step_name: 'API adapters', role: 'implementer', execution_mode: 'teammate' }),
    evt(27000, 1, { type: 'step_completed', step_id: ids[2], step_name: 'Schema migration', success: true, duration_ms: 5500, tokens_used: 9200, detail_ref: null }),
    evt(27500, 1, { type: 'verify_gate_result', step_id: ids[2], step_name: 'Schema migration', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '5 migration tests passed' }], overall_passed: true, replan_triggered: false }),
    evt(29000, 1, { type: 'step_completed', step_id: ids[3], step_name: 'API adapters', success: true, duration_ms: 7300, tokens_used: 11000, detail_ref: null }),
    evt(29500, 1, { type: 'verify_gate_result', step_id: ids[3], step_name: 'API adapters', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '12 tests passed' }], overall_passed: true, replan_triggered: false }),
    // Seed data (depends on schema migration only)
    evt(28000, 1, { type: 'step_started', step_id: ids[4], step_name: 'Seed data', role: 'implementer', execution_mode: 'inline' }),
    evt(31000, 1, { type: 'step_completed', step_id: ids[4], step_name: 'Seed data', success: true, duration_ms: 3000, tokens_used: 3500, detail_ref: null }),
    // Parallel: E2E tests + Perf benchmark (both depend on API adapters + Seed data)
    evt(31500, 1, { type: 'step_started', step_id: ids[5], step_name: 'E2E tests', role: 'tester', execution_mode: 'inline' }),
    evt(31700, 1, { type: 'step_started', step_id: ids[6], step_name: 'Perf benchmark', role: 'tester', execution_mode: 'subagent' }),
    evt(36000, 1, { type: 'step_completed', step_id: ids[5], step_name: 'E2E tests', success: true, duration_ms: 4500, tokens_used: 7200, detail_ref: null }),
    evt(36500, 1, { type: 'verify_gate_result', step_id: ids[5], step_name: 'E2E tests', checks: [{ check_type: 'test', passed: true, summary: '22 tests passed' }], overall_passed: true, replan_triggered: false }),
    evt(38000, 1, { type: 'step_completed', step_id: ids[6], step_name: 'Perf benchmark', success: true, duration_ms: 6300, tokens_used: 5500, detail_ref: null }),
    // Final deploy
    evt(38500, 1, { type: 'step_started', step_id: ids[7], step_name: 'Deploy staging', role: 'verifier', execution_mode: 'inline' }),
    evt(42000, 1, { type: 'step_completed', step_id: ids[7], step_name: 'Deploy staging', success: true, duration_ms: 3500, tokens_used: 4500, detail_ref: null }),
    evt(42500, 1, { type: 'verify_gate_result', step_id: ids[7], step_name: 'Deploy staging', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '22 tests passed' }, { check_type: 'lint', passed: true, summary: '0 warnings' }], overall_passed: true, replan_triggered: false }),
    evt(43000, 1, { type: 'run_stopped', reason: { type: 'task_complete' }, total_steps: 8, total_tokens: 47200 }),
  ]

  const explorationPlans = {
    loser: {
      strategy: 'simple',
      steps: [
        { name: 'Scan deps', role: 'scout', verify: false, tokens: 2000, depends_on: [] },
        { name: 'Migrate + test', role: 'implementer', verify: true, tokens: 30000, depends_on: ['Scan deps'] },
        { name: 'Deploy', role: 'verifier', verify: true, tokens: 5000, depends_on: ['Migrate + test'] },
      ],
      score: 0.60,
      winner: false,
      scores: { verify: 0.50, cost: 0.70, parallel: 0.00, depth: 0.10 },
    },
    winner: {
      strategy: 'safety',
      steps: [
        { name: 'Scan deps', role: 'scout', verify: false, tokens: 2000, depends_on: [] },
        { name: 'Design migration', role: 'architect', verify: false, tokens: 5000, depends_on: ['Scan deps'] },
        { name: 'Schema migration', role: 'implementer', verify: true, tokens: 10000, depends_on: ['Design migration'] },
        { name: 'API adapters', role: 'implementer', verify: true, tokens: 12000, depends_on: ['Design migration'] },
        { name: 'Seed data', role: 'implementer', verify: false, tokens: 4000, depends_on: ['Schema migration'] },
        { name: 'E2E tests', role: 'tester', verify: true, tokens: 8000, depends_on: ['API adapters', 'Seed data'] },
        { name: 'Perf benchmark', role: 'tester', verify: false, tokens: 6000, depends_on: ['API adapters', 'Seed data'] },
        { name: 'Deploy staging', role: 'verifier', verify: true, tokens: 5000, depends_on: ['E2E tests', 'Perf benchmark'] },
      ],
      score: 0.91,
      winner: true,
      scores: { verify: 0.85, cost: 0.30, parallel: 0.70, depth: 0.75 },
    },
  }
  return { id: 'complex', name: 'Complex (8 steps)', description: 'Multiple parallel groups, subagent + teammate, diamond join, verify gates', steps, events, thoughts: [], explorationPlans }
}

// ── 5. Team + Communication (teammates exchanging messages) ──

function buildTeamComm(): PlaygroundScenario {
  resetSeq()
  const ids = [uid('tcomm', 1), uid('tcomm', 2), uid('tcomm', 3), uid('tcomm', 4), uid('tcomm', 5)]

  const steps: StepSummary[] = [
    { id: ids[0], name: 'Analyze codebase', description: 'Scan structure', role: 'scout', execution_mode: 'inline', depends_on: [], verify_after: false, estimated_tokens: 3000, preferred_model: null },
    { id: ids[1], name: 'Implement API', description: 'REST endpoints', role: 'implementer', execution_mode: 'teammate', depends_on: [ids[0]], verify_after: true, estimated_tokens: 12000, preferred_model: null },
    { id: ids[2], name: 'Implement UI', description: 'Frontend views', role: 'implementer', execution_mode: 'teammate', depends_on: [ids[0]], verify_after: true, estimated_tokens: 10000, preferred_model: null },
    { id: ids[3], name: 'Integration tests', description: 'E2E test suite', role: 'tester', execution_mode: 'inline', depends_on: [ids[1], ids[2]], verify_after: true, estimated_tokens: 8000, preferred_model: null },
    { id: ids[4], name: 'Deploy', description: 'Ship to staging', role: 'verifier', execution_mode: 'inline', depends_on: [ids[3]], verify_after: true, estimated_tokens: 4000, preferred_model: null },
  ]

  const events: DashboardEvent[] = [
    evt(0, 0, { type: 'run_started', provider: 'claude-code', model: 'opus', request_summary: 'Build feature with team collaboration' }),
    evt(100, 0, { type: 'plan_exploration', candidates: [
      { strategy: 'silo', step_count: 4, estimated_tokens: 32000, score: 0.64, strengths: ['Independent'], weaknesses: ['No cross-talk'] },
      { strategy: 'team-mesh', step_count: 5, estimated_tokens: 37000, score: 0.84, strengths: ['Mesh topology', 'Message exchange'], weaknesses: ['Bandwidth'] },
    ], winner_strategy: 'team-mesh', winner_score: 0.84 }),
    evt(200, 1, { type: 'plan_generated', plan_id: 'p-tcomm', step_count: 5, parallel_group_count: 2, critical_path_length: 4, estimated_total_tokens: 37000, strategy: 'team (mesh)', team: { name: 'feature-team', topology: 'mesh', member_count: 2 }, steps }),
    // Scout
    evt(1000, 1, { type: 'step_started', step_id: ids[0], step_name: 'Analyze codebase', role: 'scout', execution_mode: 'inline' }),
    evt(3500, 1, { type: 'step_completed', step_id: ids[0], step_name: 'Analyze codebase', success: true, duration_ms: 2500, tokens_used: 2800, detail_ref: null }),
    // Both teammates start
    evt(4000, 1, { type: 'step_started', step_id: ids[1], step_name: 'Implement API', role: 'implementer', execution_mode: 'teammate' }),
    evt(4200, 1, { type: 'step_started', step_id: ids[2], step_name: 'Implement UI', role: 'implementer', execution_mode: 'teammate' }),
    // Communication: API sends endpoint schema to UI
    evt(6000, 1, { type: 'teammate_message', from_step_id: ids[1], to_step_id: ids[2], from_name: 'Implement API', to_name: 'Implement UI', summary: 'GET /api/items → [{id, name, status}]' }),
    // Communication: UI asks API about auth
    evt(8000, 1, { type: 'teammate_message', from_step_id: ids[2], to_step_id: ids[1], from_name: 'Implement UI', to_name: 'Implement API', summary: 'Need auth header format' }),
    // Communication: API replies with token format
    evt(9500, 1, { type: 'teammate_message', from_step_id: ids[1], to_step_id: ids[2], from_name: 'Implement API', to_name: 'Implement UI', summary: 'Bearer JWT, exp 1h' }),
    // API completes
    evt(12000, 1, { type: 'step_completed', step_id: ids[1], step_name: 'Implement API', success: true, duration_ms: 8000, tokens_used: 11000, detail_ref: null }),
    evt(12500, 1, { type: 'verify_gate_result', step_id: ids[1], step_name: 'Implement API', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '8 tests' }], overall_passed: true, replan_triggered: false }),
    // Communication: API tells UI it's done
    evt(13000, 1, { type: 'teammate_message', from_step_id: ids[1], to_step_id: ids[2], from_name: 'Implement API', to_name: 'Implement UI', summary: 'API complete, all endpoints ready' }),
    // UI completes
    evt(15000, 1, { type: 'step_completed', step_id: ids[2], step_name: 'Implement UI', success: true, duration_ms: 10800, tokens_used: 9500, detail_ref: null }),
    evt(15500, 1, { type: 'verify_gate_result', step_id: ids[2], step_name: 'Implement UI', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '5 tests' }], overall_passed: true, replan_triggered: false }),
    // Integration
    evt(16000, 1, { type: 'step_started', step_id: ids[3], step_name: 'Integration tests', role: 'tester', execution_mode: 'inline' }),
    evt(20000, 1, { type: 'step_completed', step_id: ids[3], step_name: 'Integration tests', success: true, duration_ms: 4000, tokens_used: 7200, detail_ref: null }),
    evt(20500, 1, { type: 'verify_gate_result', step_id: ids[3], step_name: 'Integration tests', checks: [{ check_type: 'test', passed: true, summary: '15 tests' }], overall_passed: true, replan_triggered: false }),
    // Deploy
    evt(21000, 1, { type: 'step_started', step_id: ids[4], step_name: 'Deploy', role: 'verifier', execution_mode: 'inline' }),
    evt(24000, 1, { type: 'step_completed', step_id: ids[4], step_name: 'Deploy', success: true, duration_ms: 3000, tokens_used: 3800, detail_ref: null }),
    evt(24500, 1, { type: 'verify_gate_result', step_id: ids[4], step_name: 'Deploy', checks: [{ check_type: 'build', passed: true, summary: 'deployed' }], overall_passed: true, replan_triggered: false }),
    evt(25000, 1, { type: 'run_stopped', reason: { type: 'task_complete' }, total_steps: 5, total_tokens: 34300 }),
  ]

  const explorationPlans = {
    loser: {
      strategy: 'silo',
      steps: [
        { name: 'Split tasks', role: 'architect', verify: false, tokens: 3000, depends_on: [] },
        { name: 'Team A isolated', role: 'implementer', verify: true, tokens: 14000, depends_on: ['Split tasks'] },
        { name: 'Team B isolated', role: 'implementer', verify: true, tokens: 12000, depends_on: ['Split tasks'] },
        { name: 'Merge + verify', role: 'verifier', verify: true, tokens: 3000, depends_on: ['Team A isolated', 'Team B isolated'] },
      ],
      score: 0.64,
      winner: false,
      scores: { verify: 0.60, cost: 0.55, parallel: 0.40, depth: 0.15 },
    },
    winner: {
      strategy: 'team-mesh',
      steps: [
        { name: 'Analyze', role: 'scout', verify: false, tokens: 3000, depends_on: [] },
        { name: 'API (with mesh)', role: 'implementer', verify: true, tokens: 12000, depends_on: ['Analyze'] },
        { name: 'UI (with mesh)', role: 'implementer', verify: true, tokens: 10000, depends_on: ['Analyze'] },
        { name: 'Integration', role: 'tester', verify: true, tokens: 8000, depends_on: ['API (with mesh)', 'UI (with mesh)'] },
        { name: 'Deploy', role: 'verifier', verify: true, tokens: 4000, depends_on: ['Integration'] },
      ],
      score: 0.84,
      winner: true,
      scores: { verify: 0.75, cost: 0.40, parallel: 0.65, depth: 0.55 },
    },
  }
  return { id: 'team-comm', name: 'Team + Communication', description: 'Two teammates (mesh) exchanging messages during parallel work', steps, events, thoughts: [], explorationPlans }
}

// ── 6. Hierarchical (sub-plans inside steps) ─────────────────

function buildHierarchical(): PlaygroundScenario {
  resetSeq()
  const ids = [uid('hier', 1), uid('hier', 2), uid('hier', 3), uid('hier', 4)]
  const subIds = [uid('sub-a', 1), uid('sub-a', 2), uid('sub-a', 3), uid('sub-b', 1), uid('sub-b', 2), uid('sub-b', 3)]

  const steps: StepSummary[] = [
    { id: ids[0], name: 'Analyze requirements', description: 'Parse task', role: 'scout', execution_mode: 'inline', depends_on: [], verify_after: false, estimated_tokens: 2000, preferred_model: null },
    { id: ids[1], name: 'Build API layer', description: 'REST API with auth', role: 'implementer', execution_mode: 'subagent', depends_on: [ids[0]], verify_after: true, estimated_tokens: 15000, preferred_model: null },
    { id: ids[2], name: 'Build UI layer', description: 'Frontend app', role: 'implementer', execution_mode: 'subagent', depends_on: [ids[0]], verify_after: true, estimated_tokens: 12000, preferred_model: null },
    { id: ids[3], name: 'Final verification', description: 'Full test suite', role: 'verifier', execution_mode: 'inline', depends_on: [ids[1], ids[2]], verify_after: true, estimated_tokens: 5000, preferred_model: null },
  ]

  const events: DashboardEvent[] = [
    evt(0, 0, { type: 'run_started', provider: 'claude-code', model: 'opus', request_summary: 'Build full-stack feature with sub-agent decomposition' }),
    // Workspace indexing — OCO scans the repo and builds its symbol index
    // before deciding how to orchestrate. Each tick streams one more file.
    evt(50, 0, { type: 'index_progress', files_done: 3,  symbols_so_far: 48 }),
    evt(60, 0, { type: 'index_progress', files_done: 7,  symbols_so_far: 142 }),
    evt(70, 0, { type: 'index_progress', files_done: 12, symbols_so_far: 318 }),
    evt(80, 0, { type: 'index_progress', files_done: 18, symbols_so_far: 512 }),
    evt(90, 0, { type: 'index_progress', files_done: 24, symbols_so_far: 847 }),
    evt(100, 0, { type: 'plan_exploration', candidates: [
      { strategy: 'monolithic', step_count: 2, estimated_tokens: 28000, score: 0.58, strengths: ['Simple'], weaknesses: ['No decomposition', 'Slow serial'] },
      { strategy: 'hierarchical', step_count: 4, estimated_tokens: 34000, score: 0.86, strengths: ['Parallel subagents', 'Sub-plan visibility'], weaknesses: ['Slightly higher tokens'] },
    ], winner_strategy: 'hierarchical', winner_score: 0.86 }),
    evt(200, 1, { type: 'plan_generated', plan_id: 'p-hier', step_count: 4, parallel_group_count: 2, critical_path_length: 3, estimated_total_tokens: 34000, strategy: 'hierarchical', team: null, steps }),
    // Scout
    evt(1000, 1, { type: 'step_started', step_id: ids[0], step_name: 'Analyze requirements', role: 'scout', execution_mode: 'inline' }),
    // Scout reads key files to understand the project scope.
    evt(1200, 1, { type: 'tool_call_started',   step_id: ids[0], tool_name: 'Read', args_summary: 'README.md' }),
    evt(1350, 1, { type: 'tool_call_completed', step_id: ids[0], tool_name: 'Read', success: true, duration_ms: 150, output_summary: '152 lines' }),
    evt(1400, 1, { type: 'tool_call_started',   step_id: ids[0], tool_name: 'Grep', args_summary: 'endpoint' }),
    evt(1700, 1, { type: 'tool_call_completed', step_id: ids[0], tool_name: 'Grep', success: true, duration_ms: 300, output_summary: '12 matches · 5 files' }),
    evt(1800, 1, { type: 'tool_call_started',   step_id: ids[0], tool_name: 'Read', args_summary: 'crates/shared-types/src/lib.rs' }),
    evt(2100, 1, { type: 'tool_call_completed', step_id: ids[0], tool_name: 'Read', success: true, duration_ms: 300, output_summary: '847 lines' }),
    evt(2200, 1, { type: 'tool_call_started',   step_id: ids[0], tool_name: 'Glob', args_summary: 'src/**/*.ts' }),
    evt(2400, 1, { type: 'tool_call_completed', step_id: ids[0], tool_name: 'Glob', success: true, duration_ms: 200, output_summary: '47 files' }),
    evt(3000, 1, { type: 'step_completed', step_id: ids[0], step_name: 'Analyze requirements', success: true, duration_ms: 2000, tokens_used: 1800, detail_ref: null }),
    evt(3050, 1, { type: 'budget_snapshot', tokens_used: 1800,  tokens_remaining: 198200, tool_calls_used: 4,  tool_calls_remaining: 96, retrievals_used: 3, verify_cycles_used: 0, elapsed_secs: 3 }),
    // Two subagents start in parallel at the EXACT same instant so both
    // enter `running` in the same event-loop tick. Staggering them by even
    // 200ms made it look like API booted before UI.
    evt(3500, 1, { type: 'step_started', step_id: ids[1], step_name: 'Build API layer', role: 'implementer', execution_mode: 'subagent' }),
    evt(3500, 1, { type: 'step_started', step_id: ids[2], step_name: 'Build UI layer', role: 'implementer', execution_mode: 'subagent' }),
    // Both sub-plans start simultaneously: no window where only one node
    // has its sub-steps filled in while the other is still empty.
    evt(4000, 1, { type: 'sub_plan_started', parent_step_id: ids[1], parent_step_name: 'Build API layer', sub_steps: [
      { id: subIds[0], name: 'Scaffold routes' },
      { id: subIds[1], name: 'Implement handlers' },
      { id: subIds[2], name: 'Write API tests' },
    ] }),
    evt(4000, 1, { type: 'sub_plan_started', parent_step_id: ids[2], parent_step_name: 'Build UI layer', sub_steps: [
      { id: subIds[3], name: 'Create components' },
      { id: subIds[4], name: 'Style views' },
      { id: subIds[5], name: 'Bind state' },
    ] }),
    // Sub-plans execute in TRUE parallel: each pair of sub-steps progresses
    // and completes within a ~50ms window, so the user visually perceives
    // both subagents advancing in lockstep.
    //
    // Sub-plan A (API layer) timings are paired with Sub-plan B (UI layer):
    //   Pair 1 runs:  t=4500 (A.step1) & t=4550 (B.step1)
    //   Pair 1 done:  t=5600 (A.step1) & t=5650 (B.step1)
    //   Pair 2 runs:  t=5800 (A.step2) & t=5850 (B.step2)
    //   Pair 2 done:  t=7600 (A.step2) & t=7650 (B.step2)
    //   Pair 3 runs:  t=7800 (A.step3) & t=7850 (B.step3)
    //   Pair 3 done:  t=9100 (A.step3) & t=9150 (B.step3)
    //   sub_plan_completed:  t=9300 (A) & t=9350 (B)
    //   step_completed:      t=9500 (A) & t=9550 (B)
    evt(4500, 1, { type: 'sub_step_progress', parent_step_id: ids[1], sub_step_id: subIds[0], sub_step_name: 'Scaffold routes', status: 'running' }),
    evt(4550, 1, { type: 'sub_step_progress', parent_step_id: ids[2], sub_step_id: subIds[3], sub_step_name: 'Create components', status: 'running' }),
    // API: Scaffold routes — Edit routes file + Bash check
    evt(4700, 1, { type: 'tool_call_started',   step_id: ids[1], tool_name: 'Edit', args_summary: 'src/api/routes.ts' }),
    evt(4900, 1, { type: 'tool_call_completed', step_id: ids[1], tool_name: 'Edit', success: true, duration_ms: 200, output_summary: '45 lines added' }),
    evt(4910, 1, { type: 'file_changed', step_id: ids[1], path: 'src/api/routes.ts', change_type: 'modified', lines_added: 45, lines_removed: 3 }),
    // UI: Create components — create App.svelte
    evt(4800, 1, { type: 'tool_call_started',   step_id: ids[2], tool_name: 'Write', args_summary: 'src/ui/App.svelte' }),
    evt(5100, 1, { type: 'tool_call_completed', step_id: ids[2], tool_name: 'Write', success: true, duration_ms: 300, output_summary: '54 lines' }),
    evt(5110, 1, { type: 'file_changed', step_id: ids[2], path: 'src/ui/App.svelte', change_type: 'created', lines_added: 54, lines_removed: 0 }),
    evt(5300, 1, { type: 'tool_call_started',   step_id: ids[1], tool_name: 'Bash', args_summary: 'cargo check -p api' }),
    evt(5550, 1, { type: 'tool_call_completed', step_id: ids[1], tool_name: 'Bash', success: true, duration_ms: 250, output_summary: '0 errors' }),
    evt(5600, 1, { type: 'sub_step_progress', parent_step_id: ids[1], sub_step_id: subIds[0], sub_step_name: 'Scaffold routes', status: 'passed' }),
    evt(5650, 1, { type: 'sub_step_progress', parent_step_id: ids[2], sub_step_id: subIds[3], sub_step_name: 'Create components', status: 'passed' }),
    evt(5800, 1, { type: 'sub_step_progress', parent_step_id: ids[1], sub_step_id: subIds[1], sub_step_name: 'Implement handlers', status: 'running' }),
    evt(5850, 1, { type: 'sub_step_progress', parent_step_id: ids[2], sub_step_id: subIds[4], sub_step_name: 'Style views', status: 'running' }),
    // API: Implement handlers — Read types, Edit handlers + middleware
    evt(6000, 1, { type: 'tool_call_started',   step_id: ids[1], tool_name: 'Read', args_summary: 'src/api/types.ts' }),
    evt(6200, 1, { type: 'tool_call_completed', step_id: ids[1], tool_name: 'Read', success: true, duration_ms: 200, output_summary: '89 lines' }),
    evt(6300, 1, { type: 'tool_call_started',   step_id: ids[1], tool_name: 'Edit', args_summary: 'src/api/handlers.ts' }),
    evt(6700, 1, { type: 'tool_call_completed', step_id: ids[1], tool_name: 'Edit', success: true, duration_ms: 400, output_summary: '180 lines added' }),
    evt(6710, 1, { type: 'file_changed', step_id: ids[1], path: 'src/api/handlers.ts', change_type: 'modified', lines_added: 180, lines_removed: 12 }),
    evt(6800, 1, { type: 'tool_call_started',   step_id: ids[1], tool_name: 'Write', args_summary: 'src/api/middleware.ts' }),
    evt(7000, 1, { type: 'tool_call_completed', step_id: ids[1], tool_name: 'Write', success: true, duration_ms: 200, output_summary: '62 lines' }),
    evt(7010, 1, { type: 'file_changed', step_id: ids[1], path: 'src/api/middleware.ts', change_type: 'created', lines_added: 62, lines_removed: 0 }),
    // UI: Style views — Edit App + create styles.css
    evt(6100, 1, { type: 'tool_call_started',   step_id: ids[2], tool_name: 'Edit', args_summary: 'src/ui/App.svelte' }),
    evt(6400, 1, { type: 'tool_call_completed', step_id: ids[2], tool_name: 'Edit', success: true, duration_ms: 300, output_summary: '23 lines added' }),
    evt(6410, 1, { type: 'file_changed', step_id: ids[2], path: 'src/ui/App.svelte', change_type: 'modified', lines_added: 23, lines_removed: 5 }),
    evt(6500, 1, { type: 'tool_call_started',   step_id: ids[2], tool_name: 'Write', args_summary: 'src/ui/styles.css' }),
    evt(6900, 1, { type: 'tool_call_completed', step_id: ids[2], tool_name: 'Write', success: true, duration_ms: 400, output_summary: '120 lines' }),
    evt(6910, 1, { type: 'file_changed', step_id: ids[2], path: 'src/ui/styles.css', change_type: 'created', lines_added: 120, lines_removed: 0 }),
    evt(7600, 1, { type: 'sub_step_progress', parent_step_id: ids[1], sub_step_id: subIds[1], sub_step_name: 'Implement handlers', status: 'passed' }),
    evt(7650, 1, { type: 'sub_step_progress', parent_step_id: ids[2], sub_step_id: subIds[4], sub_step_name: 'Style views', status: 'passed' }),
    evt(7800, 1, { type: 'sub_step_progress', parent_step_id: ids[1], sub_step_id: subIds[2], sub_step_name: 'Write API tests', status: 'running' }),
    evt(7850, 1, { type: 'sub_step_progress', parent_step_id: ids[2], sub_step_id: subIds[5], sub_step_name: 'Bind state', status: 'running' }),
    // API: Write API tests
    evt(8000, 1, { type: 'tool_call_started',   step_id: ids[1], tool_name: 'Write', args_summary: 'tests/api.test.ts' }),
    evt(8300, 1, { type: 'tool_call_completed', step_id: ids[1], tool_name: 'Write', success: true, duration_ms: 300, output_summary: '90 lines' }),
    evt(8310, 1, { type: 'file_changed', step_id: ids[1], path: 'tests/api.test.ts', change_type: 'created', lines_added: 90, lines_removed: 0 }),
    evt(8400, 1, { type: 'tool_call_started',   step_id: ids[1], tool_name: 'Bash', args_summary: 'pnpm test api' }),
    evt(8900, 1, { type: 'tool_call_completed', step_id: ids[1], tool_name: 'Bash', success: true, duration_ms: 500, output_summary: '12 passed' }),
    // UI: Bind state
    evt(8100, 1, { type: 'tool_call_started',   step_id: ids[2], tool_name: 'Write', args_summary: 'src/ui/store.ts' }),
    evt(8500, 1, { type: 'tool_call_completed', step_id: ids[2], tool_name: 'Write', success: true, duration_ms: 400, output_summary: '85 lines' }),
    evt(8510, 1, { type: 'file_changed', step_id: ids[2], path: 'src/ui/store.ts', change_type: 'created', lines_added: 85, lines_removed: 0 }),
    evt(9100, 1, { type: 'sub_step_progress', parent_step_id: ids[1], sub_step_id: subIds[2], sub_step_name: 'Write API tests', status: 'passed' }),
    evt(9150, 1, { type: 'sub_step_progress', parent_step_id: ids[2], sub_step_id: subIds[5], sub_step_name: 'Bind state', status: 'passed' }),
    evt(9300, 1, { type: 'sub_plan_completed', parent_step_id: ids[1], parent_step_name: 'Build API layer', success: true }),
    evt(9350, 1, { type: 'sub_plan_completed', parent_step_id: ids[2], parent_step_name: 'Build UI layer', success: true }),
    evt(9400, 1, { type: 'budget_snapshot', tokens_used: 27300, tokens_remaining: 172700, tool_calls_used: 26, tool_calls_remaining: 74, retrievals_used: 3, verify_cycles_used: 0, elapsed_secs: 9 }),
    // Parent steps complete — also paired in lockstep
    evt(9500, 1, { type: 'step_completed', step_id: ids[1], step_name: 'Build API layer', success: true, duration_ms: 6000, tokens_used: 14000, detail_ref: null }),
    evt(9550, 1, { type: 'step_completed', step_id: ids[2], step_name: 'Build UI layer', success: true, duration_ms: 5850, tokens_used: 11500, detail_ref: null }),
    evt(9700, 1, { type: 'verify_gate_result', step_id: ids[1], step_name: 'Build API layer', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '12 tests' }], overall_passed: true, replan_triggered: false }),
    evt(9750, 1, { type: 'verify_gate_result', step_id: ids[2], step_name: 'Build UI layer', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '8 tests' }], overall_passed: true, replan_triggered: false }),
    // Final verification
    evt(10000, 1, { type: 'step_started', step_id: ids[3], step_name: 'Final verification', role: 'verifier', execution_mode: 'inline' }),
    evt(10100, 1, { type: 'tool_call_started',   step_id: ids[3], tool_name: 'Bash', args_summary: 'pnpm test' }),
    evt(10800, 1, { type: 'tool_call_completed', step_id: ids[3], tool_name: 'Bash', success: true, duration_ms: 700, output_summary: '20 passed' }),
    evt(10900, 1, { type: 'tool_call_started',   step_id: ids[3], tool_name: 'Bash', args_summary: 'pnpm build' }),
    evt(11500, 1, { type: 'tool_call_completed', step_id: ids[3], tool_name: 'Bash', success: true, duration_ms: 600, output_summary: 'built in 0.6s' }),
    evt(11600, 1, { type: 'step_completed', step_id: ids[3], step_name: 'Final verification', success: true, duration_ms: 1600, tokens_used: 4500, detail_ref: null }),
    evt(11650, 1, { type: 'budget_snapshot', tokens_used: 31800, tokens_remaining: 168200, tool_calls_used: 30, tool_calls_remaining: 70, retrievals_used: 3, verify_cycles_used: 3, elapsed_secs: 12 }),
    evt(11800, 1, { type: 'verify_gate_result', step_id: ids[3], step_name: 'Final verification', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '20 tests' }, { check_type: 'lint', passed: true, summary: '0 warnings' }], overall_passed: true, replan_triggered: false }),
    evt(12100, 1, { type: 'run_stopped', reason: { type: 'task_complete' }, total_steps: 4, total_tokens: 31800 }),
  ]

  const explorationPlans = {
    loser: {
      strategy: 'monolithic',
      steps: [
        { name: 'Analyze requirements', role: 'scout', verify: false, tokens: 3000, depends_on: [] },
        { name: 'Build full-stack', role: 'implementer', verify: true, tokens: 25000, depends_on: ['Analyze requirements'] },
      ],
      score: 0.58,
      winner: false,
      scores: { verify: 0.50, cost: 0.55, parallel: 0.00, depth: 0.20 },
    },
    winner: {
      strategy: 'hierarchical',
      steps: [
        { name: 'Analyze requirements', role: 'scout', verify: false, tokens: 2000, depends_on: [] },
        { name: 'Build API layer', role: 'implementer', verify: true, tokens: 15000, depends_on: ['Analyze requirements'] },
        { name: 'Build UI layer', role: 'implementer', verify: true, tokens: 12000, depends_on: ['Analyze requirements'] },
        { name: 'Final verification', role: 'verifier', verify: true, tokens: 5000, depends_on: ['Build API layer', 'Build UI layer'] },
      ],
      score: 0.86,
      winner: true,
      scores: { verify: 0.75, cost: 0.50, parallel: 0.65, depth: 0.55 },
    },
  }
  return { id: 'hierarchical', name: 'Hierarchical (sub-plans)', description: 'Two subagents with internal sub-plan decomposition (3 sub-steps each)', steps, events, thoughts: [], explorationPlans }
}

// ── 7. Verify failure + Replan ───────────────────────────────

function buildReplan(): PlaygroundScenario {
  resetSeq()
  const ids = [uid('rpl', 1), uid('rpl', 2), uid('rpl', 3)]
  // After replan: step 2 is removed, replaced by 2' and 3'
  const replanIds = [uid('rpl', 4), uid('rpl', 5)]

  const steps: StepSummary[] = [
    { id: ids[0], name: 'Investigate auth', description: 'Scan auth module', role: 'scout', execution_mode: 'inline', depends_on: [], verify_after: false, estimated_tokens: 3000, preferred_model: null },
    { id: ids[1], name: 'Patch handler', description: 'Fix session handler', role: 'implementer', execution_mode: 'inline', depends_on: [ids[0]], verify_after: true, estimated_tokens: 8000, preferred_model: null },
    { id: ids[2], name: 'Final verify', description: 'Run full test suite', role: 'verifier', execution_mode: 'inline', depends_on: [ids[1]], verify_after: true, estimated_tokens: 4000, preferred_model: null },
  ]

  const stepsAfterReplan: StepSummary[] = [
    ...steps,
    { id: replanIds[0], name: 'Fix session type', description: 'Change session.user_id to String', role: 'implementer', execution_mode: 'inline', depends_on: [ids[0]], verify_after: true, estimated_tokens: 6000, preferred_model: null },
    { id: replanIds[1], name: 'Re-verify', description: 'Run tests after fix', role: 'verifier', execution_mode: 'inline', depends_on: [replanIds[0]], verify_after: true, estimated_tokens: 4000, preferred_model: null },
  ]

  const events: DashboardEvent[] = [
    evt(0, 0, { type: 'run_started', provider: 'claude-code', model: 'sonnet', request_summary: 'Fix session auth handler type mismatch' }),
    evt(100, 0, { type: 'plan_exploration', candidates: [
      { strategy: 'hotfix', step_count: 3, estimated_tokens: 15000, score: 0.71, strengths: ['Fast patch'], weaknesses: ['No root-cause'] },
      { strategy: 'defensive', step_count: 5, estimated_tokens: 22000, score: 0.68, strengths: ['More tests'], weaknesses: ['Slower'] },
    ], winner_strategy: 'hotfix', winner_score: 0.71 }),
    evt(200, 1, { type: 'plan_generated', plan_id: 'p-rpl', step_count: 3, parallel_group_count: 1, critical_path_length: 3, estimated_total_tokens: 15000, strategy: 'linear', team: null, steps }),
    // Step 1: investigate
    evt(1000, 1, { type: 'step_started', step_id: ids[0], step_name: 'Investigate auth', role: 'scout', execution_mode: 'inline' }),
    evt(3000, 1, { type: 'step_completed', step_id: ids[0], step_name: 'Investigate auth', success: true, duration_ms: 2000, tokens_used: 2500, detail_ref: null }),
    evt(3200, 1, { type: 'progress', completed: 1, total: 3, active_steps: [], budget: budgetAt(2500, 1, 3) }),
    // Step 2: patch — succeeds but verify fails
    evt(3500, 1, { type: 'step_started', step_id: ids[1], step_name: 'Patch handler', role: 'implementer', execution_mode: 'inline' }),
    evt(8000, 1, { type: 'step_completed', step_id: ids[1], step_name: 'Patch handler', success: true, duration_ms: 4500, tokens_used: 7200, detail_ref: null }),
    evt(8500, 1, { type: 'verify_gate_result', step_id: ids[1], step_name: 'Patch handler', checks: [
      { check_type: 'build', passed: true, summary: '0 errors' },
      { check_type: 'test', passed: false, summary: '2 failures in auth_test.rs: expected String, got i32' },
    ], overall_passed: false, replan_triggered: true }),
    evt(8700, 1, { type: 'progress', completed: 1, total: 3, active_steps: [], budget: budgetAt(9700, 3, 9) }),
    // Replan triggered
    evt(9000, 2, { type: 'replan_triggered', failed_step_name: 'Patch handler', attempt: 1, max_attempts: 3, steps_preserved: 1, steps_removed: 2, steps_added: 2 }),
    evt(10000, 2, { type: 'plan_generated', plan_id: 'p-rpl-v2', step_count: 4, parallel_group_count: 1, critical_path_length: 3, estimated_total_tokens: 13000, strategy: 'linear (replan v2)', team: null, steps: stepsAfterReplan }),
    // New step: fix session type
    evt(10500, 2, { type: 'step_started', step_id: replanIds[0], step_name: 'Fix session type', role: 'implementer', execution_mode: 'inline' }),
    evt(14000, 2, { type: 'step_completed', step_id: replanIds[0], step_name: 'Fix session type', success: true, duration_ms: 3500, tokens_used: 5500, detail_ref: null }),
    evt(14500, 2, { type: 'verify_gate_result', step_id: replanIds[0], step_name: 'Fix session type', checks: [
      { check_type: 'build', passed: true, summary: '0 errors' },
      { check_type: 'test', passed: true, summary: '8 tests passed' },
    ], overall_passed: true, replan_triggered: false }),
    evt(14700, 2, { type: 'progress', completed: 3, total: 4, active_steps: [], budget: budgetAt(17700, 6, 15) }),
    // Re-verify
    evt(15000, 2, { type: 'step_started', step_id: replanIds[1], step_name: 'Re-verify', role: 'verifier', execution_mode: 'inline' }),
    evt(18000, 2, { type: 'step_completed', step_id: replanIds[1], step_name: 'Re-verify', success: true, duration_ms: 3000, tokens_used: 3800, detail_ref: null }),
    evt(18500, 2, { type: 'verify_gate_result', step_id: replanIds[1], step_name: 'Re-verify', checks: [
      { check_type: 'build', passed: true, summary: '0 errors' },
      { check_type: 'test', passed: true, summary: '12 tests passed' },
      { check_type: 'lint', passed: true, summary: '0 warnings' },
    ], overall_passed: true, replan_triggered: false }),
    evt(18700, 2, { type: 'progress', completed: 4, total: 4, active_steps: [], budget: budgetAt(21500, 8, 19) }),
    evt(19000, 2, { type: 'run_stopped', reason: { type: 'task_complete' }, total_steps: 4, total_tokens: 21500 }),
  ]

  const explorationPlans = {
    loser: {
      strategy: 'defensive',
      steps: [
        { name: 'Investigate', role: 'scout', verify: false, tokens: 3000, depends_on: [] },
        { name: 'Root cause', role: 'architect', verify: false, tokens: 4000, depends_on: ['Investigate'] },
        { name: 'Rewrite handler', role: 'implementer', verify: true, tokens: 10000, depends_on: ['Root cause'] },
        { name: 'Expanded test suite', role: 'tester', verify: true, tokens: 5000, depends_on: ['Rewrite handler'] },
      ],
      score: 0.68,
      winner: false,
      scores: { verify: 0.75, cost: 0.50, parallel: 0.00, depth: 0.35 },
    },
    winner: {
      strategy: 'hotfix',
      steps: [
        { name: 'Investigate auth', role: 'scout', verify: false, tokens: 3000, depends_on: [] },
        { name: 'Patch handler', role: 'implementer', verify: true, tokens: 8000, depends_on: ['Investigate auth'] },
        { name: 'Final verify', role: 'verifier', verify: true, tokens: 4000, depends_on: ['Patch handler'] },
      ],
      score: 0.71,
      winner: true,
      scores: { verify: 0.70, cost: 0.70, parallel: 0.00, depth: 0.10 },
    },
  }
  return { id: 'replan', name: 'Verify + Replan', description: 'Verify gate fails → replan with different approach → success', steps, events, thoughts: [], explorationPlans }
}

// ── 8. Trivial fast-exit ────────────────────────────────────

function buildTrivial(): PlaygroundScenario {
  resetSeq()
  const ids = [uid('trv', 1), uid('trv', 2)]

  const steps: StepSummary[] = [
    { id: ids[0], name: 'Read config', description: 'Inspect tsconfig.json', role: 'scout', execution_mode: 'inline', depends_on: [], verify_after: false, estimated_tokens: 1500, preferred_model: null },
    { id: ids[1], name: 'Fix strict flag', description: 'Set strict: true in tsconfig', role: 'implementer', execution_mode: 'inline', depends_on: [ids[0]], verify_after: false, estimated_tokens: 2000, preferred_model: null },
  ]

  const events: DashboardEvent[] = [
    evt(0, 0, { type: 'run_started', provider: 'claude-code', model: 'sonnet', request_summary: 'Enable strict mode in tsconfig.json' }),
    evt(200, 1, { type: 'plan_generated', plan_id: 'p-trv', step_count: 2, parallel_group_count: 1, critical_path_length: 2, estimated_total_tokens: 3500, strategy: 'direct (fast-exit)', team: null, steps }),
    evt(500, 1, { type: 'step_started', step_id: ids[0], step_name: 'Read config', role: 'scout', execution_mode: 'inline' }),
    evt(1500, 1, { type: 'step_completed', step_id: ids[0], step_name: 'Read config', success: true, duration_ms: 1000, tokens_used: 1200, detail_ref: null }),
    evt(1600, 1, { type: 'progress', completed: 1, total: 2, active_steps: [], budget: budgetAt(1200, 1, 2) }),
    evt(1800, 1, { type: 'step_started', step_id: ids[1], step_name: 'Fix strict flag', role: 'implementer', execution_mode: 'inline' }),
    evt(3000, 1, { type: 'step_completed', step_id: ids[1], step_name: 'Fix strict flag', success: true, duration_ms: 1200, tokens_used: 1800, detail_ref: null }),
    evt(3200, 1, { type: 'progress', completed: 2, total: 2, active_steps: [], budget: budgetAt(3000, 2, 3) }),
    evt(3500, 1, { type: 'run_stopped', reason: { type: 'task_complete' }, total_steps: 2, total_tokens: 3000 }),
  ]

  return { id: 'trivial', name: 'Trivial (fast-exit)', description: '2 inline steps, no verify, no parallel — skips GraphRunner', steps, events, thoughts: [] }
}

// ── 9. Prior Art Research (parallel research → synthesis → implement) ──

function buildPriorArt(): PlaygroundScenario {
  resetSeq()
  const ids = [uid('pa', 1), uid('pa', 2), uid('pa', 3), uid('pa', 4), uid('pa', 5), uid('pa', 6), uid('pa', 7)]

  const steps: StepSummary[] = [
    { id: ids[0], name: 'Search OSS solutions', description: 'Find existing libraries on crates.io, npm, pypi', role: 'researcher', execution_mode: 'subagent', depends_on: [], verify_after: false, estimated_tokens: 2000, preferred_model: 'haiku' },
    { id: ids[1], name: 'Search research papers', description: 'Check arXiv/Scholar for recent papers on the topic', role: 'researcher', execution_mode: 'subagent', depends_on: [], verify_after: false, estimated_tokens: 1500, preferred_model: 'haiku' },
    { id: ids[2], name: 'Synthesize findings', description: 'Evaluate prior art and recommend build vs adopt', role: 'analyst', execution_mode: 'inline', depends_on: [ids[0], ids[1]], verify_after: false, estimated_tokens: 2500, preferred_model: null },
    { id: ids[3], name: 'Design architecture', description: 'Design informed by research findings', role: 'architect', execution_mode: 'inline', depends_on: [ids[2]], verify_after: false, estimated_tokens: 5000, preferred_model: 'opus' },
    { id: ids[4], name: 'Implement core', description: 'Core implementation using recommended library', role: 'implementer', execution_mode: 'inline', depends_on: [ids[3]], verify_after: true, estimated_tokens: 12000, preferred_model: null },
    { id: ids[5], name: 'Implement integration', description: 'Wire into existing codebase', role: 'implementer', execution_mode: 'inline', depends_on: [ids[3]], verify_after: true, estimated_tokens: 8000, preferred_model: null },
    { id: ids[6], name: 'Test suite', description: 'Unit + integration tests', role: 'tester', execution_mode: 'inline', depends_on: [ids[4], ids[5]], verify_after: true, estimated_tokens: 7000, preferred_model: null },
  ]

  const events: DashboardEvent[] = [
    evt(0, 0, { type: 'run_started', provider: 'claude-code', model: 'sonnet', request_summary: 'Add vector similarity search with embedding support' }),
    evt(200, 0, { type: 'plan_exploration', candidates: [
      { strategy: 'speed', step_count: 4, estimated_tokens: 28000, score: 0.58, strengths: ['Fast', 'Simple'], weaknesses: ['May reinvent the wheel', 'No research phase'] },
      { strategy: 'research-first', step_count: 7, estimated_tokens: 38000, score: 0.84, strengths: ['Prior art search', 'Informed decisions', 'Parallel research'], weaknesses: ['Extra steps'] },
    ], winner_strategy: 'research-first', winner_score: 0.84 }),
    evt(10000, 1, { type: 'plan_generated', plan_id: 'p-pa', step_count: 7, parallel_group_count: 4, critical_path_length: 5, estimated_total_tokens: 38000, strategy: 'research-first (prior art)', team: null, steps }),
    // Parallel research subagents
    evt(11000, 1, { type: 'step_started', step_id: ids[0], step_name: 'Search OSS solutions', role: 'researcher', execution_mode: 'subagent' }),
    evt(11200, 1, { type: 'step_started', step_id: ids[1], step_name: 'Search research papers', role: 'researcher', execution_mode: 'subagent' }),
    evt(13000, 1, { type: 'step_completed', step_id: ids[1], step_name: 'Search research papers', success: true, duration_ms: 1800, tokens_used: 1200, detail_ref: null }),
    evt(13500, 1, { type: 'step_completed', step_id: ids[0], step_name: 'Search OSS solutions', success: true, duration_ms: 2500, tokens_used: 1800, detail_ref: null }),
    evt(13700, 1, { type: 'progress', completed: 2, total: 7, active_steps: [], budget: budgetAt(3000, 4, 5) }),
    // Synthesis
    evt(14000, 1, { type: 'step_started', step_id: ids[2], step_name: 'Synthesize findings', role: 'analyst', execution_mode: 'inline' }),
    evt(16000, 1, { type: 'step_completed', step_id: ids[2], step_name: 'Synthesize findings', success: true, duration_ms: 2000, tokens_used: 2200, detail_ref: null }),
    evt(16200, 1, { type: 'progress', completed: 3, total: 7, active_steps: [], budget: budgetAt(5200, 6, 8) }),
    // Design
    evt(16500, 1, { type: 'step_started', step_id: ids[3], step_name: 'Design architecture', role: 'architect', execution_mode: 'inline' }),
    evt(19500, 1, { type: 'step_completed', step_id: ids[3], step_name: 'Design architecture', success: true, duration_ms: 3000, tokens_used: 4600, detail_ref: null }),
    evt(19700, 1, { type: 'progress', completed: 4, total: 7, active_steps: [], budget: budgetAt(9800, 8, 12) }),
    // Parallel implementation
    evt(20000, 1, { type: 'step_started', step_id: ids[4], step_name: 'Implement core', role: 'implementer', execution_mode: 'inline' }),
    evt(20200, 1, { type: 'step_started', step_id: ids[5], step_name: 'Implement integration', role: 'implementer', execution_mode: 'inline' }),
    evt(26500, 1, { type: 'step_completed', step_id: ids[4], step_name: 'Implement core', success: true, duration_ms: 6500, tokens_used: 11500, detail_ref: null }),
    evt(27000, 1, { type: 'verify_gate_result', step_id: ids[4], step_name: 'Implement core', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '8 tests passed' }], overall_passed: true, replan_triggered: false }),
    evt(28000, 1, { type: 'step_completed', step_id: ids[5], step_name: 'Implement integration', success: true, duration_ms: 7800, tokens_used: 7500, detail_ref: null }),
    evt(28500, 1, { type: 'verify_gate_result', step_id: ids[5], step_name: 'Implement integration', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '5 tests passed' }], overall_passed: true, replan_triggered: false }),
    evt(28700, 1, { type: 'progress', completed: 6, total: 7, active_steps: [], budget: budgetAt(31000, 20, 20) }),
    // Tests
    evt(29000, 1, { type: 'step_started', step_id: ids[6], step_name: 'Test suite', role: 'tester', execution_mode: 'inline' }),
    evt(34000, 1, { type: 'step_completed', step_id: ids[6], step_name: 'Test suite', success: true, duration_ms: 5000, tokens_used: 6800, detail_ref: null }),
    evt(34500, 1, { type: 'verify_gate_result', step_id: ids[6], step_name: 'Test suite', checks: [{ check_type: 'build', passed: true, summary: '0 errors' }, { check_type: 'test', passed: true, summary: '15 tests passed' }, { check_type: 'lint', passed: true, summary: '0 warnings' }], overall_passed: true, replan_triggered: false }),
    evt(35000, 1, { type: 'run_stopped', reason: { type: 'task_complete' }, total_steps: 7, total_tokens: 35600 }),
  ]

  const thoughts: Thought[] = [
    { text: 'Searching crates.io: "vector similarity", "embedding search"...', variant: 'action', stepId: ids[0], offsetMs: 11500 },
    { text: 'Found: hnsw (12k stars), usearch, qdrant-client, lance', variant: 'thought', stepId: ids[0], offsetMs: 12500 },
    { text: 'Checking arXiv for ANN search algorithms 2024-2025...', variant: 'action', stepId: ids[1], offsetMs: 11800 },
    { text: 'Paper: "Graph-based ANN with product quantization" (2024)', variant: 'thought', stepId: ids[1], offsetMs: 12800 },
    { text: 'hnsw crate covers exact + approximate search — adopt it', variant: 'success', stepId: ids[2], offsetMs: 14800 },
    { text: 'Recommendation: use hnsw for indexing, cosine similarity metric', variant: 'thought', stepId: ids[2], offsetMs: 15500 },
    { text: 'Designing index schema with hnsw EfConstruction=200, M=16', variant: 'action', stepId: ids[3], offsetMs: 17500 },
    { text: 'Building VectorIndex wrapper around hnsw crate', variant: 'action', stepId: ids[4], offsetMs: 22000 },
    { text: 'Wiring similarity search into retrieval pipeline', variant: 'action', stepId: ids[5], offsetMs: 23000 },
    { text: 'All 8 core tests passing', variant: 'success', stepId: ids[4], offsetMs: 26800 },
    { text: 'Testing end-to-end: index 1k docs → query → top-5 results', variant: 'action', stepId: ids[6], offsetMs: 31000 },
    { text: '15 tests passed — all clear', variant: 'success', stepId: ids[6], offsetMs: 34200 },
  ]

  const explorationPlans = {
    loser: {
      strategy: 'speed',
      steps: [
        { name: 'Design & implement', role: 'architect', verify: false, tokens: 8000, depends_on: [] },
        { name: 'Build from scratch', role: 'implementer', verify: true, tokens: 18000, depends_on: ['Design & implement'] },
        { name: 'Quick test', role: 'tester', verify: true, tokens: 4000, depends_on: ['Build from scratch'] },
      ],
      score: 0.58,
      winner: false,
      scores: { verify: 0.55, cost: 0.60, parallel: 0.00, depth: 0.00 },
    },
    winner: {
      strategy: 'research-first',
      steps: [
        { name: 'Search OSS', role: 'researcher', verify: false, tokens: 2000, depends_on: [] },
        { name: 'Search papers', role: 'researcher', verify: false, tokens: 1500, depends_on: [] },
        { name: 'Synthesize', role: 'analyst', verify: false, tokens: 2500, depends_on: ['Search OSS', 'Search papers'] },
        { name: 'Design', role: 'architect', verify: false, tokens: 5000, depends_on: ['Synthesize'] },
        { name: 'Core impl', role: 'implementer', verify: true, tokens: 12000, depends_on: ['Design'] },
        { name: 'Integration', role: 'implementer', verify: true, tokens: 8000, depends_on: ['Design'] },
        { name: 'Test suite', role: 'tester', verify: true, tokens: 7000, depends_on: ['Core impl', 'Integration'] },
      ],
      score: 0.84,
      winner: true,
      scores: { verify: 0.70, cost: 0.35, parallel: 0.55, depth: 0.40 },
    },
  }
  return { id: 'prior-art', name: 'Prior Art Research', description: 'Parallel OSS + paper research → synthesis → informed implementation', steps, events, thoughts, explorationPlans }
}

// ── Export all scenarios ──────────────────────────────────────

export const SCENARIOS: PlaygroundScenario[] = [
  buildLinear(),
  buildParallel(),
  buildPriorArt(),
  buildTeam(),
  buildComplex(),
  buildTeamComm(),
  buildHierarchical(),
  buildReplan(),
  buildTrivial(),
]

export function getScenario(id: string): PlaygroundScenario {
  return SCENARIOS.find(s => s.id === id) ?? SCENARIOS[0]
}
