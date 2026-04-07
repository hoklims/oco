/** Mirrors the Rust DashboardEvent envelope. */
export interface DashboardEvent {
  schema_version: number
  seq: number
  ts: string
  session_id: { inner: string } | string
  run_id: string
  plan_version: number
  kind: DashboardEventKind
}

export type DashboardEventKind =
  | { type: 'run_started'; provider: string; model: string; request_summary: string; complexity?: string }
  | { type: 'run_stopped'; reason: StopReason; total_steps: number; total_tokens: number }
  | { type: 'plan_exploration'; candidates: PlanCandidateSummary[]; winner_strategy: string; winner_score: number }
  | { type: 'plan_generated'; plan_id: string; step_count: number; parallel_group_count: number; critical_path_length: number; estimated_total_tokens: number; strategy: string; team: TeamSummary | null; steps: StepSummary[] }
  | { type: 'step_started'; step_id: string; step_name: string; role: string; execution_mode: string }
  | { type: 'step_completed'; step_id: string; step_name: string; success: boolean; duration_ms: number; tokens_used: number; detail_ref: string | null }
  | { type: 'flat_step_completed'; step: number; action_type: string; reason: string; duration_ms: number; budget_snapshot: BudgetSnapshot }
  | { type: 'progress'; completed: number; total: number; active_steps: ActiveStep[]; budget: BudgetSnapshot }
  | { type: 'verify_gate_result'; step_id: string; step_name: string; checks: CheckResult[]; overall_passed: boolean; replan_triggered: boolean }
  | { type: 'replan_triggered'; failed_step_name: string; attempt: number; max_attempts: number; steps_preserved: number; steps_removed: number; steps_added: number }
  | { type: 'budget_warning'; resource: string; utilization: number }
  | { type: 'budget_snapshot' } & BudgetSnapshot
  | { type: 'index_progress'; files_done: number; symbols_so_far: number }
  | { type: 'heartbeat' }
  // ── Hierarchical plan events ─────────────────────────────
  | { type: 'sub_plan_started'; parent_step_id: string; sub_step_count: number; sub_steps: SubStepSummary[] }
  | { type: 'sub_step_progress'; parent_step_id: string; sub_step_id: string; sub_step_name: string; status: 'pending' | 'running' | 'passed' | 'failed' }
  | { type: 'sub_plan_completed'; parent_step_id: string; success: boolean; duration_ms: number; tokens_used: number }
  // ── Teammate communication events ────────────────────────
  | { type: 'teammate_message'; from_step_id: string; to_step_id: string; from_name: string; to_name: string; summary: string }
  | { type: 'teammate_idle'; step_id: string; step_name: string }

export interface PlanCandidateSummary {
  strategy: string
  step_count: number
  estimated_tokens: number
  score: number
  verify_count: number
  parallel_groups: number
  winner: boolean
  planning_tokens?: number
}

export interface BudgetSnapshot {
  tokens_used: number
  tokens_remaining: number
  tool_calls_used: number
  tool_calls_remaining: number
  retrievals_used: number
  verify_cycles_used: number
  elapsed_secs: number
}

export interface StepSummary {
  id: string
  name: string
  description: string
  role: string
  execution_mode: string
  depends_on: string[]
  verify_after: boolean
  estimated_tokens: number
  preferred_model: string | null
  sub_plan?: { steps: SubStepSummary[]; parallel_groups: number } | null
}

export interface SubStepSummary {
  id: string
  name: string
  description: string
  estimated_tokens: number
}

export interface TeamSummary {
  name: string
  topology: string
  member_count: number
}

export interface ActiveStep {
  step_id: string
  step_name: string
}

export interface CheckResult {
  check_type: string
  passed: boolean
  summary: string
}

export type StopReason =
  | { type: 'task_complete' }
  | { type: 'budget_exhausted' }
  | { type: 'user_cancelled' }
  | { type: 'error'; message: string }
  | { type: 'max_steps_reached' }
  | { type: 'needs_user_input'; question: string }

/** Step row for the step table. */
export interface StepRow {
  id: string
  name: string
  role: string
  status: 'pending' | 'running' | 'passed' | 'failed'
  duration_ms: number | null
  tokens_used: number | null
  execution_mode: string
  verify_passed: boolean | null
}

