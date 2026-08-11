use std::path::PathBuf;
use std::process::Command;

use crate::error::{Result, SkillSupportError};
use crate::manifest::models::CapabilityManifest;
use crate::provider::ProviderRef;

#[derive(Debug, Clone)]
pub struct CandidateProvider {
    pub reference: ProviderRef,
    pub root: PathBuf,
    pub manifest: CapabilityManifest,
}

pub trait ProviderSelector {
    fn select(
        &self,
        capability: &str,
        candidates: Vec<CandidateProvider>,
    ) -> Result<CandidateProvider>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicSelector;

impl ProviderSelector for DeterministicSelector {
    fn select(
        &self,
        capability: &str,
        candidates: Vec<CandidateProvider>,
    ) -> Result<CandidateProvider> {
        for candidate in candidates {
            if !candidate
                .manifest
                .capabilities
                .iter()
                .any(|entry| entry == capability)
            {
                continue;
            }
            if environment_satisfies(&candidate.manifest) {
                return Ok(candidate);
            }
        }

        Err(SkillSupportError::Registry(format!(
            "no compatible provider found for capability {capability}"
        )))
    }
}

fn environment_satisfies(manifest: &CapabilityManifest) -> bool {
    let Some(install) = &manifest.install else {
        return true;
    };

    install
        .requires
        .iter()
        .all(|requirement| command_is_available(requirement))
}

fn command_is_available(requirement: &str) -> bool {
    let command = requirement
        .split([' ', '>', '='])
        .next()
        .unwrap_or(requirement)
        .trim();
    if command.is_empty() {
        return false;
    }

    Command::new(command).arg("--version").output().is_ok()
        || Command::new("sh")
            .arg("-lc")
            .arg(format!("command -v {command}"))
            .output()
            .is_ok_and(|output| output.status.success())
}
