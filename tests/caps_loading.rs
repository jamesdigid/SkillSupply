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

#[cfg(unix)]
#[test]
fn runtime_session_drop_reaps_child_process() {
    use skillsupport::runtime::capabilities::RuntimeController;

    let loader = FilesystemCapabilityLoader::default();
    let root = std::path::Path::new("tests/fixtures/providers/browser-attach");
    let capability = loader.load(root).expect("capability should load");
    let session = RuntimeController
        .start(capability.manifest(), root)
        .expect("runtime should start");
    let pid = session.child_id().expect("runtime should have a child");

    assert!(pid_is_live(pid));
    drop(session);
    wait_for_process_exit(pid);
}

#[cfg(unix)]
fn pid_is_live(pid: u32) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg("kill -0 \"$1\" 2>/dev/null")
        .arg("kill")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
fn wait_for_process_exit(pid: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if !pid_is_live(pid) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("child process {pid} stayed alive after RuntimeSession drop");
}
