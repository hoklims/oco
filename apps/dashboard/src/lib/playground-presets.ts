/**
 * Playground presets — named configurations for every post-run component.
 *
 * Each preset is a complete, self-contained data object that can be
 * plugged directly into the component. Presets cover happy paths,
 * edge cases, failures, and boundary conditions.
 */

import type {
  RunScorecard, GateResult, GateVerdict, MissionMemory,
  ReviewPacket, MergeReadiness, TrustVerdict, BaselineFreshness,
  BaselineFreshnessCheck, ScorecardDimension, DimensionScore,
} from './types'

// ── Helpers ──────────────────────────────────────────────────

const now = () => new Date().toISOString()
const rid = 'preset-run-0001'

function dims(values: Record<ScorecardDimension, [number, string]>): DimensionScore[] {
  return (Object.entries(values) as [ScorecardDimension, [number, string]][]).map(
    ([dimension, [score, detail]]) => ({ dimension, score, detail })
  )
}

// ��─ Scorecard Presets ────────────────────────────────────────

export interface ScorecardPreset { id: string; name: string; description: string; data: RunScorecard }

export const SCORECARD_PRESETS: ScorecardPreset[] = [
  {
    id: 'perfect', name: 'Perfect Run', description: 'All dimensions above 90%',
    data: {
      run_id: rid, computed_at: now(), overall_score: 0.95,
      dimensions: dims({
        Success: [0.98, 'All objectives met'],
        TrustVerdict: [0.95, 'Full verification passed'],
        VerificationCoverage: [0.92, '28 tests covering all paths'],
        MissionContinuity: [0.94, 'Complete handoff documentation'],
        CostEfficiency: [0.90, '32k of 50k budget used'],
        ReplanStability: [1.0, 'No replans needed'],
        ErrorRate: [0.98, '0 errors in 25 tool calls'],
      }),
      cost: { steps: 5, tokens: 32000, duration_ms: 22000, tool_calls: 25, verify_cycles: 3, replans: 0 },
    },
  },
  {
    id: 'mixed', name: 'Mixed Results', description: 'Good success but cost/coverage gaps',
    data: {
      run_id: rid, computed_at: now(), overall_score: 0.71,
      dimensions: dims({
        Success: [0.85, 'Primary objective met, secondary partial'],
        TrustVerdict: [0.72, 'Medium confidence — some unverified paths'],
        VerificationCoverage: [0.55, '12 tests, 3 files unverified'],
        MissionContinuity: [0.68, 'Handoff missing edge case docs'],
        CostEfficiency: [0.48, '78k of 80k budget used — tight'],
        ReplanStability: [0.80, '1 replan after lint failure'],
        ErrorRate: [0.88, '2 recoverable errors in 30 calls'],
      }),
      cost: { steps: 8, tokens: 78000, duration_ms: 45000, tool_calls: 30, verify_cycles: 5, replans: 1 },
    },
  },
  {
    id: 'failed', name: 'Failed Run', description: 'Critical failures across dimensions',
    data: {
      run_id: rid, computed_at: now(), overall_score: 0.32,
      dimensions: dims({
        Success: [0.20, 'Primary objective NOT met — build broken'],
        TrustVerdict: [0.15, 'Low confidence — verification failed'],
        VerificationCoverage: [0.30, '4 tests, 8 files unverified'],
        MissionContinuity: [0.40, 'No handoff — context lost after compact'],
        CostEfficiency: [0.25, 'Budget exhausted at 100%'],
        ReplanStability: [0.33, '3 replans, all failed'],
        ErrorRate: [0.42, '12 errors in 35 calls'],
      }),
      cost: { steps: 12, tokens: 100000, duration_ms: 120000, tool_calls: 35, verify_cycles: 8, replans: 3 },
    },
  },
  {
    id: 'cost-overrun', name: 'Cost Overrun', description: 'Good quality but budget blown',
    data: {
      run_id: rid, computed_at: now(), overall_score: 0.68,
      dimensions: dims({
        Success: [0.92, 'All objectives met eventually'],
        TrustVerdict: [0.88, 'High confidence after retries'],
        VerificationCoverage: [0.85, '22 tests, good coverage'],
        MissionContinuity: [0.78, 'Clear handoff'],
        CostEfficiency: [0.15, '95k of 50k budget — 190% overrun'],
        ReplanStability: [0.50, '2 replans needed'],
        ErrorRate: [0.70, '5 errors recovered'],
      }),
      cost: { steps: 14, tokens: 95000, duration_ms: 90000, tool_calls: 42, verify_cycles: 7, replans: 2 },
    },
  },
  {
    id: 'stability-issue', name: 'Stability Issues', description: 'Repeated replans, unstable execution',
    data: {
      run_id: rid, computed_at: now(), overall_score: 0.52,
      dimensions: dims({
        Success: [0.75, 'Objective met after 3 attempts'],
        TrustVerdict: [0.60, 'Medium — required multiple replans'],
        VerificationCoverage: [0.65, '15 tests but gaps in edge cases'],
        MissionContinuity: [0.55, 'Some context lost during replans'],
        CostEfficiency: [0.40, '70k tokens consumed'],
        ReplanStability: [0.10, '3 replans, 2 circular'],
        ErrorRate: [0.58, '8 errors, 3 unrecoverable'],
      }),
      cost: { steps: 11, tokens: 70000, duration_ms: 80000, tool_calls: 38, verify_cycles: 6, replans: 3 },
    },
  },
]

