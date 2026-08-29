use std::env;
use std::fs;
use std::path::PathBuf;

use skillsupport::learn::{LearnEngine, LearnOptions};
use skillsupport::manifest::WorkspaceManifest;
use tempfile::tempdir;

struct CwdGuard {
    previous: PathBuf,
}

impl Drop for CwdGuard {
    fn drop(&mut self) {
        let _ = env::set_current_dir(&self.previous);
    }
}

fn registry_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/registry")
}

#[test]
fn learns_and_records_edge_browser_attach() {
    let tempdir = tempdir().expect("temp dir");
    let previous = env::current_dir().expect("cwd");
    let _guard = CwdGuard { previous };
    env::set_current_dir(tempdir.path()).expect("switch cwd");

    let outcome = LearnEngine
        .learn(
            "browser-attach",
            &LearnOptions {
                registry_dir: Some(registry_root()),
            },
        )
        .expect("learn should succeed");

    assert_eq!(outcome.capability, "browser-attach");
    assert!(outcome.provider_root.exists());

    let workspace_manifest = tempdir.path().join("skillsupport.yaml");
    assert!(workspace_manifest.exists());
    let source = fs::read_to_string(workspace_manifest).expect("workspace manifest");
    let workspace = serde_yaml_ng::from_str::<WorkspaceManifest>(&source).expect("workspace yaml");
    assert!(
        workspace
            .capabilities
            .iter()
            .any(|capability| capability == "browser-attach")
    );

    let installed_manifest = tempdir
        .path()
        .join("capabilities")
        .join("browser-attach")
        .join("caps.yaml");
    assert!(installed_manifest.exists());
}
