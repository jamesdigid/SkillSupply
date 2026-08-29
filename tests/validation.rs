use skillsupport::manifest::ManifestValidator;
use std::fs;
use tempfile::tempdir;

const SAMPLE_MANIFEST: &str = r#"name: sample
version: 0.1.0
capabilities:
  - sample-capability
runtime:
  type: available
"#;

#[test]
fn validates_manifest() {
    let validator = ManifestValidator::default();
    let report = validator.validate_path("examples/gmail-send");

    assert!(report.valid, "{:#?}", report.issues);
    let manifest = report.manifest.expect("manifest should be present");
    assert_eq!(manifest.name, "gmail-send");
    assert_eq!(manifest.capabilities, vec!["gmail"]);
}

#[test]
fn accepts_plural_capabilities_manifest_filename() {
    let dir = tempdir().expect("temp dir");
    fs::write(dir.path().join("caps.yaml"), SAMPLE_MANIFEST).expect("write manifest");

    let validator = ManifestValidator::default();
    let report = validator.validate_path(dir.path());

    assert!(report.valid, "{:#?}", report.issues);
    assert!(
        report.path.ends_with("caps.yaml"),
        "expected plural manifest path, got {}",
        report.path.display()
    );
}

#[test]
fn requires_plural_capabilities_manifest_filename() {
    let dir = tempdir().expect("temp dir");
    // The legacy singular filename must NOT be discovered.
    fs::write(dir.path().join("capability.yaml"), SAMPLE_MANIFEST).expect("write manifest");

    let validator = ManifestValidator::default();
    let report = validator.validate_path(dir.path());

    assert!(
        !report.valid,
        "singular capability.yaml should not be treated as a valid manifest"
    );
    assert!(
        report.path.ends_with("caps.yaml"),
        "validator should look for the plural manifest, got {}",
        report.path.display()
    );
}