// ── Gate Presets ──────────────────────────────────────────────

export interface GatePreset { id: string; name: string; description: string; data: GateResult }

const BALANCED_POLICY = {
  thresholds: [
    { dimension: 'Success' as ScorecardDimension, min_score: 0.7, max_regression: 0.15 },
    { dimension: 'TrustVerdict' as ScorecardDimension, min_score: 0.6, max_regression: 0.2 },
    { dimension: 'VerificationCoverage' as ScorecardDimension, min_score: 0.5, max_regression: 0.2 },
    { dimension: 'MissionContinuity' as ScorecardDimension, min_score: 0.4, max_regression: 0.25 },
    { dimension: 'CostEfficiency' as ScorecardDimension, min_score: 0.3, max_regression: 0.3 },
    { dimension: 'ReplanStability' as ScorecardDimension, min_score: 0.5, max_regression: 0.25 },
    { dimension: 'ErrorRate' as ScorecardDimension, min_score: 0.7, max_regression: 0.15 },
  ],
  strategy: 'Balanced' as const,
  min_overall_score: 0.6,
  max_overall_regression: 0.15,
}

function gateCheck(dim: ScorecardDimension, cand: number, base: number, verdict: GateVerdict, reason: string) {
  const th = BALANCED_POLICY.thresholds.find(t => t.dimension === dim)!
  return { dimension: dim, candidate_score: cand, baseline_score: base, delta: cand - base, min_score: th.min_score, max_regression: th.max_regression, verdict, reason }
}

