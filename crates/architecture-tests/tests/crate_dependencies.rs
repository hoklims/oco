//! Architecture fitness tests — enforce the crate dependency DAG.
//!
//! These tests parse each crate's Cargo.toml and verify that internal
//! dependencies match the allowed dependency graph defined in CLAUDE.md.
//!
//! Inspired by arch-unit-ts (VALORA project) — adapted for Rust workspaces.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Returns the workspace root (two levels up from this crate's CARGO_MANIFEST_DIR).
fn workspace_root() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let crate_dir = PathBuf::from(manifest);

    let workspace_dir = crate_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or_else(|| {
            panic!(
                "Failed to compute workspace root from CARGO_MANIFEST_DIR: {}",
                crate_dir.display()
            )
        });

    std::fs::canonicalize(workspace_dir).unwrap_or_else(|e| {
        panic!(
            "Failed to canonicalize workspace root {}: {e}",
            workspace_dir.display()
        )
    })
}

/// Parse a Cargo.toml and extract internal (oco-*) dependency names.
fn extract_internal_deps(cargo_toml_path: &Path) -> Vec<String> {
    let content = std::fs::read_to_string(cargo_toml_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {e}", cargo_toml_path.display()));

    let parsed: toml::Value = content
        .parse()
        .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", cargo_toml_path.display()));

    let mut deps = Vec::new();

    for section in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = parsed.get(section).and_then(|v| v.as_table()) {
            for key in table.keys() {
                if key.starts_with("oco-") {
                    deps.push(key.clone());
                }
            }
        }
    }

    deps.sort();
    deps.dedup();
    deps
}

/// The authoritative dependency DAG for the OCO workspace.
///
/// Each entry maps a crate name to the set of internal crates it MAY depend on.
/// Any dependency not listed here is a violation.
fn allowed_dependency_graph() -> HashMap<&'static str, Vec<&'static str>> {
    let mut g = HashMap::new();

    // Layer 0 — Foundation (no internal deps)
    g.insert("oco-shared-types", vec![]);
    g.insert("oco-shared-proto", vec![]);

    // Layer 1 — Single-dependency crates (only shared-types)
    g.insert("oco-policy-engine", vec!["oco-shared-types"]);
    g.insert("oco-code-intel", vec!["oco-shared-types"]);
    g.insert("oco-retrieval", vec!["oco-shared-types"]);
    g.insert("oco-tool-runtime", vec!["oco-shared-types"]);
    g.insert("oco-verifier", vec!["oco-shared-types"]);
    g.insert("oco-telemetry", vec!["oco-shared-types"]);

    // Layer 2 — Mid-layer
    g.insert(
        "oco-context-engine",
        vec!["oco-shared-types", "oco-retrieval", "oco-code-intel"],
    );
    g.insert("oco-planner", vec!["oco-shared-types", "oco-policy-engine"]);

    // Layer 3 — Aggregator
    g.insert(
        "oco-orchestrator-core",
        vec![
            "oco-shared-types",
            "oco-policy-engine",
            "oco-context-engine",
            "oco-tool-runtime",
            "oco-retrieval",
            "oco-verifier",
            "oco-planner",
            "oco-telemetry",
            "oco-code-intel",
        ],
    );

    // Layer 4 — Application edge
    g.insert(
        "oco-mcp-server",
        vec![
            "oco-shared-types",
            "oco-orchestrator-core",
            "oco-retrieval",
            "oco-code-intel",
        ],
    );
    g.insert(
        "oco-dev-cli",
        vec![
            "oco-shared-types",
            "oco-orchestrator-core",
            "oco-telemetry",
            "oco-mcp-server",
            "oco-policy-engine",
            "oco-verifier",
        ],
    );

    // Meta — test-only crates (no internal deps)
    g.insert("oco-architecture-tests", vec![]);

    g
}

