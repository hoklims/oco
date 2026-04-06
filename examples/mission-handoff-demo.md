# Mission Memory Handoff Demo

This walkthrough demonstrates the `MissionMemory` lifecycle using a realistic auth bug investigation across two sessions.

## Scenario: Broken JWT refresh in login flow

A user reports that logged-in users are intermittently logged out after ~15 minutes despite having a valid refresh token. The auth stack uses JWT access tokens (15-min expiry) with HTTP-only cookie-based refresh tokens.

---

## Session 1: Investigation and partial fix

### 1. Start the investigation

```bash
oco run "fix auth bug: users get logged out after 15 minutes despite valid refresh token" \
  --workspace ./example-repo
```

OCO classifies this as a Medium task, generates an execution plan, and begins investigating.

### 2. OCO investigates

The orchestrator executes its plan:

**Step 1 -- Explore auth middleware:**
- Reads `src/auth/middleware.rs`, discovers JWT validation logic.
- Reads `src/auth/refresh.rs`, discovers the refresh endpoint.
- Establishes fact: _"Access tokens expire after 900s (15 min), configured in `src/auth/config.rs:12`."_

**Step 2 -- Trace the refresh flow:**
- Reads `src/auth/refresh.rs`, finds the `/api/auth/refresh` handler.
- Reads `src/auth/cookies.rs`, discovers cookie configuration.
- Establishes fact: _"Refresh token is set as HTTP-only cookie with `Path=/api/auth`."_
- Forms hypothesis: _"The refresh cookie Path is too narrow -- browser does not send it on `/api/auth/refresh` because the endpoint is at `/api/auth/refresh` but the cookie path is `/api/auth` (trailing slash mismatch)."_ Confidence: 55%.

**Step 3 -- Check cookie path behavior:**
- Reads browser cookie spec notes in `docs/auth-design.md`.
- Establishes fact: _"RFC 6265 path matching: cookie with `Path=/api/auth` IS sent to `/api/auth/refresh` (prefix match). Path is not the issue."_
- Hypothesis confidence drops to 15%. Hypothesis effectively invalidated.

**Step 4 -- Investigate token rotation:**
- Reads `src/auth/refresh.rs:47-82`, finds `rotate_refresh_token()`.
- Discovers that on refresh, a new refresh token is issued but the old one is not invalidated in the database until the new one is used.
- Establishes fact: _"Token rotation uses a grace period: old token valid for 30s after rotation."_
- Forms hypothesis: _"Race condition in concurrent refresh requests -- two tabs hit `/api/auth/refresh` simultaneously, both use the same old token, the second request fails because the first already rotated."_ Confidence: 80%.

**Step 5 -- Implement fix:**
- Modifies `src/auth/refresh.rs` to add a mutex on per-user refresh operations.
- Modifies `src/auth/token_store.rs` to extend the grace period window and deduplicate concurrent rotations.

**Step 6 -- Run verification:**
- Build passes.
- Existing tests pass.
- No integration test covers concurrent refresh (identified as a gap).

### 3. Session ends

The user's budget runs out (50k tokens consumed). OCO persists the mission memory.

### 4. MissionMemory is saved

OCO writes the artifact to:

```
.oco/runs/a1b2c3d4-e5f6-7890-abcd-ef1234567890/mission.json
```

---

## Viewing the mission

The user can inspect what OCO knew at session end:

```bash
oco runs show a1b2c3d4 --mission
```

This renders `to_handoff_text()` output:

```
OCO Mission Handoff
====================
Mission: fix auth bug: users get logged out after 15 minutes despite valid refresh token
Session: a1b2c3d4-e5f6-7890-abcd-ef1234567890
Captured: 2026-04-04 14:32:07 UTC
Trust: medium

VERIFIED FACTS (3):
  - Access tokens expire after 900s (15 min), configured in src/auth/config.rs:12 (source: src/auth/config.rs)
  - Refresh token is set as HTTP-only cookie with Path=/api/auth (source: src/auth/cookies.rs)
  - Token rotation uses a grace period: old token valid for 30s after rotation (source: src/auth/refresh.rs)

ACTIVE HYPOTHESES (1):
  - Race condition in concurrent refresh requests: two tabs hit /api/auth/refresh simultaneously, second request fails because first already rotated the token (confidence: 80%)

OPEN QUESTIONS (2):
  ? Does the token store handle database connection timeouts during rotation?
  ? Are there other callers of rotate_refresh_token() besides the refresh endpoint?

PLAN:
  Current objective: Add integration test for concurrent refresh
  Phase: verify
  [done] 1. Investigate auth middleware
  [done] 2. Trace refresh flow
  [done] 3. Check cookie path behavior
  [done] 4. Investigate token rotation
  [done] 5. Implement fix for concurrent refresh race
  [todo] 1. Add integration test for concurrent refresh
  [todo] 2. Run full verification suite

VERIFICATION:
  Freshness: Stale
  Last check: 14:28:43 UTC
  Passed: build, test
  Failed:
  ! src/auth/refresh.rs
  ! src/auth/token_store.rs

MODIFIED FILES (2):
  - src/auth/refresh.rs
  - src/auth/token_store.rs

KEY DECISIONS:
  - Chose per-user mutex over global lock to avoid blocking unrelated users
  - Extended grace period from 30s to 60s rather than eliminating it entirely

RISKS:
  ! No integration test covers concurrent refresh -- fix is unverified under real concurrency
  ! Grace period extension may mask other rotation bugs
```

