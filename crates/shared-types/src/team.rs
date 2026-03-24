//! Agent Teams protocol — team coordination, shared task list, mailbox messaging.
//!
//! Maps to Claude Code Agent Teams:
//! - `TeamCoordinator` → Team Lead session
//! - `SharedTaskList` → Shared Task List (Ctrl+T in Claude Code)
//! - `SharedMemoryBus` → Mailbox messaging (any-to-any)
//! - `AgentRegistry` → Teammate management
//!
//! Communication modes:
//! - `Mesh` → Agent Teams (any ↔ any via mailbox)
//! - `HubSpoke` → Subagents (all through lead)
//! - `Pipeline` → Factory assembly line (sequential handoff)

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::agent::{AgentDescriptor, AgentId, AgentRegistry, Capability};
use crate::plan::{ExecutionPlan, PlanStep, StepStatus, TeamCommunication, TeamConfig, TeamMember};

/// A task in the shared task list, visible to all team members.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedTask {
    /// The plan step this task maps to.
    pub step_id: Uuid,
    /// Task name (from PlanStep).
    pub name: String,
    /// Current status.
    pub status: StepStatus,
    /// Agent that claimed this task (None = unclaimed).
    pub claimed_by: Option<AgentId>,
    /// Whether dependencies are all met.
    pub dependencies_met: bool,
    /// When this task was claimed.
    pub claimed_at: Option<DateTime<Utc>>,
}

/// Shared task list — visible queue of tasks for the team.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SharedTaskList {
    pub tasks: Vec<SharedTask>,
}

impl SharedTaskList {
    /// Create a task list from a plan's steps.
    pub fn from_plan(plan: &ExecutionPlan) -> Self {
        let completed: std::collections::HashSet<Uuid> = plan
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .map(|s| s.id)
            .collect();

        let tasks = plan
            .steps
            .iter()
            .map(|step| SharedTask {
                step_id: step.id,
                name: step.name.clone(),
                status: step.status.clone(),
                claimed_by: None,
                dependencies_met: step.depends_on.iter().all(|d| completed.contains(d)),
                claimed_at: None,
            })
            .collect();

        Self { tasks }
    }

    /// Tasks available for claiming (pending + dependencies met + unclaimed).
    pub fn claimable(&self) -> Vec<&SharedTask> {
        self.tasks
            .iter()
            .filter(|t| {
                t.status == StepStatus::Pending && t.dependencies_met && t.claimed_by.is_none()
            })
            .collect()
    }

    /// Claim a task for an agent. Returns false if already claimed or not claimable.
    pub fn claim(&mut self, step_id: Uuid, agent_id: AgentId) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.step_id == step_id)
            && task.status == StepStatus::Pending
            && task.dependencies_met
            && task.claimed_by.is_none()
        {
            task.claimed_by = Some(agent_id);
            task.status = StepStatus::InProgress;
            task.claimed_at = Some(Utc::now());
            return true;
        }
        false
    }

    /// Mark a task as completed.
    pub fn complete(&mut self, step_id: Uuid) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.step_id == step_id) {
            task.status = StepStatus::Completed;
        }
        // Update dependency flags for downstream tasks
        self.refresh_dependencies();
    }

    /// Mark a task as failed.
    pub fn fail(&mut self, step_id: Uuid, reason: String) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.step_id == step_id) {
            task.status = StepStatus::Failed { reason };
        }
    }

    /// Refresh dependency flags after status changes.
    /// This is a UI-level approximation — the GraphRunner handles real dependency logic.
    fn refresh_dependencies(&mut self) {
        for task in &mut self.tasks {
            if task.status == StepStatus::Pending {
                // Will be overridden by sync_with_plan with real dependency data
                task.dependencies_met = true;
            }
        }
    }

    /// Sync task list with plan state (call after GraphRunner updates steps).
    pub fn sync_with_plan(&mut self, plan: &ExecutionPlan) {
        let completed: std::collections::HashSet<Uuid> = plan
            .steps
            .iter()
            .filter(|s| s.status == StepStatus::Completed)
            .map(|s| s.id)
            .collect();

        for task in &mut self.tasks {
            if let Some(step) = plan.get_step(task.step_id) {
                task.status = step.status.clone();
                task.dependencies_met = step.depends_on.iter().all(|d| completed.contains(d));
            }
        }
    }

    /// Count of tasks by status.
    pub fn summary(&self) -> TaskListSummary {
        let mut s = TaskListSummary::default();
        for task in &self.tasks {
            match task.status {
                StepStatus::Pending => s.pending += 1,
                StepStatus::Blocked => s.blocked += 1,
                StepStatus::InProgress => s.in_progress += 1,
                StepStatus::Completed => s.completed += 1,
                StepStatus::Failed { .. } => s.failed += 1,
                StepStatus::Replanned => s.replanned += 1,
            }
        }
        s.total = self.tasks.len() as u32;
        s
    }
}

