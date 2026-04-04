//! Review packet builder — constructs a [`ReviewPacket`] from run artifacts on disk.
//!
//! Loads whatever is available in `.oco/runs/<id>/` and delegates to
//! [`ReviewPacket::build`] for the actual assembly. Missing artifacts are
//! reported honestly in the packet's `unavailable_data` field.

use std::path::Path;

use anyhow::Result;
use oco_shared_types::{
    BaselineFreshnessCheck, EvalBaseline, GateConfig, GateResult, MissionMemory, ReviewPacket,
    RunScorecard,
};

use crate::ScorecardBuilder;

/// Build a [`ReviewPacket`] from run artifacts on disk.
///
/// `run_dir` is the `.oco/runs/<id>/` directory.
/// `run_id` is the human-readable identifier for the run.
/// `gate_config` is the repo-level gate config (from `oco.toml`).
/// `workspace` is the workspace root (for resolving baseline path).
///
/// Loads: `scorecard.json`, `summary.json`, `mission.json` from the run dir.
/// Optionally evaluates the gate if both a scorecard and baseline are available.
pub fn build_review_packet(
    run_dir: &Path,
    run_id: &str,
    gate_config: &GateConfig,
    workspace: &Path,
) -> Result<ReviewPacket> {
    // 1. Load scorecard (try scorecard.json, fall back to summary.json reconstruction)
    let scorecard = load_scorecard(run_dir, run_id);

    // 2. Load mission memory
    let mission = load_mission(run_dir);

    // 3. Evaluate gate (if scorecard and baseline are both available)
    let (gate_result, baseline_freshness) =
        evaluate_gate(&scorecard, gate_config, workspace);

    Ok(ReviewPacket::build(
        run_id.to_string(),
        scorecard,
        gate_result,
        mission.as_ref(),
        baseline_freshness,
    ))
}

/// Load a scorecard from the run directory.
///
/// Tries `scorecard.json` first, then reconstructs from `summary.json`.
fn load_scorecard(run_dir: &Path, run_id: &str) -> Option<RunScorecard> {
    // Try scorecard.json first
    let scorecard_path = run_dir.join("scorecard.json");
    if scorecard_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&scorecard_path) {
            if let Ok(sc) = serde_json::from_str::<RunScorecard>(&content) {
                return Some(sc);
            }
        }
    }

    // Fall back to reconstructing from summary.json
    let summary_path = run_dir.join("summary.json");
    if !summary_path.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&summary_path).ok()?;
    let summary: serde_json::Value = serde_json::from_str(&content).ok()?;

    let success = summary["success"].as_bool().unwrap_or(false);
    let tokens = summary["tokens_used"].as_u64().unwrap_or(0);
    let steps = summary["steps"].as_u64().unwrap_or(0) as u32;
    let duration_ms = summary["duration_ms"].as_u64().unwrap_or(0);

    let trust = match summary.get("trust_verdict").and_then(|v| v.as_str()) {
        Some("high") => oco_shared_types::TrustVerdict::High,
        Some("medium") => oco_shared_types::TrustVerdict::Medium,
        Some("low") => oco_shared_types::TrustVerdict::Low,
        _ => oco_shared_types::TrustVerdict::None,
    };

    let mut builder = ScorecardBuilder::new(run_id)
        .success(success)
        .trust_verdict(trust)
        .cost(tokens, steps, duration_ms, 0, 0);

    // Mission memory enrichment
    let mission_path = run_dir.join("mission.json");
    if mission_path.exists() {
        if let Ok(mission) = MissionMemory::load_from(&mission_path) {
            builder = builder.with_mission_memory(&mission);
        }
    }

    // Replan count from trace
    let trace_path = run_dir.join("trace.jsonl");
    if trace_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&trace_path) {
            let replan_count = content
                .lines()
                .filter(|line| line.contains("ReplanTriggered"))
                .count() as u32;
            builder = builder.replans(replan_count);
        }
    }

    Some(builder.build())
}

/// Load mission memory from the run directory.
fn load_mission(run_dir: &Path) -> Option<MissionMemory> {
    let mission_path = run_dir.join("mission.json");
    if !mission_path.exists() {
        return None;
    }
    MissionMemory::load_from(&mission_path).ok()
}

