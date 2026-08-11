use std::path::Path;

use crate::error::{Result, SkillSupportError};
use crate::manifest::validation::ManifestValidator;
use crate::storage::capability::FilesystemCapability;

#[derive(Default)]
pub struct FilesystemCapabilityLoader {
    validator: ManifestValidator,
}

impl FilesystemCapabilityLoader {
    pub fn new(validator: ManifestValidator) -> Self {
        Self { validator }
    }

    pub fn load(&self, capability_root: &Path) -> Result<FilesystemCapability> {
        if !capability_root.is_dir() {
            return Err(SkillSupportError::MissingManifest(capability_root.to_path_buf()));
        }

        let report = self.validator.validate_path(capability_root);
        match report.manifest {
            Some(manifest) if report.valid => Ok(FilesystemCapability::new(
                capability_root.to_path_buf(),
                manifest,
            )),
            Some(_) | None => Err(SkillSupportError::InvalidManifest {
                path: report.path,
                reason: report
                    .issues
                    .iter()
                    .map(|issue| format!("{}: {}", issue.field, issue.message))
                    .collect::<Vec<_>>()
                    .join("; "),
            }),
        }
    }
}
