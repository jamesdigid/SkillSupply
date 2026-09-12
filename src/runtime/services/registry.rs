use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::contract::{Contract, ContractSha, ContractStore, ContractStoreError};
use crate::runtime::transport::SessionId;

pub trait ServiceRegistry {}

pub trait CapabilityRegistry {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRegistration {
    pub capability: String,
    pub methods: Vec<MethodRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MethodRef {
    Name(String),
    Declared {
        name: String,
        contract: Contract,
    },
    Referenced {
        name: String,
        contract_sha: ContractSha,
    },
}

impl MethodRef {
    pub fn name(&self) -> &str {
        match self {
            Self::Name(name) | Self::Declared { name, .. } | Self::Referenced { name, .. } => name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegisteredMethod {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contract_sha: Option<ContractSha>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RegisteredCapability {
    pub capability: String,
    pub methods: Vec<RegisteredMethod>,
    #[serde(skip)]
    pub session_id: SessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodOwner {
    pub capability: String,
    pub session_id: SessionId,
    pub contract_sha: Option<ContractSha>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityRegistryError {
    EmptyCapability,
    EmptyMethods,
    DuplicateMethod(String),
    ReservedMethod(String),
    CapabilityAlreadyRegistered(String),
    MethodAlreadyRegistered(String),
    ContractMismatch(String),
    UnknownContract(String),
    DanglingContractRef(String),
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
            Self::ContractMismatch(message)
            | Self::UnknownContract(message)
            | Self::DanglingContractRef(message) => f.write_str(message),
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
        contract_store: &dyn ContractStore,
    ) -> Result<RegisteredCapability, CapabilityRegistryError> {
        validate_registration(&registration)?;

        let registered_methods = registration
            .methods
            .iter()
            .map(|method| resolve_method_ref(&registration.capability, method, contract_store))
            .collect::<std::result::Result<Vec<_>, _>>()?;

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

        for method in &registered_methods {
            if methods.contains_key(&method.name) {
                return Err(CapabilityRegistryError::MethodAlreadyRegistered(
                    method.name.clone(),
                ));
            }
        }

        let registered = RegisteredCapability {
            capability: registration.capability,
            methods: registered_methods,
            session_id,
            version: registration.version,
        };

        for method in &registered.methods {
            methods.insert(
                method.name.clone(),
                MethodOwner {
                    capability: registered.capability.clone(),
                    session_id,
                    contract_sha: method.contract_sha.clone(),
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
                    methods.remove(&method.name);
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
            methods.remove(&method.name);
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
        let name = method.name();
        if name.starts_with("runtime.") || name.starts_with("lifecycle.") {
            return Err(CapabilityRegistryError::ReservedMethod(name.to_string()));
        }
        if !seen.insert(name.to_string()) {
            return Err(CapabilityRegistryError::DuplicateMethod(name.to_string()));
        }
    }

    Ok(())
}

fn resolve_method_ref(
    _capability: &str,
    method: &MethodRef,
    contract_store: &dyn ContractStore,
) -> Result<RegisteredMethod, CapabilityRegistryError> {
    match method {
        MethodRef::Name(name) => Ok(RegisteredMethod {
            name: name.clone(),
            contract_sha: None,
            summary: None,
        }),
        MethodRef::Declared { name, contract } => {
            let contract_sha = contract_store
                .insert(contract)
                .map_err(registry_error_from_contract_store)?;
            Ok(RegisteredMethod {
                name: name.clone(),
                contract_sha: Some(contract_sha),
                summary: contract.summary.clone(),
            })
        }
        MethodRef::Referenced { name, contract_sha } => {
            let contract = contract_store
                .get(contract_sha)
                .map_err(registry_error_from_contract_store)?
                .ok_or_else(|| {
                    CapabilityRegistryError::UnknownContract(format!(
                        "unknown contract sha: {contract_sha}; re-register with the full body"
                    ))
                })?;
            Ok(RegisteredMethod {
                name: name.clone(),
                contract_sha: Some(contract_sha.clone()),
                summary: contract.summary,
            })
        }
    }
}

fn registry_error_from_contract_store(error: ContractStoreError) -> CapabilityRegistryError {
    match error {
        ContractStoreError::Mismatch { .. } => {
            CapabilityRegistryError::ContractMismatch(error.to_string())
        }
        ContractStoreError::Unknown { .. } => {
            CapabilityRegistryError::UnknownContract(error.to_string())
        }
        ContractStoreError::DanglingRef { .. } => {
            CapabilityRegistryError::DanglingContractRef(error.to_string())
        }
        error => CapabilityRegistryError::ContractMismatch(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::FilesystemContractStore;
    use tempfile::tempdir;

    fn registration(methods: &[&str]) -> CapabilityRegistration {
        CapabilityRegistration {
            capability: "math".to_string(),
            methods: methods
                .iter()
                .map(|method| MethodRef::Name(method.to_string()))
                .collect(),
            version: Some("1.0.0".to_string()),
        }
    }

    #[test]
    fn registers_and_resolves_method_owner() {
        let registry = RuntimeCapabilityRegistry::default();
        let tempdir = tempdir().expect("temp dir");
        let store = FilesystemContractStore::new(tempdir.path());
        let session_id = SessionId::new(1);

        registry
            .register(session_id, registration(&["math.add"]), &store)
            .expect("register capability");

        assert_eq!(
            registry.owner("math.add"),
            Some(MethodOwner {
                capability: "math".to_string(),
                session_id,
                contract_sha: None,
            })
        );
    }

    #[test]
    fn rejects_method_conflicts() {
        let registry = RuntimeCapabilityRegistry::default();
        let tempdir = tempdir().expect("temp dir");
        let store = FilesystemContractStore::new(tempdir.path());
        registry
            .register(SessionId::new(1), registration(&["math.add"]), &store)
            .expect("first registration");

        let error = registry
            .register(
                SessionId::new(2),
                CapabilityRegistration {
                    capability: "other".to_string(),
                    methods: vec![MethodRef::Name("math.add".to_string())],
                    version: None,
                },
                &store,
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
        let tempdir = tempdir().expect("temp dir");
        let store = FilesystemContractStore::new(tempdir.path());
        let session_id = SessionId::new(1);
        registry
            .register(session_id, registration(&["math.add"]), &store)
            .expect("register capability");

        let removed = registry.unregister_session(session_id);

        assert_eq!(removed.len(), 1);
        assert!(registry.owner("math.add").is_none());
    }
}