/// Evaluate the gate if both a scorecard and baseline are available.
///
/// Returns `(gate_result, freshness_check)`.
fn evaluate_gate(
    scorecard: &Option<RunScorecard>,
    gate_config: &GateConfig,
    workspace: &Path,
) -> (Option<GateResult>, Option<BaselineFreshnessCheck>) {
    let sc = match scorecard {
        Some(sc) => sc,
        None => return (None, None),
    };

    let baseline_path = workspace.join(&gate_config.baseline_path);
    if !baseline_path.exists() {
        return (None, None);
    }

    // Try to load baseline (EvalBaseline or raw RunScorecard)
    let content = match std::fs::read_to_string(&baseline_path) {
        Ok(c) => c,
        Err(_) => return (None, None),
    };
    let value: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return (None, None),
    };

    // Try EvalBaseline first
    let (baseline_sc, freshness) = if value.get("scorecard").is_some()
        && value.get("name").is_some()
    {
        match serde_json::from_value::<EvalBaseline>(value) {
            Ok(eb) => {
                let fc = BaselineFreshnessCheck::from_baseline(
                    &eb,
                    gate_config.fresh_days,
                    gate_config.stale_days,
                );
                (eb.scorecard, Some(fc))
            }
            Err(_) => return (None, None),
        }
    } else if value.get("run_id").is_some() && value.get("dimensions").is_some() {
        match serde_json::from_value::<RunScorecard>(value) {
            Ok(raw_sc) => (raw_sc, Some(BaselineFreshnessCheck::unknown())),
            Err(_) => return (None, None),
        }
    } else {
        return (None, None);
    };

    let policy = gate_config.resolve_policy();
    let gate_result = GateResult::evaluate(&baseline_sc, sc, &policy);

    (Some(gate_result), freshness)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use oco_shared_types::{
        CostMetrics, DimensionScore, MissionFact, MissionPlan, MissionVerificationStatus,
        ScorecardDimension, SessionId, TrustVerdict, VerificationFreshness,
    };

    fn make_test_scorecard(run_id: &str) -> RunScorecard {
        let dimensions: Vec<DimensionScore> = ScorecardDimension::all()
            .iter()
            .map(|d| DimensionScore {
                dimension: *d,
                score: 0.8,
                detail: "test".to_string(),
            })
            .collect();
        let overall = RunScorecard::compute_overall(&dimensions);
        RunScorecard {
            run_id: run_id.to_string(),
            computed_at: Utc::now(),
            dimensions,
            overall_score: overall,
            cost: CostMetrics::default(),
        }
    }

    fn make_test_mission() -> MissionMemory {
        MissionMemory {
            schema_version: oco_shared_types::mission::MISSION_SCHEMA_VERSION,
            session_id: SessionId::new(),
            created_at: Utc::now(),
            mission: "test mission".to_string(),
            facts: vec![MissionFact {
                content: "test fact".to_string(),
                source: None,
                established_at: Utc::now(),
            }],
            hypotheses: vec![],
            open_questions: vec![],
            plan: MissionPlan::default(),
            verification: MissionVerificationStatus {
                freshness: VerificationFreshness::Fresh,
                unverified_files: vec![],
                last_check: Some(Utc::now()),
                checks_passed: vec!["test".to_string()],
                checks_failed: vec![],
            },
            modified_files: vec!["src/lib.rs".to_string()],
            key_decisions: vec![],
            risks: vec![],
            trust_verdict: TrustVerdict::High,
            narrative: String::new(),
        }
    }

    #[test]
    fn build_from_full_run_dir() {
        let dir = std::env::temp_dir().join("oco-test-review-packet-builder");
        let run_dir = dir.join(".oco").join("runs").join("test-run");
        std::fs::create_dir_all(&run_dir).unwrap();

        // Write scorecard
        let sc = make_test_scorecard("test-run");
        let sc_json = serde_json::to_string_pretty(&sc).unwrap();
        std::fs::write(run_dir.join("scorecard.json"), &sc_json).unwrap();

        // Write mission
        let mission = make_test_mission();
        mission.save_to(&run_dir.join("mission.json")).unwrap();

        let gate_config = GateConfig::default();
        let packet = build_review_packet(&run_dir, "test-run", &gate_config, &dir).unwrap();

        assert_eq!(packet.run_id, "test-run");
        assert!(packet.scorecard.is_some());
        assert_eq!(packet.trust_verdict, Some(TrustVerdict::High));
        assert!(!packet.changes.modified_files.is_empty());
        // No baseline => no gate result
        assert!(packet.gate_result.is_none());
        assert!(packet.baseline_freshness.is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_from_empty_run_dir() {
        let dir = std::env::temp_dir().join("oco-test-review-packet-empty");
        let run_dir = dir.join(".oco").join("runs").join("empty");
        std::fs::create_dir_all(&run_dir).unwrap();

        let gate_config = GateConfig::default();
        let packet = build_review_packet(&run_dir, "empty", &gate_config, &dir).unwrap();

        assert_eq!(packet.run_id, "empty");
        assert!(packet.scorecard.is_none());
        assert!(packet.gate_result.is_none());
        assert_eq!(
            packet.merge_readiness,
            oco_shared_types::MergeReadiness::Unknown
        );
        assert!(packet.open_risks.unavailable_data.len() >= 3);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_with_baseline_triggers_gate() {
        let dir = std::env::temp_dir().join("oco-test-review-packet-gate");
        let run_dir = dir.join(".oco").join("runs").join("gated");
        std::fs::create_dir_all(&run_dir).unwrap();

        // Write scorecard
        let sc = make_test_scorecard("gated");
        std::fs::write(
            run_dir.join("scorecard.json"),
            serde_json::to_string_pretty(&sc).unwrap(),
        )
        .unwrap();

        // Write baseline
        let baseline_dir = dir.join(".oco");
        std::fs::create_dir_all(&baseline_dir).unwrap();
        let baseline = oco_shared_types::EvalBaseline::from_scorecard(
            "test-baseline",
            make_test_scorecard("baseline"),
            "test",
        );
        baseline
            .save_to(&baseline_dir.join("baseline.json"))
            .unwrap();

        let gate_config = GateConfig::default();
        let packet = build_review_packet(&run_dir, "gated", &gate_config, &dir).unwrap();

        assert!(packet.gate_result.is_some());
        assert!(packet.baseline_freshness.is_some());
        assert!(packet.gate_verdict.is_some());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_scorecard_from_summary_fallback() {
        let dir = std::env::temp_dir().join("oco-test-review-summary-fallback");
        std::fs::create_dir_all(&dir).unwrap();

        // Write summary.json (no scorecard.json)
        let summary = serde_json::json!({
            "success": true,
            "tokens_used": 5000,
            "steps": 3,
            "duration_ms": 2000,
            "trust_verdict": "medium"
        });
        std::fs::write(
            dir.join("summary.json"),
            serde_json::to_string(&summary).unwrap(),
        )
        .unwrap();

        let sc = load_scorecard(&dir, "fallback-run").unwrap();
        assert_eq!(sc.run_id, "fallback-run");
        assert_eq!(
            sc.dimension_score(ScorecardDimension::Success),
            Some(1.0)
        );

        std::fs::remove_dir_all(&dir).ok();
    }
}
