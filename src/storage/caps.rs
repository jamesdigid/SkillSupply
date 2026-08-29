use std::path::{Path, PathBuf};

use crate::manifest::models::CapabilityManifest;

pub trait CapabilitySource {
    fn root(&self) -> &Path;

    fn manifest(&self) -> &CapabilityManifest;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemCapability {
    root: PathBuf,
    manifest: CapabilityManifest,
}

impl FilesystemCapability {
    pub fn new(root: PathBuf, manifest: CapabilityManifest) -> Self {
        Self { root, manifest }
    }
}

impl CapabilitySource for FilesystemCapability {
    fn root(&self) -> &Path {
        &self.root
    }

    fn manifest(&self) -> &CapabilityManifest {
        &self.manifest
    }
}
