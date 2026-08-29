use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use crate::error::{Result, SkillSupportError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub transport: TransportKind,
    pub host: String,
    pub port: u16,
    pub forward_timeout_ms: u64,
}

impl RuntimeConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path)?;
        let file: RuntimeConfigFile =
            toml::from_str(&source).map_err(|error| SkillSupportError::Config(error.to_string()))?;
        Ok(file.runtime.unwrap_or_default())
    }

    pub fn socket_addr(&self) -> Result<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse()
            .map_err(|error| SkillSupportError::Config(format!("invalid runtime address: {error}")))
    }

    pub fn transport_url(&self) -> String {
        format!("ws://{}:{}", self.host, self.port)
    }

    pub fn forward_timeout(&self) -> Duration {
        Duration::from_millis(self.forward_timeout_ms)
    }

    pub fn with_host(mut self, host: Option<String>) -> Self {
        if let Some(host) = host {
            self.host = host;
        }
        self
    }

    pub fn with_port(mut self, port: Option<u16>) -> Self {
        if let Some(port) = port {
            self.port = port;
        }
        self
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            transport: TransportKind::Websocket,
            host: "127.0.0.1".to_string(),
            port: 8787,
            forward_timeout_ms: 30_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Websocket,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeConfigFile {
    runtime: Option<RuntimeConfig>,
}

impl<'de> Deserialize<'de> for RuntimeConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct RawRuntimeConfig {
            #[serde(default = "default_transport")]
            transport: TransportKind,
            #[serde(default = "default_host")]
            host: String,
            #[serde(default = "default_port")]
            port: u16,
            #[serde(default = "default_forward_timeout_ms")]
            forward_timeout_ms: u64,
        }

        let raw = RawRuntimeConfig::deserialize(deserializer)?;
        Ok(Self {
            transport: raw.transport,
            host: raw.host,
            port: raw.port,
            forward_timeout_ms: raw.forward_timeout_ms,
        })
    }
}

fn default_transport() -> TransportKind {
    TransportKind::Websocket
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    8787
}

fn default_forward_timeout_ms() -> u64 {
    30_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_websocket_localhost() {
        let config = RuntimeConfig::default();

        assert_eq!(config.transport, TransportKind::Websocket);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8787);
        assert_eq!(config.forward_timeout_ms, 30_000);
    }

    #[test]
    fn deserializes_partial_runtime_table() {
        let file: RuntimeConfigFile = toml::from_str(
            r#"
            [runtime]
            port = 9000
            forward_timeout_ms = 50
            "#,
        )
        .expect("config should deserialize");

        let config = file.runtime.expect("runtime config");
        assert_eq!(config.transport, TransportKind::Websocket);
        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 9000);
        assert_eq!(config.forward_timeout_ms, 50);
    }
}
