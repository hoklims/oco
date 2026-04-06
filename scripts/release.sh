#!/usr/bin/env bash
set -euo pipefail

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# OCO Release Script
#
# Usage:
#   bash scripts/release.sh <new-version>
#   bash scripts/release.sh 0.14.0
#   bash scripts/release.sh 0.14.0 --dry-run    # preview without pushing
#
# What it does (in order):
#   1. Pre-flight checks (clean tree, on main, synced with origin)
#   2. Run full CI locally (fmt, clippy, tests, dashboard build)
#   3. Bump version everywhere (Cargo.toml, package.json, CLAUDE.md)
#   4. Commit + push on a release branch
#   5. Create PR → wait for CI → merge
#   6. Tag + create GitHub release
#   7. Build + install oco binary (handles locked binary)
#   8. Final verification
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ── Args ──────────────────────────────────────────────────────────────
NEW_VERSION="${1:-}"
DRY_RUN=false
[[ "${2:-}" == "--dry-run" ]] && DRY_RUN=true

if [[ -z "$NEW_VERSION" ]]; then
  CURRENT=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
  echo "Usage: bash scripts/release.sh <new-version>"
  echo ""
  echo "Current version: $CURRENT"
  echo "Example:         bash scripts/release.sh 0.14.0"
  echo "                 bash scripts/release.sh 0.14.0 --dry-run"
  exit 1
fi

CURRENT_VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')

if [[ "$NEW_VERSION" == "$CURRENT_VERSION" ]]; then
  echo "ERROR: New version ($NEW_VERSION) is the same as current ($CURRENT_VERSION)"
  exit 1
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  OCO Release: $CURRENT_VERSION → $NEW_VERSION"
[[ "$DRY_RUN" == true ]] && echo "  (DRY RUN — nothing will be pushed)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# ── 1. Pre-flight checks ─────────────────────────────────────────────
echo "▸ Pre-flight checks..."

# Must be on main
BRANCH=$(git rev-parse --abbrev-ref HEAD)
if [[ "$BRANCH" != "main" ]]; then
  echo "  ERROR: Must be on 'main' branch (currently on '$BRANCH')"
  exit 1
fi
echo "  ✓ On main branch"

# Must be clean (allow untracked files like screenshots)
if ! git diff --quiet HEAD -- '*.rs' '*.toml' '*.json' '*.ts' '*.svelte' '*.md' 2>/dev/null; then
  echo "  ERROR: Working tree has uncommitted changes to tracked files"
  echo "  Run: git stash or git commit"
  exit 1
fi
echo "  ✓ Working tree clean (tracked files)"

# Sync with origin
git fetch origin main --quiet
LOCAL=$(git rev-parse HEAD)
REMOTE=$(git rev-parse origin/main)
if [[ "$LOCAL" != "$REMOTE" ]]; then
  echo "  ERROR: Local main ($LOCAL) != origin/main ($REMOTE)"
  echo "  Run: git pull"
  exit 1
fi
echo "  ✓ In sync with origin/main"

# gh CLI available
if ! command -v gh &>/dev/null; then
  echo "  ERROR: 'gh' CLI not found. Install: https://cli.github.com"
  exit 1
fi
echo "  ✓ gh CLI available"

echo ""

# ── 2. Run CI locally ────────────────────────────────────────────────
echo "▸ Running local CI..."

echo "  → cargo fmt --check"
if ! cargo fmt --check 2>/dev/null; then
  echo "  Formatting... "
  cargo fmt
  echo "  ⚠ Had to auto-format. Will include in version bump commit."
fi
echo "  ✓ Formatted"

echo "  → cargo clippy --tests"
cargo clippy --tests -- -D warnings 2>&1 | tail -1
echo "  ✓ Clippy clean"

echo "  → cargo test"
TEST_OUTPUT=$(cargo test 2>&1)
TOTAL_PASS=$(echo "$TEST_OUTPUT" | grep "test result: ok" | grep -oE '[0-9]+ passed' | awk '{s+=$1} END {print s}')
echo "  ✓ $TOTAL_PASS tests passed"

# Dashboard build
if [[ -d "apps/dashboard" ]]; then
  echo "  → dashboard build"
  (cd apps/dashboard && npx vite build 2>&1 | tail -1)
  echo "  ✓ Dashboard builds"
fi

echo ""

# ── 3. Bump versions ─────────────────────────────────────────────────
echo "▸ Bumping versions to $NEW_VERSION..."

# Cargo.toml workspace version
sed -i "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml
echo "  ✓ Cargo.toml → $NEW_VERSION"

# package.json (npm plugin)
if [[ -f package.json ]]; then
  sed -i "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" package.json
  echo "  ✓ package.json → $NEW_VERSION"
fi

# VS Code extension (may be out of sync — force to new version)
if [[ -f apps/vscode-extension/package.json ]]; then
  sed -i "s/\"version\": \"[0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\"/\"version\": \"$NEW_VERSION\"/" apps/vscode-extension/package.json
  echo "  ✓ vscode-extension/package.json → $NEW_VERSION"
fi

# CLAUDE.md test counts
if [[ -n "$TOTAL_PASS" ]] && [[ "$TOTAL_PASS" -gt 0 ]]; then
  # Update the "All tests (N+)" line
  sed -i "s/# All tests ([0-9]\+/# All tests ($TOTAL_PASS/" CLAUDE.md 2>/dev/null || true
  echo "  ✓ CLAUDE.md test count → $TOTAL_PASS"
