use std::collections::HashMap;
use std::fs;
use std::path::Path;

use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SkillSupportError};
use crate::provider::{ProviderRef, ProviderScheme};

static EMBEDDED_REGISTRY: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/registry");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry {
    pub capability: String,
    #[serde(default)]
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RegistryIndex {
    entries: HashMap<String, Vec<ProviderRef>>,
}

impl RegistryIndex {
    pub fn load_from_dir(root: &Path) -> Result<Self> {
        let mut entries = HashMap::new();
        for entry in fs::read_dir(root)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("yaml") {
                continue;
            }
            let source = fs::read_to_string(&path)?;
            let registry_entry: RegistryEntry = serde_yaml_ng::from_str(&source)?;
            let providers = registry_entry
                .providers
                .iter()
                .map(|provider| {
                    let reference: ProviderRef = provider.parse()?;
                    Ok(resolve_provider_ref(root, reference))
                })
                .collect::<Result<Vec<_>>>()?;
            entries.insert(registry_entry.capability, providers);
        }
        Ok(Self { entries })
    }

    pub fn load_embedded() -> Result<Self> {
        let mut entries = HashMap::new();
        for file in EMBEDDED_REGISTRY.files() {
            if file.path().extension().and_then(|value| value.to_str()) != Some("yaml") {
                continue;
            }
            let source = file.contents_utf8().ok_or_else(|| {
                SkillSupportError::Registry(format!(
                    "embedded registry file is not utf-8: {}",
                    file.path().display()
                ))
            })?;
            let registry_entry: RegistryEntry = serde_yaml_ng::from_str(source)?;
            let providers = registry_entry
                .providers
                .iter()
                .map(|provider| {
                    let reference: ProviderRef = provider.parse()?;
                    Ok(resolve_provider_ref(
                        Path::new(env!("CARGO_MANIFEST_DIR")),
                        reference,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            entries.insert(registry_entry.capability, providers);
        }
        Ok(Self { entries })
    }

    pub fn providers_for(&self, capability: &str) -> Vec<ProviderRef> {
        self.entries.get(capability).cloned().unwrap_or_default()
    }

    pub fn capabilities(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl From<HashMap<String, Vec<ProviderRef>>> for RegistryIndex {
    fn from(entries: HashMap<String, Vec<ProviderRef>>) -> Self {
        Self { entries }
    }
}

impl RegistryIndex {
    pub fn into_inner(self) -> HashMap<String, Vec<ProviderRef>> {
        self.entries
    }
}

fn resolve_provider_ref(root: &Path, reference: ProviderRef) -> ProviderRef {
    match reference.scheme {
        ProviderScheme::Local | ProviderScheme::File => {
            let location = Path::new(&reference.location);
            if location.is_absolute() {
                reference
            } else {
                ProviderRef {
                    scheme: reference.scheme,
                    location: root.join(location).display().to_string(),
                    reference: reference.reference,
                }
            }
        }
        ProviderScheme::Github => reference,
    }
}
