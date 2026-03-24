use oco_shared_types::Budget;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Budget status levels with thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetStatus {
    /// All budgets within normal operating range (< 80%).
    Ok,
    /// At least one budget dimension is above 80%.
    Warning,
    /// At least one budget dimension is above 95%.
    Critical,
    /// At least one budget dimension is fully consumed.
    Exhausted,
}

/// Detailed breakdown of which budget dimensions triggered the status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetReport {
    pub status: BudgetStatus,
    pub token_utilization: f64,
    pub tool_call_utilization: f64,
    pub retrieval_utilization: f64,
    pub verify_cycle_utilization: f64,
    pub time_utilization: f64,
    /// Which dimension(s) triggered the highest status level.
    pub limiting_factors: Vec<String>,
}

/// Enforces budget constraints and produces status reports.
pub struct BudgetEnforcer {
    session_start: Instant,
}

const WARNING_THRESHOLD: f64 = 0.80;
const CRITICAL_THRESHOLD: f64 = 0.95;

impl BudgetEnforcer {
    pub fn new() -> Self {
        Self {
            session_start: Instant::now(),
        }
    }

    /// Create an enforcer with a specific start time (for testing or resumed sessions).
    pub fn with_start_time(start: Instant) -> Self {
        Self {
            session_start: start,
        }
    }

    /// Check all budget dimensions and return a comprehensive report.
    pub fn check(&self, budget: &Budget) -> BudgetReport {
        let token_util = Self::utilization(budget.tokens_used, budget.max_total_tokens);
        let tool_util =
            Self::utilization(budget.tool_calls_used as u64, budget.max_tool_calls as u64);
        let retrieval_util =
            Self::utilization(budget.retrievals_used as u64, budget.max_retrievals as u64);
        let verify_util = Self::utilization(
            budget.verify_cycles_used as u64,
            budget.max_verify_cycles as u64,
        );
        let elapsed_secs = self.session_start.elapsed().as_secs();
        let time_util = Self::utilization(elapsed_secs, budget.max_duration_secs);

        let utilizations = [
            (token_util, "tokens"),
            (tool_util, "tool_calls"),
            (retrieval_util, "retrievals"),
            (verify_util, "verify_cycles"),
            (time_util, "time"),
        ];

        let mut status = BudgetStatus::Ok;
        let mut limiting_factors = Vec::new();

        for &(util, name) in &utilizations {
            let level = Self::status_for_utilization(util);
            if level > status {
                status = level;
                limiting_factors.clear();
                limiting_factors.push(name.to_string());
            } else if level == status && level > BudgetStatus::Ok {
                limiting_factors.push(name.to_string());
            }
        }

        BudgetReport {
            status,
            token_utilization: token_util,
            tool_call_utilization: tool_util,
            retrieval_utilization: retrieval_util,
            verify_cycle_utilization: verify_util,
            time_utilization: time_util,
            limiting_factors,
        }
    }

    /// Quick check: is any budget dimension exhausted?
    pub fn is_exhausted(&self, budget: &Budget) -> bool {
        self.check(budget).status == BudgetStatus::Exhausted
    }

    /// Quick check: is any budget dimension critical or worse?
    pub fn is_critical(&self, budget: &Budget) -> bool {
        self.check(budget).status >= BudgetStatus::Critical
    }

    /// Check if a proposed token expenditure would exceed the budget.
    pub fn can_afford_tokens(&self, budget: &Budget, tokens: u64) -> bool {
        budget.tokens_used + tokens <= budget.max_total_tokens
    }

    /// Check if a tool call can be made within budget.
    pub fn can_afford_tool_call(&self, budget: &Budget) -> bool {
        budget.tool_calls_used < budget.max_tool_calls
    }

    /// Check if a retrieval can be made within budget.
    pub fn can_afford_retrieval(&self, budget: &Budget) -> bool {
        budget.retrievals_used < budget.max_retrievals
    }

    /// Check if a verify cycle can be made within budget.
    pub fn can_afford_verify(&self, budget: &Budget) -> bool {
        budget.verify_cycles_used < budget.max_verify_cycles
    }

    fn utilization(used: u64, max: u64) -> f64 {
        if max == 0 {
            return 1.0; // Zero-budget means immediately exhausted.
        }
        used as f64 / max as f64
    }

    fn status_for_utilization(util: f64) -> BudgetStatus {
        if util >= 1.0 {
            BudgetStatus::Exhausted
        } else if util >= CRITICAL_THRESHOLD {
            BudgetStatus::Critical
        } else if util >= WARNING_THRESHOLD {
            BudgetStatus::Warning
        } else {
            BudgetStatus::Ok
        }
    }
}

impl Default for BudgetEnforcer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_budget(tokens_pct: f64, tools_pct: f64) -> Budget {
        let mut b = Budget::default();
        b.tokens_used = (b.max_total_tokens as f64 * tokens_pct) as u64;
        b.tool_calls_used = (b.max_tool_calls as f64 * tools_pct) as u32;
        b
    }

    #[test]
    fn ok_budget() {
        let enforcer = BudgetEnforcer::new();
        let budget = make_budget(0.5, 0.3);
        let report = enforcer.check(&budget);
        assert_eq!(report.status, BudgetStatus::Ok);
        assert!(report.limiting_factors.is_empty());
    }

    #[test]
    fn warning_budget() {
        let enforcer = BudgetEnforcer::new();
        let budget = make_budget(0.85, 0.3);
        let report = enforcer.check(&budget);
        assert_eq!(report.status, BudgetStatus::Warning);
        assert!(report.limiting_factors.contains(&"tokens".to_string()));
    }

    #[test]
    fn critical_budget() {
        let enforcer = BudgetEnforcer::new();
        let budget = make_budget(0.97, 0.3);
        let report = enforcer.check(&budget);
        assert_eq!(report.status, BudgetStatus::Critical);
    }

    #[test]
    fn exhausted_budget() {
        let enforcer = BudgetEnforcer::new();
        let budget = make_budget(1.0, 0.3);
        let report = enforcer.check(&budget);
        assert_eq!(report.status, BudgetStatus::Exhausted);
    }

    #[test]
    fn can_afford_checks() {
        let enforcer = BudgetEnforcer::new();
        let budget = make_budget(0.99, 0.99);
        // Should still be able to afford a small amount
        assert!(enforcer.can_afford_tokens(&budget, 1000));
        // But not a huge amount
        assert!(!enforcer.can_afford_tokens(&budget, budget.max_total_tokens));
    }
}
