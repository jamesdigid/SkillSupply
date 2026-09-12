use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::contract::{Contract, ContractSha, canonical_bytes};
use crate::utils::paths::contracts_dir_for;

pub trait ContractStore: Send + Sync {
    fn insert(&self, contract: &Contract) -> std::result::Result<ContractSha, ContractStoreError>;

    fn get(&self, sha: &ContractSha) -> std::result::Result<Option<Contract>, ContractStoreError>;

    fn list(&self) -> std::result::Result<Vec<ContractSha>, ContractStoreError>;

    fn contains(&self, sha: &ContractSha) -> std::result::Result<bool, ContractStoreError> {
        self.get(sha).map(|contract| contract.is_some())
    }
}

#[derive(Debug, Error)]
pub enum ContractStoreError {
    #[error("contract mismatch for {sha}")]
    Mismatch { sha: ContractSha },
    #[error("unknown contract sha: {sha}")]
    Unknown { sha: ContractSha },
    #[error("dangling contract ref: {sha}")]
    DanglingRef { sha: ContractSha },
    #[error("contract store I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("contract store JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("contract store error: {0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub struct FilesystemContractStore {
    root: PathBuf,
}

impl FilesystemContractStore {
    pub fn new(workspace_root: impl AsRef<Path>) -> Self {
        Self {
            root: contracts_dir_for(workspace_root.as_ref()),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path_for(&self, sha: &ContractSha) -> PathBuf {
        let hex = sha.hex();
        self.root
            .join("sha256")
            .join(&hex[..2])
            .join(format!("{}.json", &hex[2..]))
    }
}

impl ContractStore for FilesystemContractStore {
    fn insert(&self, contract: &Contract) -> std::result::Result<ContractSha, ContractStoreError> {
        for reference in &contract.refs {
            if !self.contains(reference)? {
                return Err(ContractStoreError::DanglingRef {
                    sha: reference.clone(),
                });
            }
        }

        let value = serde_json::to_value(contract)?;
        let bytes = canonical_bytes(&value).map_err(|error| {
            ContractStoreError::Other(format!("failed to canonicalize contract: {error}"))
        })?;
        let sha = ContractSha::new(format!("{:x}", Sha256::digest(&bytes)))
            .map_err(|error| ContractStoreError::Other(error.to_string()))?;
        let path = self.path_for(&sha);

        if path.exists() {
            let existing = fs::read(&path)?;
            if existing != bytes {
                return Err(ContractStoreError::Mismatch { sha });
            }
            return Ok(sha);
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, bytes)?;
        Ok(sha)
    }

    fn get(&self, sha: &ContractSha) -> std::result::Result<Option<Contract>, ContractStoreError> {
        let path = self.path_for(sha);
        if !path.exists() {
            return Ok(None);
        }

        let data = fs::read_to_string(path)?;
        Ok(Some(serde_json::from_str(&data)?))
    }

    fn list(&self) -> std::result::Result<Vec<ContractSha>, ContractStoreError> {
        let sha_root = self.root.join("sha256");
        if !sha_root.exists() {
            return Ok(Vec::new());
        }

        let mut contracts = Vec::new();
        for shard in fs::read_dir(sha_root)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            let prefix = shard.file_name().to_string_lossy().to_string();
            if prefix.len() != 2 {
                continue;
            }

            for entry in fs::read_dir(shard.path())? {
                let entry = entry?;
                if !entry.file_type()?.is_file() {
                    continue;
                }
                if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
                    continue;
                }
                let path = entry.path();
                let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                    continue;
                };
                let sha = ContractSha::new(format!("{prefix}{stem}"))
                    .map_err(|error| ContractStoreError::Other(error.to_string()))?;
                contracts.push(sha);
            }
        }

        contracts.sort();
        Ok(contracts)
    }
}

impl From<ContractStoreError> for crate::error::SkillSupportError {
    fn from(error: ContractStoreError) -> Self {
        crate::error::SkillSupportError::Registry(error.to_string())
    }
}
