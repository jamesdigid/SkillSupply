use skillsupport::storage::{CapabilitySource, FilesystemCapabilityLoader};

#[test]
fn loads_capability_from_disk() {
    let loader = FilesystemCapabilityLoader::default();
    let capability = loader
        .load(std::path::Path::new("examples/gmail-send"))
        .expect("capability should load");

    assert_eq!(
        capability.root().display().to_string(),
        "examples/gmail-send"
    );
    assert_eq!(capability.manifest().name, "gmail-send");
}
