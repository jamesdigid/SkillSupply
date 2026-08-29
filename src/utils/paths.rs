use std::path::{Path, PathBuf};

pub const MANIFEST_FILENAME: &str = "caps.yaml";
pub const WORKSPACE_FILENAME: &str = "skillsupport.yaml";
pub const PROMPT_FILENAME: &str = "prompt.md";

pub fn manifest_path_for(capability_root: &Path) -> PathBuf {
    capability_root.join(MANIFEST_FILENAME)
}
