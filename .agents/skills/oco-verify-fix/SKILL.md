---
name: oco-verify-fix
description: >
  Mandatory verification suite after any code change.
  Auto-activates after every source file modification, even trivial ones (one-liner).
  Detects project type (Cargo.toml, package.json, pyproject.toml, go.mod, .csproj) and runs
  in order: build → types → lint → tests. Produces a PASS/FAIL/PARTIAL verdict.
  NON-NEGOTIABLE: never consider a change complete without running this skill.
  Also activates when the user asks to verify, test, validate, or check their changes.
triggers:
  - "verify"
  - "check my changes"
  - "run tests"
  - "does it build"
  - "make sure it works"
  - "validate"
---

# OCO: Verify Fix

You are verifying that code changes are correct and complete. Follow this structured verification workflow.

## Step 1: Identify What Changed

List all modified files:
```bash
git diff --name-only HEAD 2>/dev/null || git status --short
```

## Step 2: Detect Project Type and Available Checks

Detect the verification suite from project manifests:

| Signal | Build | Types | Lint | Test |
|--------|-------|-------|------|------|
| `Cargo.toml` | `cargo build` | `cargo check` | `cargo clippy` | `cargo test` |
| `package.json` | `npm run build` | `tsc --noEmit` | `npm run lint` | `npm test` |
| `pyproject.toml` | - | `mypy .` | `ruff check .` | `pytest` |
| `go.mod` | `go build ./...` | `go vet ./...` | `golangci-lint run` | `go test ./...` |

Use `oco.verify_patch` if available (auto-detects project type and runs all checks):
```
oco.verify_patch({ workspace: "." })
```

Otherwise, read the project manifest and run checks manually. Prefer `scripts` from package.json when available.

## Step 3: Run Verification Sequence

Execute in order (stop on first failure):

1. **Build** — Does it compile?
2. **Type check** — Are types consistent?
3. **Lint** — Are there style/quality issues?
4. **Test** — Do tests pass? Are new tests needed?

For each step, report:
- Status: pass / fail / skip (not available)
- Output summary (compact, not raw dump)

## Step 4: Assess Results

Produce a verification verdict:

```
VERDICT: PASS | FAIL | PARTIAL
- Build: [pass/fail/skip]
- Types: [pass/fail/skip]
- Lint:  [pass/fail/skip]
- Tests: [pass/fail/skip]
- Missing coverage: [description if applicable]
```

## Step 5: Handle Failures

If any check fails:
1. Identify the specific failure
2. Fix it
3. Re-run the failing check
4. Continue the sequence

## Step 6: Verification Complete

After all checks pass, the PostToolUse hook automatically detects verification commands (`cargo test`, `npm test`, etc.) and marks the session as verified. The Stop hook will then allow completion without warning.

No manual marker is needed — the hook system handles this automatically.

## Rules

- Never skip a check that's available in the project
- Never report PASS if any check failed
- If tests are missing for the changed code, flag it explicitly
- Keep output summaries compact — report failures in detail, successes briefly
