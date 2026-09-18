use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum SkillSupportError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml_ng::Error),

    #[error("missing capability manifest at {0}")]
    MissingManifest(PathBuf),

    #[error("invalid capability manifest at {path}: {reason}")]
    InvalidManifest { path: PathBuf, reason: String },

    #[error("dependency resolution failed: {0}")]
    Resolution(String),

    #[error("registry error: {0}")]
    Registry(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("transport error: {0}")]
    Transport(String),

    #[error("search error: {0}")]
    Search(String),
}

pub type Result<T> = std::result::Result<T, SkillSupportError>;

pub const JSON_RPC_REGISTRATION_CONFLICT: i64 = -32001;
pub const JSON_RPC_FORWARD_TARGET_GONE: i64 = -32002;
pub const JSON_RPC_FORWARD_TIMEOUT: i64 = -32003;
pub const JSON_RPC_SELF_INVOCATION: i64 = -32004;
pub const JSON_RPC_CONTRACT_MISMATCH: i64 = -32008;
pub const JSON_RPC_UNKNOWN_CONTRACT: i64 = -32009;
pub const JSON_RPC_DANGLING_CONTRACT_REF: i64 = -32010;