/// Summary of task list status.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskListSummary {
    pub total: u32,
    pub pending: u32,
    pub blocked: u32,
    pub in_progress: u32,
    pub completed: u32,
    pub failed: u32,
    pub replanned: u32,
}

/// Coordinates a team of agents working on a plan.
///
/// Manages the lifecycle: spawn members → assign tasks → monitor → converge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamCoordinator {
    /// Team configuration.
    pub config: TeamConfig,
    /// Shared task list visible to all members.
    pub task_list: SharedTaskList,
    /// When this team was spawned.
    pub created_at: DateTime<Utc>,
    /// Messages exchanged (count for monitoring).
    pub messages_exchanged: u32,
}

impl TeamCoordinator {
    /// Create a new team coordinator from a plan and team config.
    pub fn new(plan: &ExecutionPlan, config: TeamConfig) -> Self {
        Self {
            config,
            task_list: SharedTaskList::from_plan(plan),
            created_at: Utc::now(),
            messages_exchanged: 0,
        }
    }

    /// Spawn team members in the agent registry.
    /// Returns the assigned agent IDs.
    pub fn spawn_members(&mut self, registry: &mut AgentRegistry) -> Vec<AgentId> {
        let mut ids = Vec::new();

        for member in &mut self.config.members {
            let capabilities: Vec<Capability> = member
                .role
                .required_capabilities
                .iter()
                .map(Capability::new)
                .collect();

            let agent = AgentDescriptor::new(&member.role.name, &member.role.name)
                .with_capabilities(capabilities)
                .with_max_load(member.assigned_steps.len() as u32);

            let id = registry.register(agent);
            member.agent_id = Some(id);
            ids.push(id);
        }

        ids
    }

    /// Find the best member for a given step (by capability match).
    pub fn member_for_step(&self, step: &PlanStep) -> Option<&TeamMember> {
        // First: find member explicitly assigned to this step
        let explicit = self
            .config
            .members
            .iter()
            .find(|m| m.assigned_steps.contains(&step.id));
        if explicit.is_some() {
            return explicit;
        }

        // Fallback: find member whose role matches step's required capabilities
        self.config.members.iter().find(|m| {
            step.agent_role
                .required_capabilities
                .iter()
                .all(|req| m.role.required_capabilities.contains(req))
        })
    }

    /// Whether the team has finished all work.
    pub fn is_done(&self) -> bool {
        self.task_list
            .tasks
            .iter()
            .all(|t| t.status == StepStatus::Completed || matches!(t.status, StepStatus::Failed { .. }) || t.status == StepStatus::Replanned)
    }

    /// Record a message exchange (for monitoring).
    pub fn record_message(&mut self) {
        self.messages_exchanged += 1;
    }

    /// Whether this team uses mesh communication (Agent Teams mode).
    pub fn is_mesh(&self) -> bool {
        self.config.communication == TeamCommunication::Mesh
    }