export const GATE_PRESETS: GatePreset[] = [
  {
    id: 'all-pass', name: 'All Pass', description: 'Every dimension passes — green light',
    data: {
      baseline_id: 'v1-stable', candidate_id: rid, policy: BALANCED_POLICY,
      dimension_checks: [
        gateCheck('Success', 0.95, 0.88, 'Pass', 'Improved +7%'),
        gateCheck('TrustVerdict', 0.92, 0.82, 'Pass', 'Improved +10%'),
        gateCheck('VerificationCoverage', 0.88, 0.80, 'Pass', 'Coverage up'),
        gateCheck('MissionContinuity', 0.85, 0.78, 'Pass', 'Stable continuity'),
        gateCheck('CostEfficiency', 0.72, 0.65, 'Pass', 'Within budget'),
        gateCheck('ReplanStability', 1.0, 0.90, 'Pass', 'Perfect stability'),
        gateCheck('ErrorRate', 0.96, 0.90, 'Pass', 'Near-zero errors'),
      ],
      baseline_overall: 0.82, candidate_overall: 0.90, overall_delta: 0.08,
      verdict: 'Pass', reasons: ['All 7 dimensions pass', 'Overall improved +8%'],
    },
  },
  {
    id: 'warn-coverage', name: 'Warn: Coverage Gap', description: 'Coverage below threshold — warning issued',
    data: {
      baseline_id: 'v1-stable', candidate_id: rid, policy: BALANCED_POLICY,
      dimension_checks: [
        gateCheck('Success', 0.88, 0.85, 'Pass', 'Slightly improved'),
        gateCheck('TrustVerdict', 0.80, 0.82, 'Pass', 'Minor regression OK'),
        gateCheck('VerificationCoverage', 0.52, 0.75, 'Warn', 'Regressed -23% but above min'),
        gateCheck('MissionContinuity', 0.70, 0.72, 'Pass', 'Stable'),
        gateCheck('CostEfficiency', 0.60, 0.55, 'Pass', 'Improved'),
        gateCheck('ReplanStability', 0.85, 0.80, 'Pass', 'Stable'),
        gateCheck('ErrorRate', 0.88, 0.85, 'Pass', 'Slight improvement'),
      ],
      baseline_overall: 0.76, candidate_overall: 0.75, overall_delta: -0.01,
      verdict: 'Warn', reasons: ['VerificationCoverage regressed -23%', 'Overall marginally regressed'],
    },
  },
  {
    id: 'fail-critical', name: 'Fail: Critical Regression', description: 'Success and errors regressed — gate blocked',
    data: {
      baseline_id: 'v1-stable', candidate_id: rid, policy: BALANCED_POLICY,
      dimension_checks: [
        gateCheck('Success', 0.45, 0.88, 'Fail', 'Below min 0.7, regressed -43%'),
        gateCheck('TrustVerdict', 0.35, 0.80, 'Fail', 'Below min 0.6, regressed -45%'),
        gateCheck('VerificationCoverage', 0.40, 0.75, 'Warn', 'Below min but within regression'),
        gateCheck('MissionContinuity', 0.50, 0.70, 'Pass', 'Above min'),
        gateCheck('CostEfficiency', 0.20, 0.60, 'Warn', 'Severe cost regression'),
        gateCheck('ReplanStability', 0.30, 0.85, 'Fail', 'Below min, regressed -55%'),
        gateCheck('ErrorRate', 0.40, 0.88, 'Fail', 'Below min 0.7, regressed -48%'),
      ],
      baseline_overall: 0.78, candidate_overall: 0.37, overall_delta: -0.41,
      verdict: 'Fail', reasons: ['4 dimensions failed', 'Overall regressed -41%', 'Critical: Success below minimum'],
    },
  },
  {
    id: 'mixed-verdicts', name: 'Mixed Verdicts', description: '3 pass, 2 warn, 2 fail — needs review',
    data: {
      baseline_id: 'v1-stable', candidate_id: rid, policy: BALANCED_POLICY,
      dimension_checks: [
        gateCheck('Success', 0.82, 0.85, 'Pass', 'Slight regression within tolerance'),
        gateCheck('TrustVerdict', 0.55, 0.80, 'Fail', 'Below min 0.6'),
        gateCheck('VerificationCoverage', 0.58, 0.75, 'Warn', 'Regressed but above min'),
        gateCheck('MissionContinuity', 0.72, 0.68, 'Pass', 'Improved'),
        gateCheck('CostEfficiency', 0.35, 0.60, 'Warn', 'Significant cost increase'),
        gateCheck('ReplanStability', 0.45, 0.80, 'Fail', 'Below min, unstable'),
        gateCheck('ErrorRate', 0.78, 0.82, 'Pass', 'Minor regression OK'),
      ],
      baseline_overall: 0.76, candidate_overall: 0.61, overall_delta: -0.15,
      verdict: 'Fail', reasons: ['2 dimensions failed (Trust, Stability)', '2 warnings', 'Overall at regression limit'],
    },
  },
]

// ── Mission Memory Presets ───────────────────────────────────

export interface MissionPreset { id: string; name: string; description: string; data: MissionMemory }