fi

# Verify all versions match
echo "  → Verifying version sync..."
bash scripts/ci/check-versions.sh 2>&1 | grep -E "OK|MISMATCH|FAIL" | sed 's/^/  /'

echo ""

# ── 4. Commit + push branch ──────────────────────────────────────────
RELEASE_BRANCH="chore/release-$NEW_VERSION"

echo "▸ Creating release commit..."
git checkout -b "$RELEASE_BRANCH" 2>/dev/null

# Stage all version-bumped files
git add Cargo.toml package.json CLAUDE.md
git add apps/vscode-extension/package.json 2>/dev/null || true
# Include any fmt fixes
git add -u -- '*.rs' 2>/dev/null || true

git commit -m "chore: release v$NEW_VERSION

Bump version $CURRENT_VERSION → $NEW_VERSION ($TOTAL_PASS tests passing)."

if [[ "$DRY_RUN" == true ]]; then
  echo "  DRY RUN: would push $RELEASE_BRANCH and create PR"
  git checkout main
  git branch -D "$RELEASE_BRANCH"
  echo ""
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "  DRY RUN complete. No changes pushed."
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  exit 0
fi

git push -u origin "$RELEASE_BRANCH"
echo "  ✓ Pushed $RELEASE_BRANCH"

echo ""

# ── 5. PR → CI → merge ───────────────────────────────────────────────
echo "▸ Creating PR..."
PR_URL=$(gh pr create \
  --title "chore: release v$NEW_VERSION" \
  --body "Bump version \`$CURRENT_VERSION\` → \`$NEW_VERSION\` ($TOTAL_PASS tests passing).")
PR_NUM=$(echo "$PR_URL" | grep -oE '[0-9]+$')
echo "  ✓ PR #$PR_NUM created: $PR_URL"

echo "  → Waiting for CI..."
if gh pr checks "$PR_NUM" --watch 2>&1 | tail -3 | grep -q "fail"; then
  echo "  ERROR: CI failed on PR #$PR_NUM"
  echo "  Fix the issue, push to $RELEASE_BRANCH, and re-run CI."
  echo "  Then: gh pr merge $PR_NUM --squash --delete-branch"
  exit 1
fi
echo "  ✓ CI passed"

echo "  → Merging..."
gh pr merge "$PR_NUM" --squash --delete-branch
echo "  ✓ PR #$PR_NUM merged"

echo ""

# ── 6. Tag + GitHub release ──────────────────────────────────────────
echo "▸ Creating tag + release..."
git checkout main
git pull --quiet

TAG="v$NEW_VERSION"
git tag "$TAG"
git push origin "$TAG"
echo "  ✓ Tag $TAG pushed"

gh release create "$TAG" \
  --title "$TAG" \
  --generate-notes
RELEASE_URL="https://github.com/$(gh repo view --json nameWithOwner -q .nameWithOwner)/releases/tag/$TAG"
echo "  ✓ Release created: $RELEASE_URL"

echo ""

# ── 7. Build + install binary ────────────────────────────────────────
echo "▸ Building + installing oco binary..."

cargo build -p oco-dev-cli --release 2>&1 | tail -1

# Find all oco.exe locations and update them
BUILT="$ROOT/target/release/oco.exe"
if [[ ! -f "$BUILT" ]]; then
  # Linux/macOS
  BUILT="$ROOT/target/release/oco"
fi

if [[ -f "$BUILT" ]]; then
  # Kill any running oco process first
  if command -v taskkill &>/dev/null; then
    # Windows
    taskkill //IM oco.exe //F 2>/dev/null || true
    sleep 1
  else
    # Unix
    pkill -f "oco serve" 2>/dev/null || true
    sleep 1
  fi

  # Copy to all known locations
  for dest in \
    "$HOME/.cargo/bin/oco.exe" \
    "$HOME/.cargo/bin/oco" \
    "$HOME/bin/oco.exe" \
    "$HOME/bin/oco"; do
    if [[ -f "$dest" ]]; then
      cp "$BUILT" "$dest" 2>/dev/null && echo "  ✓ Updated $dest" || echo "  ⚠ Could not update $dest (locked?)"
    fi
  done

  # Verify
  INSTALLED_VER=$(oco --version 2>/dev/null | awk '{print $2}')
  if [[ "$INSTALLED_VER" == "$NEW_VERSION" ]]; then
    echo "  ✓ oco --version → $INSTALLED_VER"
  else
    echo "  ⚠ oco --version shows $INSTALLED_VER (expected $NEW_VERSION)"
    echo "    Binary may be cached. Try: cp $BUILT $(which oco)"
  fi
else
  echo "  ⚠ Binary not found at $BUILT"
fi

echo ""

# ── 8. Final verification ────────────────────────────────────────────
echo "▸ Final verification..."
echo "  Cargo.toml:  $(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')"
echo "  package.json: $(grep -m1 '"version"' package.json | sed 's/.*"\([0-9][^"]*\)".*/\1/')"
echo "  oco binary:  $(oco --version 2>/dev/null || echo 'not found')"
echo "  git tag:     $(git tag --sort=-version:refname | head -1)"
echo "  tests:       $TOTAL_PASS"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  ✓ Release v$NEW_VERSION complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
