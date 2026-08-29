use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::runtime::transport::SessionId;

pub trait ServiceRegistry {}

pub trait CapabilityRegistry {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRegistration {
    pub capability: String,
    pub methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegisteredCapability {
    pub capability: String,
    pub methods: Vec<String>,
    #[serde(skip)]
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodOwner {
    pub capability: String,
    pub session_id: SessionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityRegistryError {
    EmptyCapability,
    EmptyMethods,
    DuplicateMethod(String),
    ReservedMethod(String),
    CapabilityAlreadyRegistered(String),
    MethodAlreadyRegistered(String),
}

impl Display for CapabilityRegistryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyCapability => write!(f, "capability must not be empty"),
            Self::EmptyMethods => write!(f, "methods must not be empty"),
            Self::DuplicateMethod(method) => write!(f, "duplicate method: {method}"),
            Self::ReservedMethod(method) => write!(f, "reserved method prefix: {method}"),
            Self::CapabilityAlreadyRegistered(capability) => {
                write!(f, "capability already registered: {capability}")
            }
            Self::MethodAlreadyRegistered(method) => {
                write!(f, "method already registered: {method}")
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct RuntimeCapabilityRegistry {
    capabilities: Mutex<HashMap<String, RegisteredCapability>>,
    methods: Mutex<HashMap<String, MethodOwner>>,
}

impl RuntimeCapabilityRegistry {
    pub fn register(
        &self,
        session_id: SessionId,
        registration: CapabilityRegistration,
    ) -> Result<RegisteredCapability, CapabilityRegistryError> {
        validate_registration(&registration)?;

        let mut capabilities = self
            .capabilities
            .lock()
            .expect("capability registry mutex poisoned");
        let mut methods = self
            .methods
            .lock()
            .expect("capability method registry mutex poisoned");

        if capabilities.contains_key(&registration.capability) {
            return Err(CapabilityRegistryError::CapabilityAlreadyRegistered(
                registration.capability,
            ));
        }

        for method in &registration.methods {
            if methods.contains_key(method) {
                return Err(CapabilityRegistryError::MethodAlreadyRegistered(
                    method.clone(),
                ));
            }
        }

        let registered = RegisteredCapability {
            capability: registration.capability,
            methods: registration.methods,
            session_id,
            version: registration.version,
        };

        for method in &registered.methods {
            methods.insert(
                method.clone(),
                MethodOwner {
                    capability: registered.capability.clone(),
                    session_id,
                },
            );
        }
        capabilities.insert(registered.capability.clone(), registered.clone());

        Ok(registered)
    }

    pub fn unregister_session(&self, session_id: SessionId) -> Vec<RegisteredCapability> {
        let mut capabilities = self
            .capabilities
            .lock()
            .expect("capability registry mutex poisoned");
        let mut methods = self
            .methods
            .lock()
            .expect("capability method registry mutex poisoned");

        let names = capabilities
            .values()
            .filter(|capability| capability.session_id == session_id)
            .map(|capability| capability.capability.clone())
            .collect::<Vec<_>>();

        names
            .into_iter()
            .filter_map(|name| capabilities.remove(&name))
            .inspect(|capability| {
                for method in &capability.methods {
                    methods.remove(method);
                }
            })
            .collect()
    }

    pub fn unregister_capability(
        &self,
        session_id: SessionId,
        capability: &str,
    ) -> Option<RegisteredCapability> {
        let mut capabilities = self
            .capabilities
            .lock()
            .expect("capability registry mutex poisoned");
        let mut methods = self
            .methods
            .lock()
            .expect("capability method registry mutex poisoned");

        let registered = capabilities.get(capability)?;
        if registered.session_id != session_id {
            return None;
        }

        let registered = capabilities.remove(capability)?;
        for method in &registered.methods {
            methods.remove(method);
        }
        Some(registered)
    }

    pub fn owner(&self, method: &str) -> Option<MethodOwner> {
        self.methods
            .lock()
            .expect("capability method registry mutex poisoned")
            .get(method)
            .cloned()
    }

    pub fn list(&self) -> Vec<RegisteredCapability> {
        self.capabilities
            .lock()
            .expect("capability registry mutex poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn capability_count(&self) -> usize {
        self.capabilities
            .lock()
            .expect("capability registry mutex poisoned")
            .len()
    }
}

fn validate_registration(
    registration: &CapabilityRegistration,
) -> Result<(), CapabilityRegistryError> {
    if registration.capability.trim().is_empty() {
        return Err(CapabilityRegistryError::EmptyCapability);
    }

    if registration.methods.is_empty() {
        return Err(CapabilityRegistryError::EmptyMethods);
    }

    let mut seen = HashSet::new();
    for method in &registration.methods {
        if method.starts_with("runtime.") || method.starts_with("lifecycle.") {
            return Err(CapabilityRegistryError::ReservedMethod(method.clone()));
        }
        if !seen.insert(method.clone()) {
            return Err(CapabilityRegistryError::DuplicateMethod(method.clone()));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registration(methods: &[&str]) -> CapabilityRegistration {
        CapabilityRegistration {
            capability: "math".to_string(),
            methods: methods.iter().map(|method| method.to_string()).collect(),
            version: Some("1.0.0".to_string()),
        }
    }

    #[test]
    fn registers_and_resolves_method_owner() {
        let registry = RuntimeCapabilityRegistry::default();
        let session_id = SessionId::new(1);

        registry
            .register(session_id, registration(&["math.add"]))
            .expect("register capability");

        assert_eq!(
            registry.owner("math.add"),
            Some(MethodOwner {
                capability: "math".to_string(),
                session_id,
            })
        );
    }

    #[test]
    fn rejects_method_conflicts() {
        let registry = RuntimeCapabilityRegistry::default();
        registry
            .register(SessionId::new(1), registration(&["math.add"]))
            .expect("first registration");

        let error = registry
            .register(
                SessionId::new(2),
                CapabilityRegistration {
                    capability: "other".to_string(),
                    methods: vec!["math.add".to_string()],
                    version: None,
                },
            )
            .expect_err("conflict");

        assert_eq!(
            error,
            CapabilityRegistryError::MethodAlreadyRegistered("math.add".to_string())
        );
    }

    #[test]
    fn unregister_session_releases_methods() {
        let registry = RuntimeCapabilityRegistry::default();
        let session_id = SessionId::new(1);
        registry
            .register(session_id, registration(&["math.add"]))
            .expect("register capability");

        let removed = registry.unregister_session(session_id);

        assert_eq!(removed.len(), 1);
        assert!(registry.owner("math.add").is_none());
    }
}
