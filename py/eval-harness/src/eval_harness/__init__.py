"""OCO Evaluation Harness — test orchestration quality against scenarios."""

__version__ = "0.1.0"

from .runner import run_scenarios, scenarios_to_jsonl
from .scenario import EvalResult, EvalScenario, ExpectedAction, ScenarioCategory

__all__ = [
    "EvalResult",
    "EvalScenario",
    "ExpectedAction",
    "ScenarioCategory",
    "run_scenarios",
    "scenarios_to_jsonl",
]
