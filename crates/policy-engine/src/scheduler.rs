//! DAG-based action scheduler with parallel wave detection.
//!
//! Groups actions by dependency level using topological sort (Kahn's algorithm
//! via `petgraph`). Actions at the same level can execute in parallel.
//! Also computes critical path and per-action slack.

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// A schedulable action with an ID, estimated duration, and dependencies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulableAction {
    /// Unique identifier for this action.
    pub id: String,
    /// Human-readable label.
    pub label: String,
    /// Estimated duration in milliseconds.
    pub estimated_duration_ms: u64,
    /// IDs of actions that must complete before this one.
    pub depends_on: Vec<String>,
}

/// A wave of actions that can execute in parallel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionWave {
    /// Wave index (0-based, topological order).
    pub level: usize,
    /// Actions in this wave — all independent of each other.
    pub actions: Vec<String>,
    /// Maximum estimated duration of this wave (bottleneck action).
    pub wave_duration_ms: u64,
}

/// Per-action scheduling metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionSchedule {
    /// Action ID.
    pub id: String,
    /// Earliest start time (sum of critical predecessors).
    pub earliest_start_ms: u64,
    /// Latest start time before delaying the project.
    pub latest_start_ms: u64,
    /// Slack = latest_start - earliest_start. Zero slack = critical path.
    pub slack_ms: u64,
    /// Whether this action is on the critical path.
    pub is_critical: bool,
}

/// Full schedule result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    /// Actions grouped into parallel waves.
    pub waves: Vec<ExecutionWave>,
    /// Per-action scheduling info with slack.
    pub action_schedules: Vec<ActionSchedule>,
    /// Total estimated duration (critical path length).
    pub total_duration_ms: u64,
    /// Maximum parallelism (largest wave size).
    pub max_parallelism: usize,
}

/// Errors during scheduling.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SchedulerError {
    #[error("circular dependency detected involving: {actions:?}")]
    CyclicDependency { actions: Vec<String> },
    #[error("unknown dependency '{dependency}' referenced by action '{action}'")]
    UnknownDependency { action: String, dependency: String },
    #[error("duplicate action ID '{id}'")]
    DuplicateActionId { id: String },
}