    /// Whether this team uses hub-spoke communication (Subagent mode).
    pub fn is_hub_spoke(&self) -> bool {
        self.config.communication == TeamCommunication::HubSpoke
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{AgentRole, PlanStrategy, StepExecution};

    fn team_plan() -> (ExecutionPlan, TeamConfig) {
        let root = PlanStep::new("setup", "Initialize");
        let mut impl_step = PlanStep::new("implement", "Write code")
            .with_role(AgentRole::new("coder").with_capabilities(vec!["file_edit".into()]))
            .with_depends_on(vec![root.id]);
        let mut test_step = PlanStep::new("test", "Write tests")
            .with_role(AgentRole::new("tester").with_capabilities(vec!["shell_exec".into()]))
            .with_depends_on(vec![root.id]);

        let plan = ExecutionPlan::new(
            vec![root, impl_step.clone(), test_step.clone()],
            PlanStrategy::Direct,
        );

        let config = TeamConfig {
            name: "feature-crew".into(),
            members: vec![
                TeamMember {
                    agent_id: None,
                    role: AgentRole::new("coder").with_capabilities(vec!["file_edit".into()]),
                    assigned_steps: vec![impl_step.id],
                },
                TeamMember {
                    agent_id: None,
                    role: AgentRole::new("tester").with_capabilities(vec!["shell_exec".into()]),
                    assigned_steps: vec![test_step.id],
                },
            ],
            communication: TeamCommunication::Mesh,
        };

        (plan, config)
    }

    #[test]
    fn task_list_from_plan() {
        let (plan, _) = team_plan();
        let list = SharedTaskList::from_plan(&plan);

        assert_eq!(list.tasks.len(), 3);
        let summary = list.summary();
        assert_eq!(summary.pending, 3);
        assert_eq!(summary.total, 3);
    }

    #[test]
    fn claimable_respects_dependencies() {
        let (plan, _) = team_plan();
        let list = SharedTaskList::from_plan(&plan);

        let claimable = list.claimable();
        // Only root (setup) is claimable — others depend on it
        assert_eq!(claimable.len(), 1);
        assert_eq!(claimable[0].name, "setup");
    }

    #[test]
    fn claim_and_complete() {
        let (plan, _) = team_plan();
        let mut list = SharedTaskList::from_plan(&plan);
        let agent = AgentId::new();

        let setup_id = list.tasks[0].step_id;

        // Claim setup
        assert!(list.claim(setup_id, agent));
        assert_eq!(list.tasks[0].status, StepStatus::InProgress);
        assert!(list.tasks[0].claimed_by.is_some());

        // Can't claim again
        assert!(!list.claim(setup_id, AgentId::new()));

        // Complete setup
        list.complete(setup_id);
        assert_eq!(list.tasks[0].status, StepStatus::Completed);

        let summary = list.summary();
        assert_eq!(summary.completed, 1);
        assert_eq!(summary.in_progress, 0);
    }

    #[test]
    fn sync_with_plan_updates_deps() {
        let (mut plan, _) = team_plan();
        let mut list = SharedTaskList::from_plan(&plan);

        // Complete root step in plan
        plan.steps[0].status = StepStatus::Completed;
        list.sync_with_plan(&plan);

        // Now implement and test should have deps met
        assert!(list.tasks[1].dependencies_met);
        assert!(list.tasks[2].dependencies_met);
    }

    #[test]
    fn team_coordinator_spawn() {
        let (plan, config) = team_plan();
        let mut coordinator = TeamCoordinator::new(&plan, config);
        let mut registry = AgentRegistry::new();

        let ids = coordinator.spawn_members(&mut registry);
        assert_eq!(ids.len(), 2);
        assert_eq!(registry.count(), 2);

        // Members should now have agent IDs
        assert!(coordinator.config.members[0].agent_id.is_some());
        assert!(coordinator.config.members[1].agent_id.is_some());
    }

    #[test]
    fn member_for_step_explicit_assignment() {
        let (plan, config) = team_plan();
        let coordinator = TeamCoordinator::new(&plan, config);

        let impl_step = &plan.steps[1];
        let member = coordinator.member_for_step(impl_step).unwrap();
        assert_eq!(member.role.name, "coder");
    }

    #[test]
    fn member_for_step_capability_fallback() {
        let (plan, mut config) = team_plan();
        // Clear explicit assignments
        for m in &mut config.members {
            m.assigned_steps.clear();
        }
        let coordinator = TeamCoordinator::new(&plan, config);

        let impl_step = &plan.steps[1]; // needs file_edit
        let member = coordinator.member_for_step(impl_step).unwrap();
        assert_eq!(member.role.name, "coder"); // matched by capability
    }

    #[test]
    fn team_is_mesh() {
        let (plan, config) = team_plan();
        let coordinator = TeamCoordinator::new(&plan, config);
        assert!(coordinator.is_mesh());
        assert!(!coordinator.is_hub_spoke());
    }

    #[test]
    fn team_is_done_when_all_terminal() {
        let (plan, config) = team_plan();
        let mut coordinator = TeamCoordinator::new(&plan, config);

        assert!(!coordinator.is_done());

        // Complete all tasks
        for task in &mut coordinator.task_list.tasks {
            task.status = StepStatus::Completed;
        }
        assert!(coordinator.is_done());
    }

    #[test]
    fn fail_task() {
        let (plan, _) = team_plan();
        let mut list = SharedTaskList::from_plan(&plan);
        let step_id = list.tasks[0].step_id;

        list.fail(step_id, "runtime error".into());
        assert!(matches!(list.tasks[0].status, StepStatus::Failed { .. }));

        let summary = list.summary();
        assert_eq!(summary.failed, 1);
    }

    #[test]
    fn message_tracking() {
        let (plan, config) = team_plan();
        let mut coordinator = TeamCoordinator::new(&plan, config);

        assert_eq!(coordinator.messages_exchanged, 0);
        coordinator.record_message();
        coordinator.record_message();
        assert_eq!(coordinator.messages_exchanged, 2);
    }
}