/// Discover all crate Cargo.toml files in the workspace.
fn discover_crates(root: &Path) -> Vec<(String, PathBuf)> {
    let mut crates = Vec::new();

    for dir in ["crates", "apps"] {
        let base = root.join(dir);
        if !base.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&base)
            .unwrap_or_else(|e| panic!("Failed to read directory {}: {e}", base.display()))
        {
            let entry =
                entry.unwrap_or_else(|e| panic!("Failed to read entry in {}: {e}", base.display()));
            let cargo_toml = entry.path().join("Cargo.toml");
            if cargo_toml.exists() {
                let content = std::fs::read_to_string(&cargo_toml)
                    .unwrap_or_else(|e| panic!("Failed to read {}: {e}", cargo_toml.display()));
                let parsed: toml::Value = content
                    .parse()
                    .unwrap_or_else(|e| panic!("Failed to parse {}: {e}", cargo_toml.display()));
                if let Some(name) = parsed
                    .get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                {
                    crates.push((name.to_string(), cargo_toml));
                }
            }
        }
    }

    crates.sort_by(|a, b| a.0.cmp(&b.0));
    crates
}

#[test]
fn crate_dependency_dag_is_enforced() {
    let root = workspace_root();
    let allowed = allowed_dependency_graph();
    let crates = discover_crates(&root);
    let mut violations = Vec::new();

    for (crate_name, cargo_toml_path) in &crates {
        let actual_deps = extract_internal_deps(cargo_toml_path);

        let Some(allowed_deps) = allowed.get(crate_name.as_str()) else {
            if !actual_deps.is_empty() {
                violations.push(format!(
                    "  {crate_name}: not in allowed graph but has internal deps: {actual_deps:?}"
                ));
            }
            continue;
        };

        let allowed_set: HashSet<&str> = allowed_deps.iter().copied().collect();

        for dep in &actual_deps {
            if !allowed_set.contains(dep.as_str()) {
                violations.push(format!(
                    "  {crate_name} → {dep} (NOT ALLOWED — add to allowed_dependency_graph() if intentional)"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "\nArchitecture violation: unauthorized internal dependencies!\n\n{}\n\n\
         If these are intentional, update allowed_dependency_graph() in this test.\n",
        violations.join("\n")
    );
}

#[test]
fn foundation_crates_have_no_internal_deps() {
    let root = workspace_root();
    let crates = discover_crates(&root);
    let foundation = ["oco-shared-types", "oco-shared-proto"];

    for (name, path) in &crates {
        if foundation.contains(&name.as_str()) {
            let deps = extract_internal_deps(path);
            assert!(
                deps.is_empty(),
                "Foundation crate {name} must have ZERO internal dependencies, but has: {deps:?}"
            );
        }
    }
}

#[test]
fn no_circular_layer_violations() {
    // Verify that no lower-layer crate depends on a higher-layer crate.
    // Layers: 0=foundation, 1=single-dep, 2=mid, 3=aggregator, 4=app
    let layers: HashMap<&str, u8> = [
        ("oco-shared-types", 0),
        ("oco-shared-proto", 0),
        ("oco-policy-engine", 1),
        ("oco-code-intel", 1),
        ("oco-retrieval", 1),
        ("oco-tool-runtime", 1),
        ("oco-verifier", 1),
        ("oco-telemetry", 1),
        ("oco-context-engine", 2),
        ("oco-planner", 2),
        ("oco-orchestrator-core", 3),
        ("oco-mcp-server", 4),
        ("oco-dev-cli", 4),
    ]
    .into_iter()
    .collect();

    let root = workspace_root();
    let crates = discover_crates(&root);
    let mut violations = Vec::new();

    for (name, path) in &crates {
        let Some(&my_layer) = layers.get(name.as_str()) else {
            continue;
        };

        let deps = extract_internal_deps(path);
        for dep in &deps {
            if let Some(&dep_layer) = layers.get(dep.as_str())
                && dep_layer > my_layer
                && my_layer < 4
            {
                violations.push(format!(
                    "  {name} (layer {my_layer}) → {dep} (layer {dep_layer}) — upward dependency!"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "\nLayer violation: lower-layer crate depends on higher-layer crate!\n\n{}\n",
        violations.join("\n")
    );
}

#[test]
fn all_workspace_crates_are_covered_by_graph() {
    let root = workspace_root();
    let allowed = allowed_dependency_graph();
    let crates = discover_crates(&root);

    let covered: HashSet<&str> = allowed.keys().copied().collect();
    let mut missing = Vec::new();

    for (name, _) in &crates {
        if !covered.contains(name.as_str()) {
            missing.push(name.as_str());
        }
    }

    assert!(
        missing.is_empty(),
        "\nNew crates not covered by architecture tests: {missing:?}\n\
         Add them to allowed_dependency_graph() with their allowed dependencies.\n"
    );
}