---

## Session 2: Resume and complete

### 6. Resume from previous session

```bash
oco run "continue fixing auth bug" \
  --workspace ./example-repo \
  --resume a1b2c3d4
```

OCO loads the mission memory:

1. **`load_from()`** reads `.oco/runs/a1b2c3d4-.../mission.json`.
2. **Schema check** confirms `schema_version: 1` is compatible.
3. **`merge_from_previous()`** carries forward all 3 verified facts, the active hypothesis, both open questions, and the key decisions into the new session's working memory.
4. The orchestrator sees the plan state: phase is `verify`, current objective is _"Add integration test for concurrent refresh"_, and two steps remain.

### 7. OCO completes the task

The resumed session picks up where session 1 left off:

**Step 1 -- Write integration test:**
- Creates `tests/auth/test_concurrent_refresh.rs` with a test that spawns 5 concurrent refresh requests using the same refresh token.
- Verifies that exactly one rotation succeeds and the others receive a valid (non-error) response via the grace period.

**Step 2 -- Run full verification:**
- Build: pass.
- Tests: pass (including the new concurrent refresh test).
- Clippy: pass.
- Verification freshness moves to `Fresh`.

**Step 3 -- Answer open questions:**
- Investigates `rotate_refresh_token()` callers: confirms only the refresh endpoint calls it.
- Checks database timeout handling: identifies a missing timeout on the token store query but flags it as a separate issue, not related to the current bug.

Session completes with trust verdict `High`.

### 8. Final mission memory

A new mission memory is persisted for the second session:

```
.oco/runs/b2c3d4e5-f6a7-8901-bcde-f12345678901/mission.json
```

This second mission memory includes:
- All facts from session 1 (carried forward via merge) plus new facts from session 2.
- The hypothesis is now confirmed (promoted to fact) or removed.
- Open questions are resolved.
- Plan shows all steps completed.
- Verification freshness is `Fresh`, trust verdict is `High`.

---

## On-disk artifact

The persisted `mission.json` is standard JSON, readable by any tool:

```json
{
  "schema_version": 1,
  "session_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "created_at": "2026-04-04T14:32:07Z",
  "mission": "fix auth bug: users get logged out after 15 minutes despite valid refresh token",
  "facts": [
    {
      "content": "Access tokens expire after 900s (15 min), configured in src/auth/config.rs:12",
      "source": "src/auth/config.rs",
      "established_at": "2026-04-04T14:18:22Z"
    },
    {
      "content": "Refresh token is set as HTTP-only cookie with Path=/api/auth",
      "source": "src/auth/cookies.rs",
      "established_at": "2026-04-04T14:20:01Z"
    },
    {
      "content": "Token rotation uses a grace period: old token valid for 30s after rotation",
      "source": "src/auth/refresh.rs",
      "established_at": "2026-04-04T14:24:15Z"
    }
  ],
  "hypotheses": [
    {
      "content": "Race condition in concurrent refresh requests: two tabs hit /api/auth/refresh simultaneously, second request fails because first already rotated the token",
      "confidence_pct": 80,
      "supporting_evidence": [
        "rotate_refresh_token() invalidates old token after issuing new one",
        "No synchronization on per-user refresh operations",
        "15-min expiry matches reported logout interval"
      ]
    }
  ],
  "open_questions": [
    "Does the token store handle database connection timeouts during rotation?",
    "Are there other callers of rotate_refresh_token() besides the refresh endpoint?"
  ],
  "plan": {
    "current_objective": "Add integration test for concurrent refresh",
    "completed_steps": [
      "Investigate auth middleware",
      "Trace refresh flow",
      "Check cookie path behavior",
      "Investigate token rotation",
      "Implement fix for concurrent refresh race"
    ],
    "remaining_steps": [
      "Add integration test for concurrent refresh",
      "Run full verification suite"
    ],
    "phase": "verify"
  },
  "verification": {
    "freshness": "Stale",
    "unverified_files": [
      "src/auth/refresh.rs",
      "src/auth/token_store.rs"
    ],
    "last_check": "2026-04-04T14:28:43Z",
    "checks_passed": ["build", "test"],
    "checks_failed": []
  },
  "modified_files": [
    "src/auth/refresh.rs",
    "src/auth/token_store.rs"
  ],
  "key_decisions": [
    "Chose per-user mutex over global lock to avoid blocking unrelated users",
    "Extended grace period from 30s to 60s rather than eliminating it entirely"
  ],
  "risks": [
    "No integration test covers concurrent refresh -- fix is unverified under real concurrency",
    "Grace period extension may mask other rotation bugs"
  ],
  "trust_verdict": "Medium",
  "narrative": ""
}
```

---

## Key takeaways

| Aspect | Behavior |
|--------|----------|
| **Persistence** | Automatic at session end to `.oco/runs/<id>/mission.json` |
| **Viewing** | `oco runs show <id> --mission` renders `to_handoff_text()` |
| **Resuming** | `oco run "..." --resume <id>` loads and merges previous mission |
| **Deduplication** | Facts, hypotheses, questions, and decisions are deduplicated by content on merge |
| **Schema safety** | Future schema versions are rejected; older versions are accepted with defaults |
| **No invention** | `from_working_state()` only captures data already present in `WorkingMemory` and `VerificationState` |