// ── Post-run intelligence types ───────────────────────────────

export type ScorecardDimension =
  | 'Success'
  | 'TrustVerdict'
  | 'VerificationCoverage'
  | 'MissionContinuity'
  | 'CostEfficiency'
  | 'ReplanStability'
  | 'ErrorRate'

export interface DimensionScore {
  dimension: ScorecardDimension
  score: number
  detail: string
}

export interface CostMetrics {
  steps: number
  tokens: number
  duration_ms: number
  tool_calls: number
  verify_cycles: number
  replans: number
}

export interface RunScorecard {
  run_id: string
  computed_at: string
  dimensions: DimensionScore[]
  overall_score: number
  cost: CostMetrics
}

// ── Gate types ────────────────────────────────────────────────

export type GateVerdict = 'Pass' | 'Warn' | 'Fail'
export type GateStrategy = 'Strict' | 'Balanced' | 'Lenient'
export type ComparisonVerdict = 'Improved' | 'Stable' | 'Regressed'

export interface GateThreshold {
  dimension: ScorecardDimension
  min_score: number
  max_regression: number
}

export interface GatePolicy {
  thresholds: GateThreshold[]
  strategy: GateStrategy
  min_overall_score: number
  max_overall_regression: number
}

export interface DimensionGateCheck {
  dimension: ScorecardDimension
  candidate_score: number
  baseline_score: number
  delta: number
  min_score: number
  max_regression: number
  verdict: GateVerdict
  reason: string
}

export interface GateResult {
  baseline_id: string
  candidate_id: string
  policy: GatePolicy
  dimension_checks: DimensionGateCheck[]
  baseline_overall: number
  candidate_overall: number
  overall_delta: number
  verdict: GateVerdict
  reasons: string[]
}

// ── Mission Memory ────────────────────────────────────────────

export interface MissionFact {
  content: string
  source: string | null
  established_at: string
}

export interface MissionHypothesis {
  content: string
  confidence_pct: number
  supporting_evidence: string[]
}

export interface MissionPlan {
  current_objective: string | null
  completed_steps: string[]
  remaining_steps: string[]
  phase: string | null
}

export type TrustVerdict = 'High' | 'Medium' | 'Low' | 'None'

export interface MissionVerificationStatus {
  freshness: string
  unverified_files: string[]
  last_check: string | null
  checks_passed: string[]
  checks_failed: string[]
}

export interface MissionMemory {
  schema_version: number
  session_id: string
  created_at: string
  mission: string
  facts: MissionFact[]
  hypotheses: MissionHypothesis[]
  open_questions: string[]
  plan: MissionPlan
  verification: MissionVerificationStatus
  modified_files: string[]
  key_decisions: string[]
  risks: string[]
}

// ── Review Packet ─────────────────────────────────────────────

export type MergeReadiness = 'Ready' | 'ConditionallyReady' | 'NotReady' | 'Unknown'
export type BaselineFreshness = 'Fresh' | 'Aging' | 'Stale' | 'Unknown'

export interface BaselineFreshnessCheck {
  freshness: BaselineFreshness
  age_days: number | null
  fresh_threshold_days: number
  stale_threshold_days: number
  recommendation: string
}

export interface ChangesSummary {
  modified_files: string[]
  key_decisions: string[]
  narrative: string | null
}

export interface VerificationSummary {
  trust_verdict: TrustVerdict
  checks_passed: string[]
  checks_failed: string[]
  unverified_files: string[]
}

export interface OpenRisks {
  risks: string[]
  open_questions: string[]
  unavailable_data: string[]
}

export interface ReviewPacket {
  schema_version: number
  generated_at: string
  run_id: string
  merge_readiness: MergeReadiness
  trust_verdict: TrustVerdict | null
  gate_verdict: GateVerdict | null
  changes: ChangesSummary
  verification: VerificationSummary
  open_risks: OpenRisks
  scorecard: RunScorecard | null
  gate_result: GateResult | null
  baseline_freshness: BaselineFreshnessCheck | null
}

// ── Compact Snapshot ──────────────────────────────────────────

export interface CompactSnapshot {
  verified_facts: string[]
  hypotheses: [string, number][]
  questions: string[]
  plan: string[]
  verification_freshness: string
  unverified_files: string[]
  inspected_paths: string[]
  created_at: string
}
