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

            UiEvent::PlanGenerated {
                step_count,
                parallel_groups,
                strategy,
                has_team,
            } => self.emit_json(
                "plan_generated",
                serde_json::json!({
                    "step_count": step_count, "parallel_groups": parallel_groups,
                    "strategy": strategy, "has_team": has_team,
                }),
            ),
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
            } => self.emit_json(
                "plan_step_completed",
                serde_json::json!({
                    "step": step_name, "success": success, "duration_ms": duration_ms,
                }),
            ),
            UiEvent::PlanReplanTriggered {
                failed_step,
                attempt,
                new_step_count,
            } => self.emit_json(
                "plan_replan",
                serde_json::json!({
                    "failed_step": failed_step, "attempt": attempt, "new_steps": new_step_count,
                }),
            ),
            UiEvent::PlanVerifyGateFailed { step_name, error } => self.emit_json(
                "plan_verify_failed",
                serde_json::json!({
                    "step": step_name, "error": error,
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
