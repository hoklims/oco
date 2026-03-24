# ADR-004: Verification Integrity Model

## Status
Accepted

## Context
In v1, the orchestrator tracked verification as a simple boolean (`has_verified`). This created several problems:

- **Stale verification**: A session could mark itself as "verified" early, then make further code modifications without re-verifying. The final state would still show `has_verified = true`, even though the verification predated the latest changes.
- **No per-file tracking**: There was no record of which files had been modified since the last verification run, making it impossible to determine whether verification results still applied.
- **Write tasks completing unverified**: Tasks involving code generation or modification could complete their lifecycle without the policy engine forcing a re-verification pass after the last edit.

This undermined the core guarantee that OCO's outputs are verified before being presented to the user.

## Decision
Replace the boolean `has_verified` with a structured verification integrity model:

### VerificationState
A per-session state object that tracks:
- **modified_files**: Set of file paths modified since the last successful verification run.
- **last_verification**: Optional `VerificationRun` snapshot.
- **freshness**: Computed enum — `Fresh`, `Partial`, `Stale`, or `None`.

### VerificationRun
A snapshot recorded after each verification pass:
- **timestamp**: When the verification completed.
- **modification_snapshot**: Set of files that were known-modified at the time of the run.
- **results**: Per-runner outcomes (test, build, lint, typecheck).

### Freshness Computation
- **Fresh**: `modified_files` is empty or equals the snapshot from the last run (no new modifications since verification).
- **Partial**: Some files modified since the last run, but the modified set is a subset of what was verified.
- **Stale**: Files modified since the last run that were not part of the verification snapshot.
- **None**: No verification has ever been run in this session.

### Policy Integration
The policy engine inspects `VerificationState.freshness` before allowing a session to complete:
- Write tasks with `Stale` or `None` freshness trigger `pending_verification = true`, which forces a verification action before the session can close.
- Read-only tasks are exempt from verification requirements.
- Tool executors report modified files back into `VerificationState.modified_files` after each tool call.

## Consequences

### Positive
- **Accurate enforcement**: Verification is no longer a one-shot flag — it reflects the actual state of the codebase relative to the last check.
- **Granular staleness**: The system distinguishes between "never verified", "partially stale", and "fully stale", enabling proportional responses (e.g., re-run only affected runners).
- **Auditability**: Each `VerificationRun` is a snapshot that can be included in decision traces for debugging and evaluation.

### Negative
- **Memory overhead**: Tracking per-file modification sets adds a small memory cost per session. Bounded by the number of files touched in a single session (typically < 100).
- **Tool cooperation required**: Tool executors must report which files they modify. If a tool fails to report, freshness computation will be optimistic (incorrectly Fresh). This is mitigated by normalizer-level file tracking in the tool runtime.