export const MISSION_PRESETS: MissionPreset[] = [
  {
    id: 'completed', name: 'Completed Mission', description: 'Successful run with full memory',
    data: {
      schema_version: 1, session_id: 'session-001', created_at: now(),
      mission: 'Refactor authentication module to use JWT with refresh token rotation',
      facts: [
        { content: 'SessionManager uses cookie-based auth with 24h TTL', source: 'src/auth/session.rs:42', established_at: now() },
        { content: 'jsonwebtoken crate already in Cargo.toml (v9.2)', source: 'Cargo.toml', established_at: now() },
        { content: 'RefreshToken table exists but is unused', source: 'migrations/003_refresh.sql', established_at: now() },
        { content: '4 route handlers check session directly', source: 'grep analysis', established_at: now() },
        { content: 'Tests cover login/logout but not token refresh', source: 'tests/auth_test.rs', established_at: now() },
      ],
      hypotheses: [
        { content: 'HS256 is sufficient — no cross-service verification needed', confidence_pct: 92, supporting_evidence: ['Single service architecture', 'No external token consumers'] },
        { content: 'Refresh token rotation prevents replay attacks', confidence_pct: 88, supporting_evidence: ['Single-use tokens invalidated after use', 'Short access token TTL (15min)'] },
      ],
      open_questions: [],
      plan: {
        current_objective: null,
        completed_steps: ['Analyze current auth', 'Design JWT schema', 'Implement middleware', 'Implement refresh', 'Integration tests'],
        remaining_steps: [],
        phase: 'complete',
      },
      verification: {
        freshness: 'Fresh', unverified_files: [],
        last_check: now(),
        checks_passed: ['cargo build', 'cargo test (22 passed)', 'cargo clippy', 'cargo fmt --check'],
        checks_failed: [],
      },
      modified_files: ['src/auth/middleware.rs', 'src/auth/jwt.rs', 'src/auth/refresh.rs', 'src/routes/mod.rs', 'tests/auth_integration.rs'],
      key_decisions: ['Chose HS256 over RS256', 'Refresh token rotation with single-use invalidation', 'Preserved session fallback for 1 release'],
      risks: ['Token secret rotation not implemented for production'],
    },
  },
  {
    id: 'active', name: 'Active Investigation', description: 'Mid-run with open hypotheses',
    data: {
      schema_version: 1, session_id: 'session-002', created_at: now(),
      mission: 'Fix intermittent 500 errors on /api/search endpoint',
      facts: [
        { content: 'Error occurs under concurrent load (>50 req/s)', source: 'access.log analysis', established_at: now() },
        { content: 'Stack trace points to SQLite connection pool', source: 'error.log:234', established_at: now() },
        { content: 'Pool size is hardcoded to 1 (Mutex<Connection>)', source: 'src/retrieval/fts.rs:18', established_at: now() },
      ],
      hypotheses: [
        { content: 'Single connection causes lock contention under load', confidence_pct: 78, supporting_evidence: ['Mutex<Connection> pattern', 'Errors correlate with high concurrency'] },
        { content: 'WAL mode not enabled — readers block writers', confidence_pct: 45, supporting_evidence: ['Default SQLite journal mode'] },
        { content: 'Connection leak in error path of search handler', confidence_pct: 22, supporting_evidence: ['Early return in error branch'] },
      ],
      open_questions: [
        'Is r2d2 connection pool appropriate for SQLite?',
        'Should we switch to async SQLite (tokio-rusqlite)?',
        'What is the expected max concurrency for search?',
      ],
      plan: {
        current_objective: 'Verify lock contention hypothesis with tracing',
        completed_steps: ['Analyze error logs', 'Identify connection pool code', 'Add tracing spans'],
        remaining_steps: ['Reproduce under load', 'Implement fix', 'Verify fix under load'],
        phase: 'executing',
      },
      verification: {
        freshness: 'Fresh', unverified_files: ['src/retrieval/fts.rs'],
        last_check: now(),
        checks_passed: ['cargo build', 'cargo test (26 passed)'],
        checks_failed: ['load test (>50 rps)'],
      },
      modified_files: ['src/retrieval/fts.rs'],
      key_decisions: ['Added tracing spans to connection acquire/release'],
      risks: ['Fix may require API change if pool is replaced', 'Load test environment not representative'],
    },
  },
  {
    id: 'early', name: 'Early Stage', description: 'Just started — minimal memory',
    data: {
      schema_version: 1, session_id: 'session-003', created_at: now(),
      mission: 'Add WebSocket support for real-time dashboard updates',
      facts: [
        { content: 'Axum 0.7 supports WebSocket via axum::extract::ws', source: 'Cargo.toml', established_at: now() },
      ],
      hypotheses: [
        { content: 'tokio-tungstenite is needed for WS client in tests', confidence_pct: 60, supporting_evidence: ['Standard test pattern for WS'] },
      ],
      open_questions: [
        'Should WS replace SSE or coexist?',
        'What message format — JSON or protobuf?',
        'Authentication mechanism for WS connections?',
      ],
      plan: {
        current_objective: 'Survey existing SSE implementation',
        completed_steps: ['Identify WS requirements'],
        remaining_steps: ['Survey SSE code', 'Design WS protocol', 'Implement WS endpoint', 'Add WS client to dashboard', 'Integration tests'],
        phase: 'planning',
      },
      verification: {
        freshness: 'Unknown', unverified_files: [],
        last_check: null,
        checks_passed: [],
        checks_failed: [],
      },
      modified_files: [],
      key_decisions: [],
      risks: ['May conflict with existing SSE consumers', 'WS adds complexity to session management'],
    },
  },
  {
    id: 'risky', name: 'High Risk Completion', description: 'Done but with significant open risks',
    data: {
      schema_version: 1, session_id: 'session-004', created_at: now(),
      mission: 'Migrate database from SQLite to PostgreSQL',
      facts: [
        { content: 'SQLite FTS5 used for full-text search — no PG equivalent without extension', source: 'src/retrieval/fts.rs', established_at: now() },
        { content: 'pg_trgm extension available on target server', source: 'ops confirmation', established_at: now() },
        { content: '3 raw SQL queries use SQLite-specific syntax', source: 'grep analysis', established_at: now() },
        { content: 'Migration scripts created for 12 tables', source: 'migrations/', established_at: now() },
      ],
      hypotheses: [
        { content: 'pg_trgm provides acceptable search quality', confidence_pct: 55, supporting_evidence: ['Similar fuzzy match capabilities', 'No benchmark data yet'] },
      ],
      open_questions: [
        'Performance impact of pg_trgm vs FTS5?',
        'Rollback strategy if PG migration fails?',
        'Data migration for existing production database?',
      ],
      plan: {
        current_objective: null,
        completed_steps: ['Audit SQLite usage', 'Create PG schema', 'Rewrite queries', 'Update connection pool', 'Basic tests'],
        remaining_steps: [],
        phase: 'complete',
      },
      verification: {
        freshness: 'Aging', unverified_files: ['src/retrieval/fts.rs', 'src/retrieval/vector.rs'],
        last_check: now(),
        checks_passed: ['cargo build', 'cargo test (18 passed)'],
        checks_failed: ['performance benchmark', 'full-text search quality'],
      },
      modified_files: ['src/retrieval/fts.rs', 'src/retrieval/vector.rs', 'src/retrieval/lib.rs', 'Cargo.toml', 'migrations/'],
      key_decisions: ['Chose pg_trgm over tsquery for simplicity', 'Kept SQLite as fallback with feature flag'],
      risks: [
        'Search quality degradation not quantified',
        'No production data migration tested',
        'Performance unknown under real load',
        'Rollback requires manual intervention',
      ],
    },
  },
]

