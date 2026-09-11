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

    let workspace_manifest = tempdir.path().join("caps.yaml");
    let source = fs::read_to_string(workspace_manifest).expect("workspace manifest");
    assert!(source.contains("browser-attach"));

    // `init`, `learn`, and `graph` must agree on the install directory. Asserting
    // each in isolation lets them drift apart, which is how `caps/` and
    // `capabilities/` diverged previously.
    assert!(
        tempdir
            .path()
            .join("caps")
            .join("browser-attach")
            .join("caps.yaml")
            .exists(),
        "learn should install into the caps/ directory scaffolded by init"
    );

    assert!(
        !tempdir.path().join("caps").join(".acquired").exists(),
        "staged provider copies should be removed after a successful install"
    );

    Command::cargo_bin("caps")
        .expect("binary")
        .current_dir(tempdir.path())
        .arg("graph")
        .assert()
        .success()
        .stdout(contains("browser-attach"));
}

#[test]
fn cli_explains_why_each_provider_was_rejected() {
    let tempdir = tempdir().expect("temp dir");
    let registry = tempdir.path().join("registry");
    let provider = tempdir.path().join("provider-without-manifest");
    let workspace = tempdir.path().join("workspace");
    for directory in [&registry, &provider, &workspace] {
        fs::create_dir_all(directory).expect("directory");
    }

    fs::write(
        registry.join("needs-manifest.yaml"),
        format!(
            "capability: needs-manifest\nproviders:\n  - local:{}\n",
            provider.display()
        ),
    )
    .expect("registry entry");

    Command::cargo_bin("caps")
        .expect("binary")
        .current_dir(&workspace)
        .args([
            "--registry",
            registry.to_str().expect("registry path"),
            "learn",
            "needs-manifest",
        ])
        .assert()
        .failure()
        .stderr(contains("rejected providers"))
        .stderr(contains("caps.yaml: not found"));
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
