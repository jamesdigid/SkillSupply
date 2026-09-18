use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContractSha(String);

impl ContractSha {
    pub fn new(hex: impl Into<String>) -> Result<Self, ContractShaParseError> {
        let hex = hex.into();
        let value = if hex.starts_with("sha256:") {
            hex
        } else {
            format!("sha256:{hex}")
        };
        validate(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn hex(&self) -> &str {
        &self.0["sha256:".len()..]
    }
}

impl Display for ContractSha {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ContractSha {
    type Err = ContractShaParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        validate(value)?;
        Ok(Self(value.to_string()))
    }
}

impl Serialize for ContractSha {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ContractSha {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractShaParseError {
    value: String,
}

impl Display for ContractShaParseError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid contract sha: {}", self.value)
    }
}

impl std::error::Error for ContractShaParseError {}

fn validate(value: &str) -> Result<(), ContractShaParseError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(ContractShaParseError {
            value: value.to_string(),
        });
    };

    if hex.len() != 64 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(ContractShaParseError {
            value: value.to_string(),
        });
    }

    Ok(())
}
