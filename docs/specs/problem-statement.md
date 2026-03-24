# Open Context Orchestrator — Problem Statement

## For Expert Review: Why an External Orchestration Layer Can Outperform LLM-Driven Coding Agents

**Author**: OCO Project
**Date**: March 2026
**Status**: Draft for expert review
**Audience**: Senior systems architects, AI infrastructure engineers, LLM product leads

---

## 1. The Core Problem

Every major AI coding assistant today — Claude Code, Cursor, Windsurf, GitHub Copilot Agent Mode, Aider — uses the **same fundamental architecture**: the LLM itself controls the entire decision loop.

When a developer asks "fix the authentication bug", the model decides:
- Whether to retrieve files (and which ones)
- Whether to run a search (and what query)
- Whether to call a tool (and with what arguments)
- Whether to verify its work (and how)
- Whether to respond or keep iterating

**Every one of these routing decisions is a forward pass through the LLM.** Each decision consumes tokens, incurs latency, and operates without cost awareness, verification discipline, or learned heuristics from prior sessions.

This is architecturally equivalent to having a surgeon also do the scheduling, the inventory management, the billing, and the quality assurance — all while performing surgery.

---

## 2. Quantified Inefficiencies

### 2.1 Token Waste on Routing Decisions

Research from SupervisorAgent (arXiv:2510.26585) demonstrates that an external meta-agent monitoring LLM decisions — without modifying the base agent — reduces token consumption by **29.68% on SWE-bench tasks** and **39.36% on tool-intensive multi-agent systems**, while maintaining or improving success rates. The oversight agent's own overhead was only 15.45% of total tokens.

This means current LLM-driven agents waste roughly **30-40% of their token budget** on suboptimal routing, redundant exploration, and action loops that a structured orchestrator would prevent.

### 2.2 Context Window Stuffing

Attention cost scales **quadratically** with input length. Doubling context from 64K to 128K tokens quadruples compute cost. Current agents frequently stuff context with unranked code chunks — 200 chunks at 250 tokens each (50,000 tokens) when 10 ML-ranked chunks (2,500 tokens) would deliver better relevance. That's a **20x cost reduction** on retrieval alone (Shaped.ai research on context window optimization).

### 2.3 Session Cost Reality

Claude Code averages ~$6/developer/day, with power users hitting $12+. Reports of "4 hours of usage gone in 3 prompts" during architectural refactoring are common. Monthly allocations exhausted in single sessions forced Anthropic to introduce weekly rate limits in August 2025 — a reactive measure that addresses symptoms, not the architectural cause.

### 2.4 Compression Information Loss

Factory.ai's research on context compression shows that unstructured summarization loses critical details across compression cycles. File paths, architectural decisions, and attempted approaches silently vanish. Their structured anchored summarization (with explicit sections for modified files, decisions made, next steps) scored **0.45 points higher** on continuation accuracy than Anthropic's built-in compression — because **structure forces preservation**.

### 2.5 Multi-Agent Coordination Overhead

Research on scaling multi-agent systems (arXiv:2512.08296) found that on sequential coding tasks, all multi-agent variants **degrade performance by -39% to -70%** compared to single-agent systems, despite additional compute budget. The coordination overhead (message routing, synchronization, error propagation) exceeds parallelism benefits for tasks with inherent sequential dependencies — which describes most coding work.

---

## 3. The Architectural Thesis

**An external, deterministic orchestration layer sitting between the IDE and the LLM can make better routing decisions than the LLM itself, at negligible cost.**

This is not a claim that LLMs are bad at coding. It is a claim that LLMs are **wasteful at orchestrating themselves** because:

1. **Routing decisions don't require intelligence** — "Has context been retrieved for this type of task?" is a boolean check, not a reasoning problem. Running it through a 200B-parameter model is a 10,000x overhead over a rule engine.

2. **Budget awareness requires state tracking, not reasoning** — "Am I spending too many tokens on retrieval?" requires counters and thresholds, not natural language understanding.

3. **Verification discipline requires policy, not judgment** — "Should I run tests after modifying this file?" is a policy decision based on file type and change risk, not a creative problem.

4. **Loop detection requires memory, not intelligence** — "Have I already tried this approach and failed?" requires comparing current state against history, not generating novel insights.

5. **Cost-optimal model routing requires metadata, not reasoning** — "Should this sub-task use Haiku or Opus?" depends on measured complexity, not on the model's self-assessment.

