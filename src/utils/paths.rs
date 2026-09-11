use std::path::{Path, PathBuf};

/// Capability manifest, found at the root of each installed capability.
pub const MANIFEST_FILENAME: &str = "caps.yaml";
/// Workspace manifest, found at the workspace root. Shares a name with
/// `MANIFEST_FILENAME` but holds a `WorkspaceManifest`, never a
/// `CapabilityManifest`; the two never share a directory.
pub const WORKSPACE_FILENAME: &str = "caps.yaml";
pub const WORKSPACE_DOC_FILENAME: &str = "CAPS.md";
pub const PROMPT_FILENAME: &str = "prompt.md";
/// Workspace-relative directory holding installed capabilities.
pub const CAPS_DIRNAME: &str = "caps";
/// Staging area for fetched providers. Lives inside `caps/` so the workspace
/// root stays clean, and is dot-prefixed so it is not mistaken for a capability.
pub const ACQUIRED_DIRNAME: &str = ".acquired";

pub fn manifest_path_for(capability_root: &Path) -> PathBuf {
    capability_root.join(MANIFEST_FILENAME)
}

pub fn caps_dir_for(workspace_root: &Path) -> PathBuf {
    workspace_root.join(CAPS_DIRNAME)
}

pub fn acquired_dir_for(workspace_root: &Path) -> PathBuf {
    caps_dir_for(workspace_root).join(ACQUIRED_DIRNAME)
}
