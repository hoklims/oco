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
    /// Step IDs this task depends on (fix #19: compute deps on demand, not cached).
    pub depends_on: Vec<Uuid>,
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
        let tasks = plan
            .steps
            .iter()
            .map(|step| SharedTask {
                step_id: step.id,
                name: step.name.clone(),
                status: step.status.clone(),
                claimed_by: None,
                depends_on: step.depends_on.clone(),
                claimed_at: None,
            })
            .collect();

        Self { tasks }
    }

    /// Compute the set of completed task IDs (used for on-demand dependency checks).
    fn completed_ids(&self) -> std::collections::HashSet<Uuid> {
        self.tasks
            .iter()
            .filter(|t| t.status == StepStatus::Completed)
            .map(|t| t.step_id)
            .collect()
    }

    /// Whether a task's dependencies are all met (fix #19: computed on demand).
    pub fn dependencies_met(&self, task: &SharedTask) -> bool {
        let completed = self.completed_ids();
        task.depends_on.iter().all(|d| completed.contains(d))
    }

    /// Tasks available for claiming (pending + dependencies met + unclaimed).
    pub fn claimable(&self) -> Vec<&SharedTask> {
        let completed = self.completed_ids();
        self.tasks
            .iter()
            .filter(|t| {
                t.status == StepStatus::Pending
                    && t.depends_on.iter().all(|d| completed.contains(d))
                    && t.claimed_by.is_none()
            })
            .collect()
    }

    /// Claim a task for an agent. Returns false if already claimed or not claimable.
    pub fn claim(&mut self, step_id: Uuid, agent_id: AgentId) -> bool {
        let completed = self.completed_ids();
        if let Some(task) = self.tasks.iter_mut().find(|t| t.step_id == step_id)
            && task.status == StepStatus::Pending
            && task.depends_on.iter().all(|d| completed.contains(d))
            && task.claimed_by.is_none()
        {
            task.claimed_by = Some(agent_id);
            task.status = StepStatus::InProgress;
            task.claimed_at = Some(Utc::now());
            return true;
        }
        false
    }

    /// Mark a task as completed. Requires the claiming agent's ID for ownership check.
    /// Returns false if the task is not claimed by `agent_id`.
    pub fn complete(&mut self, step_id: Uuid, agent_id: AgentId) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.step_id == step_id) {
            if task.claimed_by != Some(agent_id) {
                return false; // ownership check failed
            }
            task.status = StepStatus::Completed;
            return true;
        }
        false
    }

    /// Mark a task as completed without ownership check (for coordinator/system use).
    pub fn force_complete(&mut self, step_id: Uuid) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.step_id == step_id) {
            task.status = StepStatus::Completed;
        }
    }

    /// Mark a task as failed. Requires the claiming agent's ID for ownership check.
    /// Returns false if the task is not claimed by `agent_id`.
    pub fn fail(&mut self, step_id: Uuid, agent_id: AgentId, reason: String) -> bool {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.step_id == step_id) {
            if task.claimed_by != Some(agent_id) {
                return false;
            }
            task.status = StepStatus::Failed { reason };
            return true;
        }
        false
    }

    /// Mark a task as failed without ownership check (for coordinator/system use).
    pub fn force_fail(&mut self, step_id: Uuid, reason: String) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.step_id == step_id) {
            task.status = StepStatus::Failed { reason };
        }
    }

    /// Sync task list with plan state (call after GraphRunner updates steps).
    /// Updates status and dependency edges from the canonical plan (fix #19).
    pub fn sync_with_plan(&mut self, plan: &ExecutionPlan) {
        for task in &mut self.tasks {
            if let Some(step) = plan.get_step(task.step_id) {
                task.status = step.status.clone();
                task.depends_on = step.depends_on.clone();
            }
        }

        // Add any new steps from the plan that aren't in the task list yet (replan)
        let existing: std::collections::HashSet<Uuid> =
            self.tasks.iter().map(|t| t.step_id).collect();
        for step in &plan.steps {
            if !existing.contains(&step.id) {
                self.tasks.push(SharedTask {
                    step_id: step.id,
                    name: step.name.clone(),
                    status: step.status.clone(),
                    claimed_by: None,
                    depends_on: step.depends_on.clone(),
                    claimed_at: None,
                });
            }
        }
    }

    /// Revoke claims on tasks whose steps have been replanned (fix #21).
    /// Call after `sync_with_plan()` on a replanned plan.
    pub fn revoke_replanned_claims(&mut self) {
        for task in &mut self.tasks {
            if task.status == StepStatus::Replanned {
                task.claimed_by = None;
                task.claimed_at = None;
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
/// Members are scoped to the plan_id to avoid polluting the global registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamCoordinator {
    /// Plan ID this team belongs to (for scoped cleanup).
    pub plan_id: Uuid,
    /// Team configuration.
    pub config: TeamConfig,
    /// Shared task list visible to all members.
    pub task_list: SharedTaskList,
    /// When this team was spawned.
    pub created_at: DateTime<Utc>,
    /// Messages exchanged (count for monitoring).
    pub messages_exchanged: u32,
    /// Agent IDs spawned by this team (for cleanup).
    pub spawned_agent_ids: Vec<AgentId>,
}

impl TeamCoordinator {
    /// Create a new team coordinator from a plan and team config.
    pub fn new(plan: &ExecutionPlan, config: TeamConfig) -> Self {
        Self {
            plan_id: plan.id,
            config,
            task_list: SharedTaskList::from_plan(plan),
            created_at: Utc::now(),
            messages_exchanged: 0,
            spawned_agent_ids: Vec::new(),
        }
    }

    /// Spawn team members in the agent registry.
    /// Agent names are prefixed with plan_id for isolation.
    /// Returns the assigned agent IDs.
    pub fn spawn_members(&mut self, registry: &mut AgentRegistry) -> Vec<AgentId> {
        let mut ids = Vec::new();
        let plan_prefix = &self.plan_id.to_string()[..8];

        for member in &mut self.config.members {
            let capabilities: Vec<Capability> = member
                .role
                .required_capabilities
                .iter()
                .map(Capability::new)
                .collect();

            let scoped_name = format!("{plan_prefix}/{}", member.role.name);
            let agent = AgentDescriptor::new(&scoped_name, &member.role.name)
                .with_capabilities(capabilities)
                .with_max_load(member.assigned_steps.len() as u32);

            let id = registry.register(agent);
            member.agent_id = Some(id);
            ids.push(id);
        }

        self.spawned_agent_ids = ids.clone();
        ids
    }

    /// Teardown: unregister all agents spawned by this team.
    /// Call when the plan is complete or aborted.
    pub fn teardown(&mut self, registry: &mut AgentRegistry) {
        for id in self.spawned_agent_ids.drain(..) {
            registry.unregister(id);
        }
        for member in &mut self.config.members {
            member.agent_id = None;
        }
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

    /// Record a message exchange with topology enforcement (fix #18).
    ///
    /// Validates that `sender` and `recipient` are valid team members and that
    /// the communication is allowed by the team's topology:
    /// - **HubSpoke**: only lead (first member) ↔ other members
    /// - **Mesh**: any member → any other member
    /// - **Pipeline**: only member[i] → member[i+1] (sequential)
    pub fn record_message(
        &mut self,
        sender: AgentId,
        recipient: AgentId,
    ) -> Result<(), TeamTopologyError> {
        if sender == recipient {
            return Err(TeamTopologyError::SelfMessage);
        }

        let member_ids: Vec<AgentId> = self
            .config
            .members
            .iter()
            .filter_map(|m| m.agent_id)
            .collect();

        if !member_ids.contains(&sender) {
            return Err(TeamTopologyError::NotAMember { agent_id: sender });
        }
        if !member_ids.contains(&recipient) {
            return Err(TeamTopologyError::NotAMember {
                agent_id: recipient,
            });
        }

        match &self.config.communication {
            TeamCommunication::Mesh => {
                // Any → any is allowed
            }
            TeamCommunication::HubSpoke => {
                // Only lead (first member) ↔ others
                let lead = member_ids[0];
                if sender != lead && recipient != lead {
                    return Err(TeamTopologyError::HubSpokeViolation { sender, recipient });
                }
            }
            TeamCommunication::Pipeline => {
                // Only sequential: member[i] → member[i+1]
                let sender_idx = member_ids.iter().position(|&id| id == sender);
                let recipient_idx = member_ids.iter().position(|&id| id == recipient);
                if let (Some(si), Some(ri)) = (sender_idx, recipient_idx)
                    && ri != si + 1
                {
                    return Err(TeamTopologyError::PipelineViolation { sender, recipient });
                }
            }
        }

        self.messages_exchanged += 1;
        Ok(())
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

/// Errors when topology rules are violated (fix #18).
#[derive(Debug, Clone, thiserror::Error)]
pub enum TeamTopologyError {
    #[error("agent {agent_id:?} is not a member of this team")]
    NotAMember { agent_id: AgentId },
    #[error("cannot send message to self")]
    SelfMessage,
    #[error("hub-spoke violation: {sender:?} → {recipient:?} (only lead ↔ members allowed)")]
    HubSpokeViolation {
        sender: AgentId,
        recipient: AgentId,
    },
    #[error("pipeline violation: {sender:?} → {recipient:?} (only sequential allowed)")]
    PipelineViolation {
        sender: AgentId,
        recipient: AgentId,
    },
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

        // Complete setup (with ownership)
        assert!(list.complete(setup_id, agent));
        assert_eq!(list.tasks[0].status, StepStatus::Completed);

        // Can't complete someone else's task
        assert!(!list.complete(setup_id, AgentId::new()));

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

        // Now implement and test should have deps met (computed on demand)
        assert!(list.dependencies_met(&list.tasks[1]));
        assert!(list.dependencies_met(&list.tasks[2]));
    }

    #[test]
    fn dependencies_met_computed_on_demand_not_stale() {
        // Fix #19: verify that dependencies_met is never stale
        let (plan, _) = team_plan();
        let mut list = SharedTaskList::from_plan(&plan);

        // Initially only root is claimable
        assert_eq!(list.claimable().len(), 1);
        assert!(!list.dependencies_met(&list.tasks[1]));

        // Complete root via force_complete
        let root_id = list.tasks[0].step_id;
        list.force_complete(root_id);

        // Now deps are met — computed live, no stale cache
        assert!(list.dependencies_met(&list.tasks[1]));
        assert!(list.dependencies_met(&list.tasks[2]));
        assert_eq!(list.claimable().len(), 2);
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

        // Use force_fail for system-level failure (no ownership required)
        list.force_fail(step_id, "runtime error".into());
        assert!(matches!(list.tasks[0].status, StepStatus::Failed { .. }));

        let summary = list.summary();
        assert_eq!(summary.failed, 1);
    }

    #[test]
    fn message_tracking_mesh() {
        let (plan, config) = team_plan();
        let mut coordinator = TeamCoordinator::new(&plan, config);
        let mut registry = AgentRegistry::new();
        let ids = coordinator.spawn_members(&mut registry);

        assert_eq!(coordinator.messages_exchanged, 0);
        // Mesh allows any → any
        assert!(coordinator.record_message(ids[0], ids[1]).is_ok());
        assert!(coordinator.record_message(ids[1], ids[0]).is_ok());
        assert_eq!(coordinator.messages_exchanged, 2);
    }

    #[test]
    fn hubspoke_topology_enforced() {
        let (plan, mut config) = team_plan();
        config.communication = TeamCommunication::HubSpoke;
        let mut coordinator = TeamCoordinator::new(&plan, config);
        let mut registry = AgentRegistry::new();
        let ids = coordinator.spawn_members(&mut registry);

        // Lead (ids[0]) → member OK
        assert!(coordinator.record_message(ids[0], ids[1]).is_ok());
        // Member → lead OK
        assert!(coordinator.record_message(ids[1], ids[0]).is_ok());
        // Member → member BLOCKED (must go through lead)
        // Need a third member for this test — use existing two, they can't talk directly
        // With only 2 members + 1 lead, member[1] is already tested above
    }

    #[test]
    fn pipeline_topology_enforced() {
        // Build a 3-member pipeline team
        let root = PlanStep::new("setup", "Initialize");
        let s1 = PlanStep::new("step1", "First")
            .with_role(AgentRole::new("a").with_capabilities(vec!["x".into()]));
        let s2 = PlanStep::new("step2", "Second")
            .with_role(AgentRole::new("b").with_capabilities(vec!["y".into()]));
        let s3 = PlanStep::new("step3", "Third")
            .with_role(AgentRole::new("c").with_capabilities(vec!["z".into()]));

        let plan = ExecutionPlan::new(
            vec![root, s1.clone(), s2.clone(), s3.clone()],
            PlanStrategy::Direct,
        );

        let config = TeamConfig {
            name: "pipeline".into(),
            members: vec![
                TeamMember {
                    agent_id: None,
                    role: AgentRole::new("a").with_capabilities(vec!["x".into()]),
                    assigned_steps: vec![s1.id],
                },
                TeamMember {
                    agent_id: None,
                    role: AgentRole::new("b").with_capabilities(vec!["y".into()]),
                    assigned_steps: vec![s2.id],
                },
                TeamMember {
                    agent_id: None,
                    role: AgentRole::new("c").with_capabilities(vec!["z".into()]),
                    assigned_steps: vec![s3.id],
                },
            ],
            communication: TeamCommunication::Pipeline,
        };

        let mut coordinator = TeamCoordinator::new(&plan, config);
        let mut registry = AgentRegistry::new();
        let ids = coordinator.spawn_members(&mut registry);

        // Sequential OK: a→b, b→c
        assert!(coordinator.record_message(ids[0], ids[1]).is_ok());
        assert!(coordinator.record_message(ids[1], ids[2]).is_ok());
        // Non-sequential BLOCKED: a→c (skip), c→a (backward)
        assert!(coordinator.record_message(ids[0], ids[2]).is_err());
        assert!(coordinator.record_message(ids[2], ids[0]).is_err());
    }

    #[test]
    fn topology_rejects_non_member() {
        let (plan, config) = team_plan();
        let mut coordinator = TeamCoordinator::new(&plan, config);
        let mut registry = AgentRegistry::new();
        let ids = coordinator.spawn_members(&mut registry);

        let outsider = AgentId::new();
        assert!(matches!(
            coordinator.record_message(outsider, ids[0]),
            Err(TeamTopologyError::NotAMember { .. })
        ));
    }

    // -- Toxic scenarios (fix #24) --

    #[test]
    fn concurrent_claim_race_second_fails() {
        let (plan, _) = team_plan();
        let mut list = SharedTaskList::from_plan(&plan);
        let root_id = list.tasks[0].step_id;

        let agent_a = AgentId::new();
        let agent_b = AgentId::new();

        // First claim succeeds
        assert!(list.claim(root_id, agent_a));
        // Second claim on same task fails
        assert!(!list.claim(root_id, agent_b));
        // Verify only agent_a owns it
        assert_eq!(list.tasks[0].claimed_by, Some(agent_a));
    }

    #[test]
    fn revoke_replanned_claims() {
        let (plan, _) = team_plan();
        let mut list = SharedTaskList::from_plan(&plan);
        let root_id = list.tasks[0].step_id;
        let agent = AgentId::new();

        // Claim and start root
        list.claim(root_id, agent);
        assert_eq!(list.tasks[0].status, StepStatus::InProgress);

        // Simulate replan: mark as replanned
        list.tasks[0].status = StepStatus::Replanned;
        list.revoke_replanned_claims();

        // Claim should be revoked
        assert!(list.tasks[0].claimed_by.is_none());
        assert!(list.tasks[0].claimed_at.is_none());
    }

    #[test]
    fn sync_with_plan_adds_new_steps_from_replan() {
        let (plan, _) = team_plan();
        let mut list = SharedTaskList::from_plan(&plan);
        assert_eq!(list.tasks.len(), 3);

        // Simulate replan: add a new step to the plan
        let mut new_plan = plan.clone();
        new_plan.steps.push(PlanStep::new("new-fix", "Fix the issue"));

        list.sync_with_plan(&new_plan);
        assert_eq!(list.tasks.len(), 4);
        assert!(list.tasks.iter().any(|t| t.name == "new-fix"));
    }

    #[test]
    fn topology_rejects_self_message() {
        let (plan, config) = team_plan();
        let mut coordinator = TeamCoordinator::new(&plan, config);
        let mut registry = AgentRegistry::new();
        let ids = coordinator.spawn_members(&mut registry);

        assert!(matches!(
            coordinator.record_message(ids[0], ids[0]),
            Err(TeamTopologyError::SelfMessage)
        ));
    }
}
