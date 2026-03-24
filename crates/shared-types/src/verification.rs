use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Tracks what has been modified and what has been verified,
/// enabling the system to know whether verification is fresh or stale.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerificationState {
    /// Files that have been modified during this session, with their modification timestamps.
    pub modified_files: HashMap<String, DateTime<Utc>>,
    /// Verification runs performed, keyed by strategy name.
    pub runs: Vec<VerificationRun>,
}

/// A single verification run result with provenance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationRun {
    /// Which strategy was used (build, test, lint, typecheck, custom).
    pub strategy: String,
    /// When this verification was executed.
    pub timestamp: DateTime<Utc>,
    /// Whether the verification passed.
    pub passed: bool,
    /// Files that were covered by this verification (empty = whole project).
    pub covered_files: HashSet<String>,
    /// Snapshot of modified_files at the time of verification.
    /// Used to detect staleness: if modified_files changed after this snapshot,
    /// the verification is stale.
    pub modifications_snapshot: HashMap<String, DateTime<Utc>>,
    /// Duration of the verification run in milliseconds.
    pub duration_ms: u64,
    /// Failures reported by the runner.
    pub failures: Vec<String>,
}

/// Summary of the current verification freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationFreshness {
    /// All modified files have been verified after their latest modification.
    Fresh,
    /// Some modified files have been verified, but not all.
    Partial,
    /// Modifications happened after the last verification — results are stale.
    Stale,
    /// No verification has been performed yet.
    None,
}

impl VerificationState {
    /// Record that a file was modified.
    pub fn record_modification(&mut self, path: String) {
        self.modified_files.insert(path, Utc::now());
    }

    /// Record a verification run.
    pub fn record_run(&mut self, run: VerificationRun) {
        self.runs.push(run);
    }

    /// Compute the current freshness of verification relative to modifications.
    pub fn freshness(&self) -> VerificationFreshness {
        if self.runs.is_empty() {
            return VerificationFreshness::None;
        }
        if self.modified_files.is_empty() {
            // Nothing was modified — any verification is fresh by definition.
            return VerificationFreshness::Fresh;
        }

        let latest_run = self.runs.iter().max_by_key(|r| r.timestamp);
        let Some(latest) = latest_run else {
            return VerificationFreshness::None;
        };

        // Check if any file was modified after the latest verification.
        let any_stale = self.modified_files.iter().any(|(path, mod_time)| {
            // File is stale if it was modified after the latest run timestamp
            // AND it wasn't in the snapshot (meaning it changed after verify).
            match latest.modifications_snapshot.get(path) {
                Some(snapshot_time) => mod_time > snapshot_time,
                None => *mod_time > latest.timestamp,
            }
        });

        if any_stale {
            return VerificationFreshness::Stale;
        }

        // Check if all modified files are covered.
        if latest.covered_files.is_empty() {
            // Whole-project verification covers everything.
            return VerificationFreshness::Fresh;
        }

        let all_covered = self
            .modified_files
            .keys()
            .all(|f| latest.covered_files.contains(f));

        if all_covered {
            VerificationFreshness::Fresh
        } else {
            VerificationFreshness::Partial
        }
    }

    /// Get the latest passing run for a given strategy, if any.
    pub fn latest_passing(&self, strategy: &str) -> Option<&VerificationRun> {
        self.runs
            .iter()
            .rev()
            .find(|r| r.strategy == strategy && r.passed)
    }

    /// Get all strategies that have been run.
    pub fn strategies_run(&self) -> HashSet<&str> {
        self.runs.iter().map(|r| r.strategy.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_none_when_no_runs() {
        let state = VerificationState::default();
        assert_eq!(state.freshness(), VerificationFreshness::None);
    }

    #[test]
    fn freshness_fresh_when_no_modifications() {
        let mut state = VerificationState::default();
        state.record_run(VerificationRun {
            strategy: "build".into(),
            timestamp: Utc::now(),
            passed: true,
            covered_files: HashSet::new(),
            modifications_snapshot: HashMap::new(),
            duration_ms: 100,
            failures: vec![],
        });
        assert_eq!(state.freshness(), VerificationFreshness::Fresh);
    }

    #[test]
    fn freshness_stale_after_new_modification() {
        let mut state = VerificationState::default();
        let verify_time = Utc::now();
        state.record_run(VerificationRun {
            strategy: "build".into(),
            timestamp: verify_time,
            passed: true,
            covered_files: HashSet::new(),
            modifications_snapshot: HashMap::new(),
            duration_ms: 100,
            failures: vec![],
        });
        // Simulate a modification after verification.
        state.modified_files.insert(
            "src/main.rs".into(),
            verify_time + chrono::Duration::seconds(1),
        );
        assert_eq!(state.freshness(), VerificationFreshness::Stale);
    }

    #[test]
    fn freshness_fresh_when_verified_after_modification() {
        let mut state = VerificationState::default();
        let mod_time = Utc::now();
        state.record_modification("src/main.rs".into());

        let mut snapshot: HashMap<String, DateTime<Utc>> = HashMap::new();
        snapshot.insert("src/main.rs".into(), mod_time + chrono::Duration::seconds(1));
        state.modified_files.insert("src/main.rs".into(), mod_time);

        state.record_run(VerificationRun {
            strategy: "build".into(),
            timestamp: mod_time + chrono::Duration::seconds(2),
            passed: true,
            covered_files: HashSet::new(),
            modifications_snapshot: state.modified_files.clone(),
            duration_ms: 100,
            failures: vec![],
        });
        assert_eq!(state.freshness(), VerificationFreshness::Fresh);
    }

    #[test]
    fn latest_passing_returns_correct_run() {
        let mut state = VerificationState::default();
        state.record_run(VerificationRun {
            strategy: "test".into(),
            timestamp: Utc::now(),
            passed: false,
            covered_files: HashSet::new(),
            modifications_snapshot: HashMap::new(),
            duration_ms: 50,
            failures: vec!["test_foo failed".into()],
        });
        state.record_run(VerificationRun {
            strategy: "test".into(),
            timestamp: Utc::now(),
            passed: true,
            covered_files: HashSet::new(),
            modifications_snapshot: HashMap::new(),
            duration_ms: 80,
            failures: vec![],
        });
        assert!(state.latest_passing("test").is_some());
        assert!(state.latest_passing("build").is_none());
    }
}
