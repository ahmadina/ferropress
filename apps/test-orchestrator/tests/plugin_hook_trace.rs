//! Lab fixture plugins produce the same load hook trace as the recorded lifecycle.

use std::path::PathBuf;

use test_orchestrator::{OrchestratorError, load_scenario, run_scenario};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn single_site_plugin_load_matches_the_lab() {
    let report = run_scenario(&repo_root(), "plugin-hook-trace").unwrap();
    assert!(report.passed(), "{}", test_orchestrator::describe(&report));
    assert_eq!(report.scenario.as_str(), "plugin-hook-trace");
    assert_eq!(report.steps.len(), 1);
}

#[test]
fn network_plugin_load_matches_the_lab() {
    let report = run_scenario(&repo_root(), "plugin-hook-trace-network").unwrap();
    assert!(report.passed(), "{}", test_orchestrator::describe(&report));
    assert_eq!(report.mismatch_count(), 0);
}

#[test]
fn unknown_scenarios_are_rejected() {
    let error = run_scenario(&repo_root(), "front-page").unwrap_err();
    assert!(
        matches!(error, OrchestratorError::Unsupported(_)),
        "{error}"
    );
}

#[test]
fn committed_scenarios_parse() {
    let single = load_scenario(&repo_root(), "plugin-hook-trace").unwrap();
    assert_eq!(single.id().as_str(), "plugin-hook-trace");
    assert_eq!(single.compare().len(), 2);
    let network = load_scenario(&repo_root(), "plugin-hook-trace-network").unwrap();
    assert_eq!(network.profile().as_str(), "network");
}

#[test]
fn missing_scenario_files_are_read_errors() {
    let error = load_scenario(&repo_root(), "does-not-exist").unwrap_err();
    assert!(matches!(error, OrchestratorError::Read { .. }), "{error}");
}
