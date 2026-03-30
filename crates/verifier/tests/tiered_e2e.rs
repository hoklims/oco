//! E2E tests for tiered verification against real project fixtures.
//!
//! These tests create temporary projects (Rust, Node, Python) and run
//! actual build/test commands to verify tiered dispatch works end-to-end.

use oco_shared_types::{TierSelector, VerificationTier};
use oco_verifier::VerificationDispatcher;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Create a minimal Rust project that compiles and has a passing test.
fn create_rust_project() -> TempDir {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"[package]
name = "test-fixture"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        r#"pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        assert_eq!(add(2, 2), 4);
    }
}
"#,
    )
    .unwrap();

    dir
}

/// Create a Rust project with a compile error (for failure testing).
fn create_broken_rust_project() -> TempDir {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("Cargo.toml"),
        r#"[package]
name = "broken-fixture"
version = "0.1.0"
edition = "2021"
"#,
    )
    .unwrap();

    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        r#"pub fn add(a: i32, b: i32) -> i32 {
    a + b + undefined_variable  // compile error
}
"#,
    )
    .unwrap();

    dir
}

/// Create a minimal Node.js project with a passing test script.
fn create_node_project() -> TempDir {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("package.json"),
        r#"{
  "name": "test-fixture",
  "version": "1.0.0",
  "scripts": {
    "build": "node -e \"console.log('build ok')\"",
    "test": "node test.js"
  }
}
"#,
    )
    .unwrap();

    fs::write(
        dir.path().join("test.js"),
        r#"const assert = require('assert');
assert.strictEqual(2 + 2, 4, 'math works');
console.log('1 test passed');
"#,
    )
    .unwrap();

    dir
}

/// Create a minimal Node.js project with a failing test.
fn create_broken_node_project() -> TempDir {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("package.json"),
        r#"{
  "name": "broken-fixture",
  "version": "1.0.0",
  "scripts": {
    "build": "node -e \"console.log('build ok')\"",
    "test": "node test.js"
  }
}
"#,
    )
    .unwrap();

    fs::write(
        dir.path().join("test.js"),
        r#"const assert = require('assert');
assert.strictEqual(2 + 2, 5, 'math is broken');
"#,
    )
    .unwrap();

    dir
}

/// Create a minimal Python project with pytest.
fn create_python_project() -> TempDir {
    let dir = TempDir::new().expect("create temp dir");

    fs::write(
        dir.path().join("pyproject.toml"),
        r#"[project]
name = "test-fixture"
version = "0.1.0"
requires-python = ">=3.8"
"#,
    )
    .unwrap();

    fs::write(
        dir.path().join("test_math.py"),
        r#"def test_add():
    assert 2 + 2 == 4

def test_multiply():
    assert 3 * 3 == 9
"#,
    )
    .unwrap();

    dir
}

/// Check if a command is available on PATH.
fn command_exists(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").output().is_ok()
}

// ---------------------------------------------------------------------------
// TierSelector integration (deterministic, no I/O)
// ---------------------------------------------------------------------------

#[test]
fn tier_selector_real_changeset_standard() {
    let files = vec!["src/main.rs".to_string(), "src/utils/helper.rs".to_string()];
    let tier = TierSelector::select(&files);
    assert_eq!(tier, VerificationTier::Standard);
    assert_eq!(tier.strategies().len(), 2); // Build + RunTests
}

#[test]
fn tier_selector_real_changeset_thorough() {
    let files = vec![
        "src/main.rs".to_string(),
        "src/auth/jwt_handler.rs".to_string(), // security pattern
        "Cargo.toml".to_string(),              // architecture pattern
    ];
    let tier = TierSelector::select(&files);
    assert_eq!(tier, VerificationTier::Thorough);
    assert_eq!(tier.strategies().len(), 4); // Build + TypeCheck + Lint + RunTests
}

// ---------------------------------------------------------------------------
// dispatch_tiered E2E with real cargo build + test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tiered_standard_real_rust_project_passes() {
    let project = create_rust_project();
    let dispatcher = VerificationDispatcher::new(120);

    let result = dispatcher
        .dispatch_tiered(VerificationTier::Standard, project.path().to_str().unwrap())
        .await
        .expect("dispatch_tiered should not error");

    // Standard tier = Build + RunTests
    assert_eq!(result.tier, VerificationTier::Standard);
    assert!(
        result.all_passed(),
        "all strategies should pass on a valid project, failures: {:?}",
        result.all_failures()
    );
    assert_eq!(
        result.results.len(),
        2,
        "Standard tier should run 2 strategies (Build + RunTests)"
    );

    // Verify each result has real output
    for (strategy, output) in &result.results {
        assert!(
            output.passed,
            "{strategy:?} should pass, exit_code={}",
            output.exit_code
        );
        assert_eq!(output.exit_code, 0);
        assert!(output.duration_ms > 0, "{strategy:?} should take >0ms");
    }
}