### The Five-Action Model

At each step, the orchestrator selects exactly one action from:

| Action | Decision Basis | LLM Required? |
|--------|---------------|---------------|
| **Respond** | Confidence threshold met, context sufficient | No (deterministic) |
| **Retrieve** | Missing context for task type, low confidence | No (deterministic) |
| **Tool Call** | Task requires execution, context retrieved | No (deterministic) |
| **Verify** | Write action completed, risk level warrants it | No (deterministic) |
| **Stop** | Budget exhausted, task complete, error limit | No (deterministic) |

The LLM is used **only** for content generation (the actual response), never for routing. This eliminates ~30% of token consumption on routing decisions.

---

## 4. What OCO Does Differently

### 4.1 Deterministic Policy Engine (Zero LLM Cost for Routing)

OCO's policy engine scores each action candidate using:
- **Task complexity classification** (keyword heuristic, 5 tiers: Trivial→Critical)
- **Knowledge boundary estimation** (6 signals: complexity, file paths, retrieval status, error count, observation quality, request specificity)
- **Budget enforcement** (token/time/tool/retrieval/verify budgets with Warning/Critical/Exhausted thresholds)
- **Write-risk gating** (keyword detection + PolicyGate with registered tool risk levels)
- **Diminishing returns** on repeated actions (retrieval penalty after 3+ attempts)

All scoring is deterministic, auditable, and takes <1ms. No tokens consumed.

### 4.2 Structured Observation Pipeline

Every tool output, retrieval result, and verification result is normalized into a typed `Observation` with:
- Source attribution
- Token cost estimate
- Relevance score
- Structured payload (not raw text)

This prevents the "lost in the middle" problem: observations are ranked by relevance and priority before being assembled into context.

### 4.3 Budget-Aware Context Assembly

OCO's context engine:
- Sorts items by priority tier (System > Pinned > High > Medium > Low > Summary)
- Greedy-fills within a strict token budget
- Deduplicates overlapping code snippets
- Truncates with head+tail preservation
- Reports what was excluded and why

This replaces the "dump everything into context and hope" approach with auditable, budgeted assembly.

### 4.4 Structured Decision Traces

Every step produces a `DecisionTrace` containing:
- The selected action and its score
- All alternatives considered with their scores
- Budget snapshot at decision time
- Knowledge confidence at decision time
- Duration in milliseconds

This makes orchestration behavior **fully auditable and replayable** — something impossible with pure LLM-driven decisions.

### 4.5 Graceful Degradation

OCO works without:
- An ML worker (falls back to keyword-based retrieval and heuristic reranking)
- A specific LLM provider (provider-agnostic: Anthropic, Ollama, any OpenAI-compatible API)
- An IDE extension (CLI-first, server mode for integrations)

---

## 5. The Integration Challenge

### 5.1 Current State

Claude Code's architecture does not expose hooks for external decision verification. The LLM controls the tool loop directly. There is no standardized way to:
- Intercept a tool call before execution
- Inject pre-ranked context before the model retrieves
- Override a routing decision with a cheaper one
- Enforce a verification step after a write action

### 5.2 MCP as Integration Surface

The Model Context Protocol provides a partial solution. OCO can expose itself as an MCP server, giving Claude Code access to:
- `oco_search` — OCO's ranked retrieval instead of Claude Code's raw file reads
- `oco_orchestrate` — OCO's full orchestration loop for complex tasks
- `oco_trace` — Decision traces for debugging and auditing

However, this is **tool-level integration**, not **decision-level integration**. Claude Code still decides *when* to call OCO's tools. The optimal architecture would be the inverse: OCO decides when to call the LLM.

### 5.3 The Ideal Architecture

```
Developer → IDE → OCO Orchestrator → LLM (only for generation)
                       ↓
              Policy Engine (routing)
              Context Engine (assembly)
              Retrieval Engine (search)
              Tool Runtime (execution)
              Verifier (validation)
              Telemetry (traces)
```

In this model, the LLM is a **compute resource**, not a **decision maker**. OCO handles all routing, budgeting, context assembly, verification, and loop control. The LLM receives a pre-assembled, budget-optimized, deduplicated context window and generates content — the one thing it's uniquely good at.

### 5.4 What Would Need to Change in Claude Code

For deep integration, Claude Code would need to expose:

