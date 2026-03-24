"""Evaluation scenario definitions."""

from __future__ import annotations

from enum import Enum
from typing import Any

from pydantic import BaseModel, Field


class ScenarioCategory(str, Enum):
    CODE_NAVIGATION = "code_navigation"
    REPO_QA = "repo_qa"
    GUIDED_REFACTOR = "guided_refactor"
    DEBUG = "debug"
    TEST_GUIDED = "test_guided"


class ExpectedAction(BaseModel):
    """An expected action in the orchestration trace."""

    action_type: str
    must_contain: dict[str, Any] = Field(default_factory=dict)
    order: int | None = None


class EvalScenario(BaseModel):
    """A single evaluation scenario."""

    id: str
    name: str
    category: ScenarioCategory
    description: str
    user_request: str
    workspace_path: str
    expected_actions: list[ExpectedAction] = Field(default_factory=list)
    expected_outcome: str
    max_steps: int = 25
    timeout_secs: int = 120


class EvalResult(BaseModel):
    """Result of running an evaluation scenario."""

    scenario_id: str
    passed: bool
    actual_steps: int
    actual_actions: list[str]
    outcome: str
    duration_ms: int
    notes: list[str] = Field(default_factory=list)
