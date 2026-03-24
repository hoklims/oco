//! Policy engine for the Open Context Orchestrator.
//!
//! This crate provides deterministic, rule-based decision-making for the
//! orchestration loop. No LLM calls — all logic is heuristic and reproducible.
//!
//! # Modules
//!
//! - [`classifier`] — Task complexity classification via keyword heuristics
//! - [`selector`] — Action selection based on orchestration state and policy rules
//! - [`budget`] — Budget enforcement with warning/critical/exhausted thresholds
//! - [`gates`] — Write action gates for destructive operation control
//! - [`knowledge`] — Knowledge boundary estimation heuristics

pub mod budget;
pub mod classifier;
pub mod gates;
pub mod knowledge;
pub mod scheduler;
pub mod secret_scanner;
pub mod selector;

pub use budget::{BudgetEnforcer, BudgetReport, BudgetStatus};
pub use classifier::TaskClassifier;
pub use gates::{PolicyGate, WritePolicy};
pub use knowledge::KnowledgeBoundaryEstimator;
pub use scheduler::{SchedulableAction, Schedule, SchedulerError};
pub use secret_scanner::{SecretScanResult, scan_secrets};
pub use selector::{
    ActionDecision, ActionSelector, DefaultActionSelector, PolicyState, ScoredAlternative,
};
