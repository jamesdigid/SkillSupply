use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

fn registry_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/registry")
}

#[test]
fn cli_learns_a_provider_and_records_it() {
    let tempdir = tempdir().expect("temp dir");
    let registry = registry_root();

    Command::cargo_bin("caps")
        .expect("binary")
        .current_dir(tempdir.path())
        .arg("init")
        .assert()
        .success();

    Command::cargo_bin("caps")
        .expect("binary")
        .current_dir(tempdir.path())
        .args([
            "--registry",
            registry.to_str().expect("registry path"),
            "learn",
            "browser-attach",
        ])
        .assert()
        .success()
        .stdout(contains("learned capability browser-attach"));

    let workspace_manifest = tempdir.path().join("skillsupport.yaml");
    let source = fs::read_to_string(workspace_manifest).expect("workspace manifest");
    assert!(source.contains("browser-attach"));
}

#[test]
fn cli_reports_verification_failures() {
    let tempdir = tempdir().expect("temp dir");
    let registry = registry_root();

    Command::cargo_bin("caps")
        .expect("binary")
        .current_dir(tempdir.path())
        .args([
            "--registry",
            registry.to_str().expect("registry path"),
            "learn",
            "browser-attach-fail",
        ])
        .assert()
        .failure()
        .stderr(contains("verification"));
}
