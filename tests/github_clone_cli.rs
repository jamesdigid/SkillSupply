use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use assert_cmd::Command as AssertCommand;
use predicates::str::contains;
use tempfile::tempdir;

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent dir");
    }
    fs::write(path, contents).expect("file write");
}

fn write_fake_git(bin_dir: &Path) {
    let git_path = bin_dir.join("git");
    let script = r#"#!/bin/sh
set -eu
if [ "$1" = "clone" ]; then
  shift
  while [ "$1" = "--depth" ]; do
    shift 2
  done
  url="$1"
  dest="$2"
  src="${url#file://}"
  mkdir -p "$dest"
  cp -R "$src"/. "$dest"/
  exit 0
fi
if [ "$1" = "-C" ] && [ "$3" = "checkout" ]; then
  exit 0
fi
exit 1
"#;
    fs::write(&git_path, script).expect("git shim");
    let mut permissions = fs::metadata(&git_path).expect("git metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&git_path, permissions).expect("git permissions");
}

#[test]
fn cli_learns_from_a_github_provider_via_git_clone() {
    let workspace = tempdir().expect("workspace");
    let registry = tempdir().expect("registry");
    let remote_root = tempdir().expect("remote root");
    let shim_root = tempdir().expect("shim root");

    let source_repo = remote_root.path().join("jamesdigid/browser-attach.git");
    fs::create_dir_all(&source_repo).expect("source repo dir");
    write_file(
        &source_repo.join("caps.yaml"),
        "name: browser-attach\nversion: 1.0.0\ndescription: Git-backed browser attach fixture\ncapabilities:\n  - browser-attach\ndependencies: []\ninstall:\n  requires: []\n  steps: []\nruntime:\n  type: launch\n  initialize: sh bin/provider.sh\n  ready:\n    type: stdio_handshake\n    expected: ready\n  shutdown: null\ncommunication:\n  transport: stdio\nprompt: prompt.md\ntests:\n  startup: tests/startup.yaml\n  smoke: tests/smoke.yaml\nkeywords:\n  - browser\n",
    );
    write_file(
        &source_repo.join("prompt.md"),
        "# Browser Attach\n\nGit-backed provider fixture.\n",
    );
    write_file(
        &source_repo.join("tests/startup.yaml"),
        "name: startup\nsteps:\n  - operation: ping\n    expect:\n      status: ok\n",
    );
    write_file(
        &source_repo.join("tests/smoke.yaml"),
        "name: smoke\nsteps:\n  - operation: ping\n    expect:\n      status: ok\n",
    );
    write_file(
        &source_repo.join("bin/provider.sh"),
        "#!/bin/sh\nprintf 'ready\\n'\nwhile IFS= read -r line; do\n  case \"$line\" in\n    *\"ping\"*) printf '{\"status\":\"ok\"}\\n' ;;\n    *) printf '{\"status\":\"unknown\"}\\n' ;;\n  esac\ndone\n",
    );

    write_file(
        &registry.path().join("browser-attach.yaml"),
        "capability: browser-attach\nproviders:\n  - github:jamesdigid/browser-attach\n",
    );

    let shim_bin = shim_root.path().join("bin");
    fs::create_dir_all(&shim_bin).expect("shim bin");
    write_fake_git(&shim_bin);

    let base_url = format!("file://{}", remote_root.path().display());
    let path_env = format!(
        "{}:{}",
        shim_bin.display(),
        std::env::var("PATH").expect("path")
    );

    AssertCommand::cargo_bin("caps")
        .expect("binary")
        .current_dir(workspace.path())
        .env("SKILLSUPPORT_GITHUB_BASE_URL", base_url)
        .env("PATH", path_env)
        .args([
            "--registry",
            registry.path().to_str().expect("registry"),
            "learn",
            "browser-attach",
        ])
        .assert()
        .success()
        .stdout(contains("learned capability browser-attach"));
}
