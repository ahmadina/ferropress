//! End-to-end tests of the `xtask` command-line interface.
#![expect(
    clippy::expect_used,
    reason = "test helper: failing to spawn the binary must fail the test"
)]

use std::process::{Command, Output};

fn xtask(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("xtask binary runs")
}

#[test]
fn check_layers_succeeds_on_the_workspace() {
    let output = xtask(&["check-layers"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("layer check passed:"), "{stdout}");
}

#[test]
fn verify_manifest_succeeds_on_the_committed_manifest() {
    let output = xtask(&["verify-manifest"]);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.starts_with("manifest valid:"), "{stdout}");
}

#[test]
fn help_prints_usage() {
    let output = xtask(&["help"]);
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("check-layers")
    );
}

#[test]
fn unknown_command_exits_with_usage_error() {
    let output = xtask(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unknown command `frobnicate`"), "{stderr}");
    assert!(stderr.contains("usage: cargo xtask"), "{stderr}");
}
