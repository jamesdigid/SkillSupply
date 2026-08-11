use std::path::Path;

use crate::error::{Result, SkillSupportError};
use crate::registry::traits::RegistryProvider;
use crate::storage::capability::{CapabilitySource, FilesystemCapability};
use crate::storage::loader::FilesystemCapabilityLoader;

#[derive(Default)]
pub struct LocalRegistry {
    capabilities: Vec<FilesystemCapability>,
    loader: FilesystemCapabilityLoader,
}

impl LocalRegistry {
    pub fn new(loader: FilesystemCapabilityLoader) -> Self {
        Self {
            capabilities: Vec::new(),
            loader,
        }
    }

    pub fn load_from_filesystem(
        &mut self,
        capability_root: &Path,
    ) -> Result<&FilesystemCapability> {
        let capability = self.loader.load(capability_root)?;
        self.register(capability);
        self.capabilities
            .last()
            .ok_or_else(|| SkillSupportError::Registry("capability registry is empty".to_string()))
    }
}

impl RegistryProvider for LocalRegistry {
    fn register(&mut self, capability: FilesystemCapability) {
        self.capabilities.push(capability);
    }

    fn capabilities(&self) -> &[FilesystemCapability] {
        &self.capabilities
    }

    fn find_by_capability(&self, contract: &str) -> Vec<FilesystemCapability> {
        self.capabilities
            .iter()
            .filter(|capability_record| {
                capability_record
                    .manifest()
                    .capabilities
                    .iter()
                    .any(|entry| entry == contract)
            })
            .cloned()
            .collect()
    }
}