#[tokio::test]
async fn tiered_light_real_rust_project_passes() {
    let project = create_rust_project();
    let dispatcher = VerificationDispatcher::new(120);

    let result = dispatcher
        .dispatch_tiered(VerificationTier::Light, project.path().to_str().unwrap())
        .await
        .expect("dispatch_tiered should not error");

    assert_eq!(result.tier, VerificationTier::Light);
    assert!(result.all_passed());
    assert_eq!(result.results.len(), 1, "Light tier = Build only");
}

#[tokio::test]
async fn tiered_stops_on_first_failure() {
    let project = create_broken_rust_project();
    let dispatcher = VerificationDispatcher::new(120);

    let result = dispatcher
        .dispatch_tiered(VerificationTier::Standard, project.path().to_str().unwrap())
        .await
        .expect("dispatch_tiered should not error (returns results, not Err)");

    assert!(!result.all_passed(), "broken project should fail");
    // Should stop after Build (first strategy) since it fails.
    assert_eq!(
        result.results.len(),
        1,
        "should stop after first failure (Build), not continue to RunTests"
    );

    let failures = result.all_failures();
    assert!(!failures.is_empty(), "should report at least one failure");
}

#[tokio::test]
async fn tiered_thorough_real_rust_project_passes() {
    let project = create_rust_project();
    let dispatcher = VerificationDispatcher::new(120);

    let result = dispatcher
        .dispatch_tiered(VerificationTier::Thorough, project.path().to_str().unwrap())
        .await
        .expect("dispatch_tiered should not error");

    assert_eq!(result.tier, VerificationTier::Thorough);
    // Thorough = Build + TypeCheck + Lint + RunTests
    // Note: Lint (clippy) may not be installed, so we check that at least
    // Build passed and the tier was correctly dispatched.
    assert!(!result.results.is_empty(), "should run at least Build");

    // First strategy (Build) should always pass
    let (first_strategy, first_output) = &result.results[0];
    assert!(
        first_output.passed,
        "{first_strategy:?} should pass on valid project"
    );
}

// ---------------------------------------------------------------------------
// Node.js E2E
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tiered_standard_node_project_passes() {
    if !command_exists("node") || !command_exists("npm") {
        eprintln!("SKIP: node/npm not found");
        return;
    }

    let project = create_node_project();
    let dispatcher = VerificationDispatcher::new(60);

    let result = dispatcher
        .dispatch_tiered(VerificationTier::Standard, project.path().to_str().unwrap())
        .await
        .expect("dispatch_tiered should not error");

    assert_eq!(result.tier, VerificationTier::Standard);
    assert!(
        result.all_passed(),
        "Node project should pass Standard tier, failures: {:?}",
        result.all_failures()
    );
    assert_eq!(result.results.len(), 2, "Standard = Build + RunTests");
}

#[tokio::test]
async fn tiered_standard_broken_node_project_fails() {
    if !command_exists("node") || !command_exists("npm") {
        eprintln!("SKIP: node/npm not found");
        return;
    }

    let project = create_broken_node_project();
    let dispatcher = VerificationDispatcher::new(60);

    let result = dispatcher
        .dispatch_tiered(VerificationTier::Standard, project.path().to_str().unwrap())
        .await
        .expect("dispatch_tiered should not error");

    // Build passes (it's just `node -e "..."`), but test fails (assertion error).
    assert!(!result.all_passed(), "broken Node project should fail");

    let failures = result.all_failures();
    assert!(!failures.is_empty(), "should report test failures");
}

// ---------------------------------------------------------------------------
// Python E2E
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tiered_standard_python_project_passes() {
    if !command_exists("pytest") {
        eprintln!("SKIP: pytest not found");
        return;
    }

    let project = create_python_project();
    let dispatcher = VerificationDispatcher::new(60);

    // Python build (`python -m build`) requires the `build` package which
    // may not be installed. Use Light tier (build only) to test detection,
    // then Standard to test pytest.
    // Actually, for Python, Build will likely fail without `build` package,
    // so test RunTests directly via single dispatch to prove pytest works.
    let result = dispatcher
        .dispatch(
            oco_shared_types::VerificationStrategy::RunTests,
            None,
            project.path().to_str().unwrap(),
        )
        .await
        .expect("pytest dispatch should not error");

    assert!(
        result.passed,
        "pytest should pass on valid Python project, stderr: {}",
        result.stderr
    );
    assert_eq!(result.exit_code, 0);
    assert!(result.duration_ms > 0);
}

#[tokio::test]
async fn tier_selector_routes_python_auth_to_thorough() {
    // Prove that a Python changeset with security files gets Thorough tier.
    let files = vec![
        "app/views.py".to_string(),
        "app/auth/oauth_handler.py".to_string(),
        "pyproject.toml".to_string(),
    ];
    let tier = TierSelector::select(&files);
    assert_eq!(tier, VerificationTier::Thorough);
}

#[tokio::test]
async fn tier_selector_routes_node_src_to_standard() {
    let files = vec![
        "src/index.ts".to_string(),
        "src/utils/format.ts".to_string(),
    ];
    let tier = TierSelector::select(&files);
    assert_eq!(tier, VerificationTier::Standard);
}