// ── Review Packet Presets ────────────────────────────────────

export interface ReviewPreset { id: string; name: string; description: string; data: ReviewPacket }

function freshness(f: BaselineFreshness, days: number | null): BaselineFreshnessCheck {
  return {
    freshness: f, age_days: days,
    fresh_threshold_days: 14, stale_threshold_days: 30,
    recommendation: f === 'Fresh' ? 'Baseline is recent' : f === 'Aging' ? 'Consider refreshing baseline' : f === 'Stale' ? 'Baseline outdated — re-run evaluation' : 'No baseline available',
  }
}

export const REVIEW_PRESETS: ReviewPreset[] = [
  {
    id: 'ready', name: 'Merge Ready', description: 'All green — ready to merge',
    data: {
      schema_version: 1, generated_at: now(), run_id: rid,
      merge_readiness: 'Ready', trust_verdict: 'High', gate_verdict: 'Pass',
      changes: {
        modified_files: ['src/auth/jwt.rs', 'src/auth/middleware.rs', 'tests/auth_test.rs'],
        key_decisions: ['HS256 for JWT signing', 'Refresh token rotation'],
        narrative: 'Clean implementation of JWT auth with full test coverage. All checks pass, no regressions detected.',
      },
      verification: { trust_verdict: 'High', checks_passed: ['build', 'test (22)', 'clippy', 'fmt'], checks_failed: [], unverified_files: [] },
      open_risks: { risks: [], open_questions: [], unavailable_data: [] },
      scorecard: SCORECARD_PRESETS[0].data,
      gate_result: GATE_PRESETS[0].data,
      baseline_freshness: freshness('Fresh', 3),
    },
  },
  {
    id: 'conditional', name: 'Conditionally Ready', description: 'Passes but with warnings',
    data: {
      schema_version: 1, generated_at: now(), run_id: rid,
      merge_readiness: 'ConditionallyReady', trust_verdict: 'Medium', gate_verdict: 'Warn',
      changes: {
        modified_files: ['src/retrieval/fts.rs', 'src/retrieval/vector.rs', 'Cargo.toml'],
        key_decisions: ['Switched to r2d2 connection pool', 'Added WAL mode pragma'],
        narrative: 'Fix applied and basic tests pass. Coverage gap on concurrent access paths. Manual review recommended.',
      },
      verification: { trust_verdict: 'Medium', checks_passed: ['build', 'test (18)'], checks_failed: ['load test'], unverified_files: ['src/retrieval/vector.rs'] },
      open_risks: { risks: ['Concurrent access not fully tested'], open_questions: ['Expected max concurrency?'], unavailable_data: ['Production load profile'] },
      scorecard: SCORECARD_PRESETS[1].data,
      gate_result: GATE_PRESETS[1].data,
      baseline_freshness: freshness('Aging', 18),
    },
  },
  {
    id: 'not-ready', name: 'Not Ready', description: 'Critical failures — do not merge',
    data: {
      schema_version: 1, generated_at: now(), run_id: rid,
      merge_readiness: 'NotReady', trust_verdict: 'Low', gate_verdict: 'Fail',
      changes: {
        modified_files: ['src/core/state.rs', 'src/core/runtime.rs', 'src/planner/llm.rs', 'Cargo.toml'],
        key_decisions: ['Attempted async migration of state machine'],
        narrative: 'Migration partially complete. Build passes but 8 tests fail. State machine has race condition in replan path.',
      },
      verification: { trust_verdict: 'Low', checks_passed: ['build'], checks_failed: ['test (8 failed)', 'clippy (3 warnings)'], unverified_files: ['src/core/state.rs', 'src/core/runtime.rs'] },
      open_risks: { risks: ['Race condition in replan path', 'Incomplete async migration', 'Possible data loss on concurrent writes'], open_questions: ['Should we revert to sync approach?', 'Is tokio::Mutex sufficient here?'], unavailable_data: ['Thread sanitizer output'] },
      scorecard: SCORECARD_PRESETS[2].data,
      gate_result: GATE_PRESETS[2].data,
      baseline_freshness: freshness('Stale', 45),
    },
  },
  {
    id: 'no-baseline', name: 'No Baseline', description: 'First run — no comparison available',
    data: {
      schema_version: 1, generated_at: now(), run_id: rid,
      merge_readiness: 'Unknown', trust_verdict: null, gate_verdict: null,
      changes: {
        modified_files: ['src/new_module.rs'],
        key_decisions: ['Initial implementation'],
        narrative: 'First evaluation run — no baseline for comparison. Review manually.',
      },
      verification: { trust_verdict: 'Medium', checks_passed: ['build', 'test (5)'], checks_failed: [], unverified_files: ['src/new_module.rs'] },
      open_risks: { risks: [], open_questions: ['Establish baseline from this run?'], unavailable_data: ['Historical baseline', 'Gate comparison'] },
      scorecard: SCORECARD_PRESETS[1].data,
      gate_result: null,
      baseline_freshness: freshness('Unknown', null),
    },
  },
]

// ── Aggregate: all presets by category ────────────────────────

export const ALL_PRESETS = {
  scorecard: SCORECARD_PRESETS,
  gate: GATE_PRESETS,
  mission: MISSION_PRESETS,
  review: REVIEW_PRESETS,
} as const