/// Build a parallel execution schedule from a set of actions with dependencies.
///
/// Returns waves of actions that can run concurrently, plus critical path analysis.
pub fn schedule(actions: &[SchedulableAction]) -> Result<Schedule, SchedulerError> {
    if actions.is_empty() {
        return Ok(Schedule {
            waves: vec![],
            action_schedules: vec![],
            total_duration_ms: 0,
            max_parallelism: 0,
        });
    }

    // Validate unique action IDs
    {
        let mut seen = std::collections::HashSet::new();
        for action in actions {
            if !seen.insert(&action.id) {
                return Err(SchedulerError::DuplicateActionId {
                    id: action.id.clone(),
                });
            }
        }
    }

    // Build DAG
    let mut graph = DiGraph::<&str, ()>::new();
    let mut id_to_node: HashMap<&str, NodeIndex> = HashMap::new();
    let mut node_to_id: HashMap<NodeIndex, &str> = HashMap::new();
    let mut durations: HashMap<&str, u64> = HashMap::new();

    for action in actions {
        let node = graph.add_node(action.id.as_str());
        id_to_node.insert(&action.id, node);
        node_to_id.insert(node, &action.id);
        durations.insert(&action.id, action.estimated_duration_ms);
    }

    // Add edges: dependency → action (dependency must complete first)
    for action in actions {
        let action_node = id_to_node[action.id.as_str()];
        for dep_id in &action.depends_on {
            let dep_node = id_to_node.get(dep_id.as_str()).ok_or_else(|| {
                SchedulerError::UnknownDependency {
                    action: action.id.clone(),
                    dependency: dep_id.clone(),
                }
            })?;
            graph.add_edge(*dep_node, action_node, ());
        }
    }

    // Topological sort with level assignment (Kahn's algorithm)
    let mut in_degree: HashMap<NodeIndex, usize> = HashMap::new();
    for node in graph.node_indices() {
        in_degree.insert(
            node,
            graph
                .edges_directed(node, petgraph::Direction::Incoming)
                .count(),
        );
    }

    let mut queue: VecDeque<NodeIndex> = VecDeque::new();
    let mut max_pred_level: HashMap<NodeIndex, usize> = HashMap::new();
    for (&node, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(node);
            max_pred_level.insert(node, 0);
        }
    }

    let mut levels: HashMap<NodeIndex, usize> = HashMap::new();
    let mut processed = 0usize;

    while let Some(node) = queue.pop_front() {
        let level = max_pred_level[&node];
        levels.insert(node, level);
        processed += 1;

        for edge in graph.edges(node) {
            let target = edge.target();
            // Track the maximum predecessor level for each target
            let entry = max_pred_level.entry(target).or_insert(0);
            *entry = (*entry).max(level + 1);

            let deg = in_degree.get_mut(&target).expect("node in graph");
            *deg -= 1;
            if *deg == 0 {
                queue.push_back(target);
            }
        }
    }

    // Cycle detection
    if processed != graph.node_count() {
        let remaining: Vec<String> = graph
            .node_indices()
            .filter(|n| !levels.contains_key(n))
            .filter_map(|n| node_to_id.get(&n).map(|s| s.to_string()))
            .collect();
        return Err(SchedulerError::CyclicDependency { actions: remaining });
    }

    // Group by level → waves
    let max_level = levels.values().copied().max().unwrap_or(0);
    let mut waves: Vec<ExecutionWave> = Vec::with_capacity(max_level + 1);

    for level in 0..=max_level {
        let mut wave_actions: Vec<String> = levels
            .iter()
            .filter(|(_, l)| **l == level)
            .map(|(node, _)| node_to_id[node].to_string())
            .collect();
        wave_actions.sort();

        let wave_duration = wave_actions
            .iter()
            .map(|id| durations[id.as_str()])
            .max()
            .unwrap_or(0);

        waves.push(ExecutionWave {
            level,
            actions: wave_actions,
            wave_duration_ms: wave_duration,
        });
    }

    // Critical path analysis — forward pass (earliest start)
    let mut earliest_start: HashMap<&str, u64> = HashMap::new();
    for wave in &waves {
        for action_id in &wave.actions {
            let node = id_to_node[action_id.as_str()];
            let es = graph
                .edges_directed(node, petgraph::Direction::Incoming)
                .map(|e| {
                    let pred_id = node_to_id[&e.source()];
                    earliest_start[pred_id] + durations[pred_id]
                })
                .max()
                .unwrap_or(0);
            earliest_start.insert(action_id.as_str(), es);
        }
    }

    // Total project duration
    let total_duration = actions
        .iter()
        .map(|a| earliest_start[a.id.as_str()] + a.estimated_duration_ms)
        .max()
        .unwrap_or(0);

    // Backward pass (latest start)
    let mut latest_start: HashMap<&str, u64> = HashMap::new();
    for wave in waves.iter().rev() {
        for action_id in &wave.actions {
            let node = id_to_node[action_id.as_str()];
            let dur = durations[action_id.as_str()];
            let ls = graph
                .edges(node)
                .map(|e| {
                    let succ_id = node_to_id[&e.target()];
                    latest_start[succ_id]
                })
                .min()
                .map(|min_succ_ls| min_succ_ls.saturating_sub(dur))
                .unwrap_or_else(|| total_duration.saturating_sub(dur));
            latest_start.insert(action_id.as_str(), ls);
        }
    }

    // Build per-action schedules
    let mut action_schedules: Vec<ActionSchedule> = Vec::with_capacity(actions.len());
    for action in actions {
        let es = earliest_start[action.id.as_str()];
        let ls = latest_start[action.id.as_str()];
        let slack = ls.saturating_sub(es);
        action_schedules.push(ActionSchedule {
            id: action.id.clone(),
            earliest_start_ms: es,
            latest_start_ms: ls,
            slack_ms: slack,
            is_critical: slack == 0,
        });
    }

    let max_parallelism = waves.iter().map(|w| w.actions.len()).max().unwrap_or(0);

    Ok(Schedule {
        waves,
        action_schedules,
        total_duration_ms: total_duration,
        max_parallelism,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(id: &str, duration: u64, deps: &[&str]) -> SchedulableAction {
        SchedulableAction {
            id: id.to_string(),
            label: id.to_string(),
            estimated_duration_ms: duration,
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn empty_schedule() {
        let s = schedule(&[]).unwrap();
        assert!(s.waves.is_empty());
        assert_eq!(s.total_duration_ms, 0);
        assert_eq!(s.max_parallelism, 0);
    }

    #[test]
    fn single_action() {
        let s = schedule(&[action("a", 100, &[])]).unwrap();
        assert_eq!(s.waves.len(), 1);
        assert_eq!(s.waves[0].actions, vec!["a"]);
        assert_eq!(s.total_duration_ms, 100);
        assert_eq!(s.max_parallelism, 1);
    }

    #[test]
    fn two_independent_actions_same_wave() {
        let s = schedule(&[action("a", 100, &[]), action("b", 200, &[])]).unwrap();
        assert_eq!(s.waves.len(), 1);
        assert_eq!(s.waves[0].actions.len(), 2);
        assert_eq!(s.max_parallelism, 2);
        // Total duration = max of parallel actions
        assert_eq!(s.total_duration_ms, 200);
    }

    #[test]
    fn sequential_chain() {
        let s = schedule(&[
            action("a", 100, &[]),
            action("b", 200, &["a"]),
            action("c", 50, &["b"]),
        ])
        .unwrap();
        assert_eq!(s.waves.len(), 3);
        assert_eq!(s.total_duration_ms, 350);
        assert_eq!(s.max_parallelism, 1);
        // All on critical path
        for sched in &s.action_schedules {
            assert!(sched.is_critical, "{} should be critical", sched.id);
            assert_eq!(sched.slack_ms, 0);
        }
    }

    #[test]
    fn diamond_dependency() {
        // a → b, a → c, b → d, c → d
        let s = schedule(&[
            action("a", 100, &[]),
            action("b", 200, &["a"]),
            action("c", 50, &["a"]),
            action("d", 100, &["b", "c"]),
        ])
        .unwrap();
        assert_eq!(s.waves.len(), 3);
        // Wave 0: [a], Wave 1: [b, c], Wave 2: [d]
        assert_eq!(s.waves[0].actions.len(), 1);
        assert_eq!(s.waves[1].actions.len(), 2);
        assert_eq!(s.waves[2].actions.len(), 1);
        // Critical path: a(100) → b(200) → d(100) = 400
        assert_eq!(s.total_duration_ms, 400);
        // c has slack (50ms vs 200ms for b)
        let c_sched = s.action_schedules.iter().find(|s| s.id == "c").unwrap();
        assert!(c_sched.slack_ms > 0);
        assert!(!c_sched.is_critical);
    }

    #[test]
    fn detects_cycle() {
        let result = schedule(&[action("a", 100, &["b"]), action("b", 100, &["a"])]);
        assert!(matches!(
            result,
            Err(SchedulerError::CyclicDependency { .. })
        ));
    }

    #[test]
    fn detects_unknown_dependency() {
        let result = schedule(&[action("a", 100, &["nonexistent"])]);
        assert!(matches!(
            result,
            Err(SchedulerError::UnknownDependency { .. })
        ));
    }

    #[test]
    fn complex_dag_parallelism() {
        // a → c, b → c, b → d, d → e (a and b are independent roots)
        let s = schedule(&[
            action("a", 100, &[]),
            action("b", 150, &[]),
            action("c", 50, &["a", "b"]),
            action("d", 200, &["b"]),
            action("e", 100, &["d"]),
        ])
        .unwrap();
        assert_eq!(s.max_parallelism, 2); // a,b in wave 0; c,d in wave 1
        // Critical path: b(150) → d(200) → e(100) = 450
        assert_eq!(s.total_duration_ms, 450);
    }

    #[test]
    fn wide_parallelism() {
        let actions: Vec<_> = (0..10)
            .map(|i| action(&format!("t{i}"), 100, &[]))
            .collect();
        let s = schedule(&actions).unwrap();
        assert_eq!(s.waves.len(), 1);
        assert_eq!(s.max_parallelism, 10);
        assert_eq!(s.total_duration_ms, 100);
    }
}
