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
  | { type: 'run_started'; provider: string; model: string; request_summary: string }
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

export interface PlanCandidateSummary {
  strategy: string
  step_count: number
  estimated_tokens: number
  score: number
  strengths: string[]
  weaknesses: string[]
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
