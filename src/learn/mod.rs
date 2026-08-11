use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Result, SkillSupportError};
use crate::manifest::models::WorkspaceManifest;
use crate::manifest::validation::ManifestValidator;
use crate::provider::{
    GitProviderFetcher, LocalProviderFetcher, ProviderFetcher, ProviderRef, ProviderScheme,
};
use crate::reasoning::{CandidateProvider, DeterministicSelector, ProviderSelector};
use crate::registry::RegistryIndex;
use crate::runtime::RuntimeController;
use crate::utils::paths::WORKSPACE_FILENAME;
use crate::verification::VerificationRunner;

#[derive(Debug, Clone, Default)]
pub struct LearnOptions {
    pub registry_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct LearnOutcome {
    pub capability: String,
    pub provider_root: PathBuf,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LearnEngine;

impl LearnEngine {
    pub fn learn(&self, capability: &str, options: &LearnOptions) -> Result<LearnOutcome> {
        let workspace_root = std::env::current_dir()?;
        let registry = load_registry(options)?;
        let mut visited = HashSet::new();
        learn_inner(capability, &workspace_root, &registry, &mut visited)
    }
}

fn learn_inner(
    capability: &str,
    workspace_root: &Path,
    registry: &RegistryIndex,
    visited: &mut HashSet<String>,
) -> Result<LearnOutcome> {
    if !visited.insert(capability.to_string()) {
        return Err(SkillSupportError::Registry(format!(
            "cyclic capability dependency detected at {capability}"
        )));
    }

    if workspace_contains(workspace_root, capability)? {
        return Ok(LearnOutcome {
            capability: capability.to_string(),
            provider_root: workspace_root
                .join("capabilities")
                .join(sanitize_capability(capability)),
        });
    }

    let candidates = registry.providers_for(capability);
    if candidates.is_empty() {
        return Err(SkillSupportError::Registry(format!(
            "no providers registered for {capability}"
        )));
    }

    let temp_root = workspace_root.join(".skillsupport").join("acquired");
    fs::create_dir_all(&temp_root)?;

    let fetcher = LocalProviderFetcher;
    let git_fetcher = GitProviderFetcher;
    let validator = ManifestValidator::default();
    let selector = DeterministicSelector;
    let mut candidate_records = Vec::new();

    for reference in candidates {
        let fetched_root = fetch_provider(&reference, &temp_root, &fetcher, &git_fetcher)?;
        let report = validator.validate_path(&fetched_root);
        let Some(manifest) = report.manifest.clone() else {
            continue;
        };
        if !report.valid {
            continue;
        }
        candidate_records.push(CandidateProvider {
            reference,
            root: fetched_root,
            manifest,
        });
    }

    let selected = selector.select(capability, candidate_records)?;

    for dependency in selected.manifest.dependencies.clone() {
        learn_inner(&dependency, workspace_root, registry, visited)?;
    }

    let runtime_controller = RuntimeController;
    let mut session = runtime_controller.start(&selected.manifest, &selected.root)?;
    let verification_runner = VerificationRunner;
    verification_runner.verify(&selected.root, &selected.manifest, &mut session)?;
    session.shutdown(&selected.manifest, &selected.root)?;

    let installed_root = install_provider(workspace_root, capability, &selected.root)?;
    record_workspace_capability(workspace_root, capability)?;

    Ok(LearnOutcome {
        capability: capability.to_string(),
        provider_root: installed_root,
    })
}

fn load_registry(options: &LearnOptions) -> Result<RegistryIndex> {
    if let Some(registry_dir) = &options.registry_dir {
        return RegistryIndex::load_from_dir(registry_dir);
    }

    if let Ok(value) = std::env::var("EPISTEM_REGISTRY") {
        return RegistryIndex::load_from_dir(Path::new(&value));
    }

    RegistryIndex::load_embedded()
}

fn fetch_provider(
    reference: &ProviderRef,
    temp_root: &Path,
    local_fetcher: &LocalProviderFetcher,
    git_fetcher: &GitProviderFetcher,
) -> Result<PathBuf> {
    match reference.scheme {
        ProviderScheme::Github => git_fetcher.fetch(reference, temp_root),
        ProviderScheme::Local | ProviderScheme::File => local_fetcher.fetch(reference, temp_root),
    }
}

fn install_provider(
    workspace_root: &Path,
    capability: &str,
    source_root: &Path,
) -> Result<PathBuf> {
    let destination = workspace_root
        .join("capabilities")
        .join(sanitize_capability(capability));
    if destination.exists() {
        fs::remove_dir_all(&destination)?;
    }
    copy_dir_all(source_root, &destination)?;
    Ok(destination)
}

fn record_workspace_capability(workspace_root: &Path, capability: &str) -> Result<()> {
    let manifest_path = workspace_root.join(WORKSPACE_FILENAME);
    let mut workspace = if manifest_path.exists() {
        let source = fs::read_to_string(&manifest_path)?;
        serde_yaml_ng::from_str::<WorkspaceManifest>(&source)?
    } else {
        WorkspaceManifest {
            name: workspace_root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("skillsupport-workspace")
                .to_string(),
            version: "0.1.0".to_string(),
            capabilities: Vec::new(),
        }
    };

    if !workspace
        .capabilities
        .iter()
        .any(|entry| entry == capability)
    {
        workspace.capabilities.push(capability.to_string());
    }

    fs::write(manifest_path, serde_yaml_ng::to_string(&workspace)?)?;
    Ok(())
}

fn workspace_contains(workspace_root: &Path, capability: &str) -> Result<bool> {
    let manifest_path = workspace_root.join(WORKSPACE_FILENAME);
    if !manifest_path.exists() {
        return Ok(false);
    }

    let source = fs::read_to_string(manifest_path)?;
    let workspace = serde_yaml_ng::from_str::<WorkspaceManifest>(&source)?;
    Ok(workspace
        .capabilities
        .iter()
        .any(|entry| entry == capability))
}

fn sanitize_capability(capability: &str) -> String {
    capability
        .chars()
        .map(|character| match character {
            '/' | '@' | ':' => '-',
            other => other,
        })
        .collect()
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&from, &to)?;
        } else {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
