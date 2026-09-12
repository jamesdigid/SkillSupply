use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::VERSION;
use crate::contract::{ContractSha, ContractStore};
use crate::error::{
    JSON_RPC_CONTRACT_MISMATCH, JSON_RPC_DANGLING_CONTRACT_REF, JSON_RPC_REGISTRATION_CONFLICT,
    JSON_RPC_UNKNOWN_CONTRACT,
};
use crate::runtime::lifecycle::{LifecycleBus, LifecycleEvent};
use crate::runtime::services::registry::{
    CapabilityRegistration, CapabilityRegistryError, RuntimeCapabilityRegistry,
};
use crate::runtime::transport::session::SessionId;

use super::session::SessionManager;

type Handler = dyn Fn(MethodContext) -> HandlerResult + Send + Sync + 'static;
type HandlerResult = std::result::Result<Value, JsonRpcError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonRpcRequest {
    pub method: String,
    pub params: Option<Value>,
    pub id: Value,
    pub notification: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodContext {
    pub caller: SessionId,
    pub request: JsonRpcRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Value,
}

impl JsonRpcResponse {
    pub fn success(result: Value, id: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn failure(error: JsonRpcError, id: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(error),
            id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcError {
    pub fn parse_error() -> Self {
        Self {
            code: -32700,
            message: "Parse error".to_string(),
        }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: message.into(),
        }
    }

    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {method}"),
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }

    pub fn server_error(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Default)]
pub struct Router {
    handlers: HashMap<String, Arc<Handler>>,
}

impl Router {
    pub fn register<F>(&mut self, method: impl Into<String>, handler: F)
    where
        F: Fn(MethodContext) -> HandlerResult + Send + Sync + 'static,
    {
        self.handlers.insert(method.into(), Arc::new(handler));
    }

    pub fn has_method(&self, method: &str) -> bool {
        self.handlers.contains_key(method)
    }

    pub fn dispatch_text(&self, caller: SessionId, source: &str) -> Option<JsonRpcResponse> {
        let value = match serde_json::from_str::<Value>(source) {
            Ok(value) => value,
            Err(_) => {
                return Some(JsonRpcResponse::failure(
                    JsonRpcError::parse_error(),
                    Value::Null,
                ));
            }
        };

        match parse_message(value) {
            Ok(JsonRpcMessage::Request(request)) => {
                self.dispatch_request(MethodContext { caller, request })
            }
            Ok(JsonRpcMessage::Response(_)) => None,
            Err(error) => Some(JsonRpcResponse::failure(error, Value::Null)),
        }
    }

    pub fn dispatch_request(&self, context: MethodContext) -> Option<JsonRpcResponse> {
        let Some(handler) = self.handlers.get(&context.request.method) else {
            if context.request.notification {
                return None;
            }
            return Some(JsonRpcResponse::failure(
                JsonRpcError::method_not_found(&context.request.method),
                context.request.id,
            ));
        };

        let id = context.request.id.clone();
        let notification = context.request.notification;
        match handler(context) {
            Ok(_) if notification => None,
            Ok(result) => Some(JsonRpcResponse::success(result, id)),
            Err(_) if notification => None,
            Err(error) => Some(JsonRpcResponse::failure(error, id)),
        }
    }
}

pub fn register_runtime_methods(
    router: &mut Router,
    sessions: Arc<SessionManager>,
    registry: Arc<RuntimeCapabilityRegistry>,
    contract_store: Arc<dyn ContractStore>,
    lifecycle: Arc<LifecycleBus>,
    started_at: Instant,
) {
    router.register("runtime.ping", |_| Ok(Value::String("pong".to_string())));

    router.register("runtime.version", |_| {
        Ok(serde_json::json!({
            "name": "skillsupport",
            "version": VERSION,
        }))
    });

    let health_registry = Arc::clone(&registry);
    router.register("runtime.health", move |_| {
        Ok(serde_json::json!({
            "status": "ok",
            "uptime_secs": started_at.elapsed().as_secs(),
            "sessions": sessions.connected_count(),
            "capabilities": health_registry.capability_count(),
        }))
    });

    let register_registry = Arc::clone(&registry);
    let register_contract_store = Arc::clone(&contract_store);
    let register_lifecycle = Arc::clone(&lifecycle);
    router.register("runtime.register", move |context| {
        let params = context.request.params.unwrap_or(Value::Null);
        let registration = serde_json::from_value::<CapabilityRegistration>(params)
            .map_err(|error| JsonRpcError::invalid_params(error.to_string()))?;
        let registered = register_registry
            .register(
                context.caller,
                registration,
                register_contract_store.as_ref(),
            )
            .map_err(registry_error_to_json_rpc)?;

        register_lifecycle.emit(LifecycleEvent::CapabilityRegistered {
            session_id: context.caller,
            capability: registered.clone(),
        });

        Ok(serde_json::json!({
            "capability": registered.capability,
            "methods": registered.methods,
            "version": registered.version,
        }))
    });

    let unregister_registry = Arc::clone(&registry);
    let unregister_lifecycle = Arc::clone(&lifecycle);
    router.register("runtime.unregister", move |context| {
        let params = context
            .request
            .params
            .unwrap_or_else(|| serde_json::json!({}));
        let params = serde_json::from_value::<UnregisterParams>(params)
            .map_err(|error| JsonRpcError::invalid_params(error.to_string()))?;

        let removed = if let Some(capability) = params.capability {
            unregister_registry
                .unregister_capability(context.caller, &capability)
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            unregister_registry.unregister_session(context.caller)
        };

        for capability in &removed {
            unregister_lifecycle.emit(LifecycleEvent::CapabilityUnregistered {
                session_id: context.caller,
                capability: capability.clone(),
            });
        }

        Ok(serde_json::json!({
            "unregistered": removed,
        }))
    });

    let methods_registry = Arc::clone(&registry);
    router.register("runtime.methods", move |_| {
        Ok(serde_json::json!({
            "capabilities": methods_registry.list(),
        }))
    });

    #[cfg(debug_assertions)]
    {
        let contracts_store = Arc::clone(&contract_store);
        router.register("runtime.contracts", move |_| {
            let contracts = contracts_store.list().map_err(|error| {
                JsonRpcError::server_error(JSON_RPC_CONTRACT_MISMATCH, error.to_string())
            })?;
            Ok(serde_json::json!({
                "count": contracts.len(),
                "contracts": contracts,
            }))
        });
    }

    let contract_store = Arc::clone(&contract_store);
    router.register("runtime.contract", move |context| {
        let params = context.request.params.unwrap_or(Value::Null);
        let params = serde_json::from_value::<ContractParams>(params)
            .map_err(|error| JsonRpcError::invalid_params(error.to_string()))?;
        let Some(contract) = contract_store.get(&params.sha).map_err(|error| {
            JsonRpcError::server_error(JSON_RPC_CONTRACT_MISMATCH, error.to_string())
        })?
        else {
            return Err(JsonRpcError::server_error(
                JSON_RPC_UNKNOWN_CONTRACT,
                format!("unknown contract sha: {}", params.sha),
            ));
        };

        serde_json::to_value(contract).map_err(|error| {
            JsonRpcError::server_error(
                JSON_RPC_CONTRACT_MISMATCH,
                format!("failed to serialize contract: {error}"),
            )
        })
    });
}

#[derive(Debug, Deserialize)]
struct UnregisterParams {
    capability: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContractParams {
    sha: ContractSha,
}

pub fn parse_message(value: Value) -> std::result::Result<JsonRpcMessage, JsonRpcError> {
    let Some(object) = value.as_object() else {
        return Err(JsonRpcError::invalid_request("message must be an object"));
    };

    if object.contains_key("method") {
        parse_request(value).map(JsonRpcMessage::Request)
    } else if object.contains_key("result") || object.contains_key("error") {
        parse_response(value).map(JsonRpcMessage::Response)
    } else {
        Err(JsonRpcError::invalid_request(
            "message must contain method, result, or error",
        ))
    }
}

pub fn parse_request(value: Value) -> std::result::Result<JsonRpcRequest, JsonRpcError> {
    let Some(object) = value.as_object() else {
        return Err(JsonRpcError::invalid_request("request must be an object"));
    };

    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err(JsonRpcError::invalid_request("jsonrpc must be \"2.0\""));
    }

    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Err(JsonRpcError::invalid_request("method must be a string"));
    };

    Ok(JsonRpcRequest {
        method: method.to_string(),
        params: object.get("params").cloned(),
        id: object.get("id").cloned().unwrap_or(Value::Null),
        notification: !object.contains_key("id"),
    })
}

pub fn parse_response(value: Value) -> std::result::Result<JsonRpcResponse, JsonRpcError> {
    let response = serde_json::from_value::<JsonRpcResponse>(value)
        .map_err(|error| JsonRpcError::invalid_request(error.to_string()))?;

    if response.jsonrpc != "2.0" {
        return Err(JsonRpcError::invalid_request("jsonrpc must be \"2.0\""));
    }

    if response.result.is_none() && response.error.is_none() {
        return Err(JsonRpcError::invalid_request(
            "response must contain result or error",
        ));
    }

    Ok(response)
}

fn registry_error_to_json_rpc(error: CapabilityRegistryError) -> JsonRpcError {
    let code = match error {
        CapabilityRegistryError::ContractMismatch(_) => JSON_RPC_CONTRACT_MISMATCH,
        CapabilityRegistryError::UnknownContract(_) => JSON_RPC_UNKNOWN_CONTRACT,
        CapabilityRegistryError::DanglingContractRef(_) => JSON_RPC_DANGLING_CONTRACT_REF,
        _ => JSON_RPC_REGISTRATION_CONFLICT,
    };
    JsonRpcError::server_error(code, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_registered_method() {
        let mut router = Router::default();
        router.register("runtime.ping", |_| Ok(Value::String("pong".to_string())));

        let response = router
            .dispatch_text(
                SessionId::new(1),
                r#"{"jsonrpc":"2.0","method":"runtime.ping","id":1}"#,
            )
            .expect("response");

        assert_eq!(response.result, Some(Value::String("pong".to_string())));
        assert_eq!(response.id, Value::from(1));
    }

    #[test]
    fn unknown_method_returns_json_rpc_error() {
        let router = Router::default();

        let response = router
            .dispatch_text(
                SessionId::new(1),
                r#"{"jsonrpc":"2.0","method":"runtime.missing","id":"a"}"#,
            )
            .expect("response");

        assert_eq!(
            response.error.expect("error"),
            JsonRpcError::method_not_found("runtime.missing")
        );
        assert_eq!(response.id, Value::from("a"));
    }

    #[test]
    fn notifications_do_not_return_responses() {
        let mut router = Router::default();
        router.register("runtime.ping", |_| Ok(Value::String("pong".to_string())));

        let response = router.dispatch_text(
            SessionId::new(1),
            r#"{"jsonrpc":"2.0","method":"runtime.ping"}"#,
        );

        assert!(response.is_none());
    }
}