1. **A pre-tool-call hook** — "OCO, should I execute this tool call?" → allows policy gating
2. **A context injection point** — "OCO, what context should I include?" → allows ranked retrieval
3. **A post-action hook** — "OCO, should I verify this result?" → allows verification discipline
4. **A model routing interface** — "OCO, which model should handle this sub-task?" → allows cost optimization
5. **A session state export** — "OCO, here's my current state" → allows external telemetry and replay

None of these exist today. The closest mechanism is MCP tool exposure, which provides tool-level but not decision-level integration.

---

## 6. Empirical Evidence for the Approach

| Source | Finding | Relevance |
|--------|---------|-----------|
| **SupervisorAgent** (arXiv:2510.26585) | External meta-agent reduces token usage 29-39% on SWE-bench | Direct proof that external orchestration outperforms self-orchestration |
| **Factory.ai** compression research | Structured summarization preserves 0.45 points more context quality | Validates OCO's structured observation pipeline |
| **Shaped.ai** context optimization | ML-ranked retrieval reduces context tokens 20x vs stuffing | Validates OCO's budget-aware context assembly |
| **TALE** (arXiv:2412.18547) | Token-budget-aware reasoning reduces output costs 67% | Validates OCO's explicit budget enforcement |
| **RouteLLM** framework | Cost-aware routing achieves 95% of GPT-4 quality at 14% of cost | Validates OCO's per-step model selection approach |
| **mini-SWE-agent** | 100-line agent achieves 65% SWE-bench (vs 72% for Claude Code) | Proves simpler orchestration can match complex frameworks |
| **Multi-agent scaling** (arXiv:2512.08296) | Multi-agent degrades -39% to -70% on sequential tasks | Validates single-orchestrator over multi-agent approaches |
| **mcp-cli** dynamic discovery | 99% token reduction on tool schemas (47K→400 tokens) | Validates OCO's dynamic over static tool integration |

---

## 7. Open Questions for Expert Review

1. **Is the deterministic routing thesis correct?** Can heuristic-based action selection truly outperform LLM-based routing for coding tasks, or are there task categories where LLM routing is strictly superior?

2. **What is the optimal integration surface?** MCP tool exposure, IDE extension hooks, proxy-based interception, or a new protocol entirely?

3. **How should the knowledge boundary estimator be calibrated?** OCO uses 6 heuristic signals — is this sufficient, or should it use a lightweight ML model trained on session outcomes?

4. **What is the ceiling for deterministic orchestration?** At what task complexity does the rigid five-action model break down and require LLM-driven planning?

5. **Is the cost argument sufficient?** If LLM inference costs continue to drop 10x/year, does the orchestration overhead optimization become irrelevant?

6. **How should verification loops be structured for coding tasks specifically?** Build→test→lint is obvious, but what about semantic correctness, architectural coherence, and security review?

7. **Can session-level learning improve the policy engine?** If OCO tracks which decisions led to successful outcomes, can it evolve heuristics per-codebase or per-developer without online learning?

---

## 8. Current Implementation Status

OCO v1 is implemented as a Rust/Python/TypeScript polyglot monorepo:
- **90 tests passing**, 0 failures
- **~10,500 lines of Rust**, ~500 Python, ~310 TypeScript
- **12 Rust crates** covering: policy engine, context engine, retrieval (SQLite FTS5), code intelligence, tool runtime, verifier, telemetry, MCP server, orchestrator core
- **3 LLM providers**: Anthropic API, Ollama (local), Stub (development)
- **Functional CLI**: index, search, run, serve, init
- **HTTP/MCP server** with persistent sessions
- **Reviewed by GPT-5.4 Thinking** with all CRITICAL and HIGH findings remediated

The system is architecturally complete but not yet battle-tested on real-world codebases at scale.

---

## 9. The Ask

We are seeking expert review on:

1. The validity of the architectural thesis (deterministic orchestration > LLM self-orchestration)
2. The optimal integration path with existing tools (Claude Code, Cursor, etc.)
3. The calibration strategy for the policy engine
4. The evaluation methodology (how to prove OCO makes sessions cheaper/faster/better)
5. Whether this approach generalizes beyond coding to other agentic domains

---

*This document references research from: Anthropic (building-effective-agents), SupervisorAgent (arXiv:2510.26585), Factory.ai (compression evaluation), Shaped.ai (context optimization), TALE (arXiv:2412.18547), RouteLLM, mini-SWE-agent, multi-agent scaling (arXiv:2512.08296), and the MCP specification (2025-06-18).*
