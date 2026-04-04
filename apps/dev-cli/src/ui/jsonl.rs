use super::Renderer;
use super::event::UiEvent;

/// Machine-readable JSONL renderer.
/// One JSON object per line on stdout. No colors, no spinners.
pub struct JsonlRenderer;

impl JsonlRenderer {
    pub fn new() -> Self {
        Self
    }

    fn emit_json(&self, event_type: &str, data: serde_json::Value) {
        let mut obj = serde_json::json!({ "event": event_type });
        if let serde_json::Value::Object(map) = data {
            for (k, v) in map {
                obj[&k] = v;
            }
        }
        // Write JSON + newline on the same locked handle to prevent interleaving
        use std::io::Write;
        let mut out = std::io::stdout().lock();
        let _ = serde_json::to_writer(&mut out, &obj);
        let _ = out.write_all(b"\n");
    }
}

impl Renderer for JsonlRenderer {
    fn emit(&mut self, event: UiEvent) {
        match event {
            UiEvent::RunStarted {
                provider,
                model,
                request,
                workspace,
            } => self.emit_json(
                "run_started",
                serde_json::json!({
                    "provider": provider,
                    "model": model,
                    "request": request,
                    "workspace": workspace,
                }),
            ),

            UiEvent::RunStepCompleted {
                step,
                action_type,
                reason,
                tokens_used,
                tokens_max,
                duration_ms,
            } => self.emit_json(
                "run_step",
                serde_json::json!({
                    "step": step,
                    "action": action_type,
                    "reason": reason,
                    "tokens_used": tokens_used,
                    "tokens_max": tokens_max,
                    "duration_ms": duration_ms,
                }),
            ),

            UiEvent::RunFinished {
                session_id,
                steps,
                tokens_used,
                tokens_max,
                duration_ms,
                success,
            } => self.emit_json(
                "run_finished",
                serde_json::json!({
                    "session_id": session_id,
                    "steps": steps,
                    "tokens_used": tokens_used,
                    "tokens_max": tokens_max,
                    "duration_ms": duration_ms,
                    "success": success,
                }),
            ),

            UiEvent::RunResponse { content } => {
                self.emit_json("run_response", serde_json::json!({ "content": content }))
            }

            UiEvent::IndexStarted { path } => self.emit_json(
                "index_started",
                serde_json::json!({ "path": path.display().to_string() }),
            ),

            UiEvent::IndexCompleted {
                files,
                symbols,
                duration_ms,
            } => self.emit_json(
                "index_completed",
                serde_json::json!({
                    "files": files,
                    "symbols": symbols,
                    "duration_ms": duration_ms,
                }),
            ),

            UiEvent::SearchResult {
                rank,
                path,
                score,
                snippet,
            } => self.emit_json(
                "search_result",
                serde_json::json!({
                    "rank": rank,
                    "path": path,
                    "score": score,
                    "snippet": snippet,
                }),
            ),

            UiEvent::SearchEmpty { query } => {
                self.emit_json("search_empty", serde_json::json!({ "query": query }))
            }

            UiEvent::DoctorHeader { workspace } => self.emit_json(
                "doctor_start",
                serde_json::json!({ "workspace": workspace }),
            ),

            UiEvent::DoctorCheck {
                name,
                status,
                detail,
            } => self.emit_json(
                "doctor_check",
                serde_json::json!({
                    "name": name,
                    "status": format!("{status:?}").to_lowercase(),
                    "detail": detail,
                }),
            ),

            UiEvent::DoctorProfile {
                stack,
                build_cmd,
                test_cmd,
            } => self.emit_json(
                "doctor_profile",
                serde_json::json!({
                    "stack": stack,
                    "build_command": build_cmd,
                    "test_command": test_cmd,
                }),
            ),

            UiEvent::DoctorSummary { issues } => {
                self.emit_json("doctor_summary", serde_json::json!({ "issues": issues }))
            }

            UiEvent::EvalStarted { scenario_count } => self.emit_json(
                "eval_started",
                serde_json::json!({ "scenario_count": scenario_count }),
            ),

            UiEvent::EvalScenario {
                name,
                success,
                steps,
                tokens,
                duration_ms,
                tokens_per_step,
            } => self.emit_json(
                "eval_scenario",
                serde_json::json!({
                    "name": name,
                    "success": success,
                    "steps": steps,
                    "tokens": tokens,
                    "duration_ms": duration_ms,
                    "tokens_per_step": tokens_per_step,
                }),
            ),

            UiEvent::EvalSaved { path } => {
                self.emit_json("eval_saved", serde_json::json!({ "path": path }))
            }

            UiEvent::ServerListening { host, port } => self.emit_json(
                "server_listening",
                serde_json::json!({ "host": host, "port": port }),
            ),

            UiEvent::PlanOverview {
                step_count,
                parallel_groups,
                critical_path_length,
                estimated_tokens,
                budget_tokens,
                strategy,
                team,
                steps,
            } => {
                let step_data: Vec<serde_json::Value> = steps
                    .iter()
                    .map(|s| {
                        serde_json::json!({
                            "id": s.id.to_string(),
                            "name": s.name,
                            "role": s.role,
                            "mode": s.execution_mode,
                            "depends_on": s.depends_on.iter().map(|d| d.to_string()).collect::<Vec<_>>(),
                            "verify_after": s.verify_after,
                            "estimated_tokens": s.estimated_tokens,
                            "preferred_model": s.preferred_model,
                        })
                    })
                    .collect();
                self.emit_json(
                    "plan_overview",
                    serde_json::json!({
                        "step_count": step_count,
                        "parallel_groups": parallel_groups,
                        "critical_path_length": critical_path_length,
                        "estimated_tokens": estimated_tokens,
                        "budget_tokens": budget_tokens,
                        "strategy": strategy,
                        "team": team.as_ref().map(|(name, topo, count)| serde_json::json!({
                            "name": name, "topology": topo, "members": count,
                        })),
                        "steps": step_data,
                    }),
                );
            }
            UiEvent::PlanStepStarted {
                step_name,
                role,
                execution_mode,
            } => self.emit_json(
                "plan_step_started",
                serde_json::json!({
                    "step": step_name, "role": role, "mode": execution_mode,
                }),
            ),
            UiEvent::PlanStepCompleted {
                step_name,
                success,
                duration_ms,
                tokens_used,
            } => self.emit_json(
                "plan_step_completed",
                serde_json::json!({
                    "step": step_name, "success": success,
                    "duration_ms": duration_ms, "tokens_used": tokens_used,
                }),
            ),
            UiEvent::PlanProgress {
                completed,
                total,
                active_steps,
                budget_used_pct,
            } => self.emit_json(
                "plan_progress",
                serde_json::json!({
                    "completed": completed, "total": total,
                    "active_steps": active_steps, "budget_used_pct": budget_used_pct,
                }),
            ),
            UiEvent::PlanVerifyGateResult {
                step_name,
                checks,
                overall_passed,
                replan_triggered,
            } => {
                let check_data: Vec<serde_json::Value> = checks
                    .iter()
                    .map(|(ct, passed, summary)| {
                        serde_json::json!({"check": ct, "passed": passed, "summary": summary})
                    })
                    .collect();
                self.emit_json(
                    "plan_verify_gate",
                    serde_json::json!({
                        "step": step_name, "checks": check_data,
                        "passed": overall_passed, "replan_triggered": replan_triggered,
                    }),
                );
            }
            UiEvent::PlanReplanTriggered {
                failed_step,
                attempt,
                max_attempts,
                steps_preserved,
                steps_removed,
                steps_added,
            } => self.emit_json(
                "plan_replan",
                serde_json::json!({
                    "failed_step": failed_step, "attempt": attempt,
                    "max_attempts": max_attempts,
                    "preserved": steps_preserved, "removed": steps_removed,
                    "added": steps_added,
                }),
            ),
            UiEvent::TeamStatus {
                team_name,
                members,
                communication,
                completed,
                total,
                messages,
            } => self.emit_json(
                "team_status",
                serde_json::json!({
                    "team": team_name, "members": members, "communication": communication,
                    "completed": completed, "total": total, "messages": messages,
                }),
            ),

            UiEvent::PolicyPackActive { pack } => {
                self.emit_json("policy_pack_active", serde_json::json!({ "pack": pack }))
            }
            UiEvent::TrustVerdictFinal { verdict, freshness } => self.emit_json(
                "trust_verdict_final",
                serde_json::json!({ "verdict": verdict, "freshness": freshness }),
            ),

            UiEvent::ScorecardSummary {
                run_id,
                overall_score,
                dimension_count,
            } => self.emit_json(
                "scorecard_summary",
                serde_json::json!({
                    "run_id": run_id,
                    "overall_score": overall_score,
                    "dimension_count": dimension_count,
                }),
            ),
            UiEvent::ComparisonResult {
                baseline_id,
                candidate_id,
                overall_delta,
                regressions,
                improvements,
                verdict,
            } => self.emit_json(
                "comparison_result",
                serde_json::json!({
                    "baseline_id": baseline_id,
                    "candidate_id": candidate_id,
                    "overall_delta": overall_delta,
                    "regressions": regressions,
                    "improvements": improvements,
                    "verdict": verdict,
                }),
            ),
            UiEvent::ComparisonDetail {
                dimension,
                baseline_score,
                candidate_score,
                delta,
                kind,
            } => self.emit_json(
                "comparison_detail",
                serde_json::json!({
                    "dimension": dimension,
                    "baseline_score": baseline_score,
                    "candidate_score": candidate_score,
                    "delta": delta,
                    "kind": kind,
                }),
            ),

            // ── Eval Gate (Q6) ────────────────────────────
            UiEvent::GateHeader {
                baseline_id,
                candidate_id,
                policy,
            } => self.emit_json(
                "gate_header",
                serde_json::json!({
                    "baseline_id": baseline_id,
                    "candidate_id": candidate_id,
                    "policy": policy,
                }),
            ),
            UiEvent::GateDimensionCheck {
                dimension,
                baseline_score,
                candidate_score,
                delta,
                min_score,
                verdict,
            } => self.emit_json(
                "gate_dimension_check",
                serde_json::json!({
                    "dimension": dimension,
                    "baseline_score": baseline_score,
                    "candidate_score": candidate_score,
                    "delta": delta,
                    "min_score": min_score,
                    "verdict": verdict,
                }),
            ),
            UiEvent::GateVerdict {
                verdict,
                exit_code,
                reasons,
                failed_count,
                warned_count,
            } => self.emit_json(
                "gate_verdict",
                serde_json::json!({
                    "verdict": verdict,
                    "exit_code": exit_code,
                    "reasons": reasons,
                    "failed_count": failed_count,
                    "warned_count": warned_count,
                }),
            ),

            UiEvent::Info { message } => {
                self.emit_json("info", serde_json::json!({ "message": message }))
            }
            UiEvent::Success { message } => {
                self.emit_json("success", serde_json::json!({ "message": message }))
            }
            UiEvent::Warning { message } => {
                self.emit_json("warning", serde_json::json!({ "message": message }))
            }
            UiEvent::Error { message } => {
                self.emit_json("error", serde_json::json!({ "message": message }))
            }
        }
    }
}
