use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::ContractSha;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Immediate,
    Job,
}

fn default_execution() -> ExecutionMode {
    ExecutionMode::Immediate
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contract {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default = "default_execution")]
    pub execution: ExecutionMode,
    #[serde(default)]
    pub cancelable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<ContractSha>,
}
