# ADR-007: Hooks-First Runtime — MVP Validation

## Status
Accepted

## Context
OCO integrates with Claude Code via HTTP hooks (`.claude/hooks/`), forming a
"hooks-first runtime" where orchestration events flow from Claude Code into OCO
in real time.  The runtime includes:

- **Pre/post tool-use hooks** — validation, observation recording
- **User-prompt-submit hook** — session init, contract enforcement
- **Stop hook** — session teardown
- **Skills** — `/oco-verify-fix`, `/oco-safe-refactor`, `/oco-investigate-bug`,
  `/oco-inspect-repo-area`, `/oco-trace-stack`
- **MCP bridge** — exposes OCO tools as MCP resources

The question was whether this runtime adds value without creating friction that
blocks Claude from working naturally.

## Validation Protocol

Four progressively harder tasks were executed in a single fresh session, each
measuring: hook errors, flow naturalness, correctness, and verification
completeness.

### Test 1 — Bugfix
**Task**: Identify and fix a real bug in the codebase.
**Result**: Found `estimated_total_tokens()` returning `u32` (mismatched with
spec declaring `u64`).  Fixed return type across 3 files.  Build, clippy, 489
tests green.
**Observations**: Investigation before patch. Minimal diff. No hook errors.

### Test 2 — Refactor
**Task**: Extract duplicated boilerplate in `mcp-server/hooks.rs`.
**Result**: 4 identical validate+deserialize blocks → 1 generic
`extract_hook_data<T>` helper.  Net -25 lines. Behavior preserved.  Build,
clippy, 489 tests green.
**Observations**: `/oco-safe-refactor` skill invoked, structured workflow
followed. No hook errors. Impact analysis done before changes.

### Test 3 — Feature
**Task**: Add `Display` impls for `PlanStep`, `ExecutionPlan`, `StepExecution`,
`StepStatus`.
**Result**: 4 `Display` impls + 7 tests added in `shared-types/plan.rs`.
Compact, human-readable format for logs. Build, clippy, 496 tests green.
**Observations**: Self-contained change. No cross-crate impact. No hook errors.

### Test 4 — Multi-file cross-crate consistency
**Task**: Align `tokens_used` type to `u64` across all event/result types to
match `Budget` contract.
**Result**: 7 files modified across 4 crates + 1 app.  Two intermediate
compilation errors resolved by natural type propagation.  Deliberate boundary
preserved: `u32` for unit estimates, `u64` for accumulated totals.  Build,
clippy, 496 tests green.
**Observations**: Non-linear task. Cross-crate type propagation. Compilation
errors as expected and resolved without backtracking. No hook errors.

## Decision

The hooks-first runtime is validated as **production-viable on the tested MVP
scope**: a single-user, single-session, Rust monorepo environment executing
four distinct task types.  This is not a universal validation — it covers the
specific workflows tested and nothing beyond.

### What is validated

1. **No friction on standard tasks** — bugfix, refactor, feature, multi-file
   consistency changes all complete without hook-induced errors or blocking.
2. **Skills structure work without imposing it** — `/oco-safe-refactor` provides
   a staged workflow; Claude follows it naturally when invoked but isn't forced
   into it otherwise.
3. **Hooks are invisible when correct** — zero hook errors across all 4 tests.
   The runtime doesn't announce itself; it stays out of the way.
4. **Verification is reliable** — build, clippy (`--tests -D warnings`), and
   full test suite run consistently at each task boundary.
5. **Cross-crate changes work** — type propagation across 4 crates + 1 app
   completed with natural error-driven iteration.

### What is NOT validated

Each item below carries a specific risk if deployed without further testing.

1. **Long sessions (>50% context)** — untested.  Risk: hooks or skills may
   behave differently after `/compact` strips context.  `PostCompact`
   re-injection is a stub (TODO #45), so critical state could silently
   disappear.
2. **Concurrent agent execution** — untested.  Risk: Agent Teams and parallel
   subagents may produce race conditions in session state or hook ordering.
3. **Hook failure recovery** — untested.  Risk: a hook timeout or 500 could
   silently drop events or, worse, block Claude Code if the hook is
   configured as blocking.
4. **Rate limiting / abuse** — untested.  Risk: hook endpoints accept unlimited
   requests (TODO in `hooks.rs:80`); a runaway loop or external caller could
   exhaust server resources.
5. **PostCompact context re-injection** — stub only (TODO #45).  Risk: after
   compaction, working memory and plan state are lost from Claude's context
   with no recovery mechanism.
6. **Incremental re-indexing** — stub only (TODO #45).  Risk: symbol index goes
   stale after file edits, degrading search and code-intel accuracy over long
   sessions.
7. **Non-Rust projects** — untested.  Risk: hook payloads, skill workflows, and
   verification commands are only proven on a Cargo workspace.  Node, Python,
   or polyglot repos may hit unexpected code paths.
8. **Adversarial inputs** — untested.  Risk: malformed hook payloads, oversized
   bodies, or auth bypass attempts could crash handlers or leak state.  Auth
   middleware exists but has no integration tests.

### Observability metrics for continued validation

Track across the next 10-20 real sessions:

| Metric | Source | Target |
|--------|--------|--------|
| Hook error rate | Hook handler logs (`warn!` level) | 0% |
| Hook latency p99 | Server-side timing | < 50ms |
| Skill invocation success rate | Skill execution logs | > 95% |
| False-positive blocks | Pre-tool-use hook denials | 0 on legitimate ops |
| Context survival after compact | PostCompact hook | N/A (not yet wired) |
| Test suite stability | CI / manual runs | 0 regressions |

## Consequences

### Operational

- The runtime can be used in daily development on this repo within the
  tested scope (single-user, single-session, Rust monorepo).
- Hook error handling and timeout behavior should be hardened before
  multi-user or CI deployment.
- The CRLF warnings on `.claude/hooks/*` should be fixed via
  `.gitattributes` to prevent cross-platform diff pollution.
- `ContextAssembled::total_tokens` remains `u32` intentionally — it measures
  per-assembly budget, not accumulated totals.  This boundary is documented
  in the test 4 rationale and should be preserved.

### Architectural doctrine

These principles emerged from the validation and from earlier debugging of
configuration issues.  They are binding for this project unless explicitly
superseded by a later ADR.

1. **Hooks = enforcement layer.**  Hooks validate, record, and gate.  They
   do not generate content, make decisions, or call LLMs.  A hook that does
   more than enforce a contract is misplaced logic.

2. **MCP = accelerators only.**  MCP tools (search, trace, verify, findings)
   are optional performance boosters.  Every workflow must degrade gracefully
   if the MCP server is unreachable.  No skill or hook may hard-depend on
   MCP availability.

3. **Project-level `.claude/` = single source of truth.**  All hooks, skills,
   and settings live in the project's `.claude/` directory.  The global
   `~/.claude/` provides user preferences (language, style) only — never
   hook definitions, never skill overrides.

4. **No global/project duplication.**  A hook or skill must exist in exactly
   one place.  If the same logic appears in both `~/.claude/` and the
   project `.claude/`, the project copy wins and the global copy must be
   removed.  Duplication between layers caused the most debugging time
   during initial setup and is now a hard rule.
