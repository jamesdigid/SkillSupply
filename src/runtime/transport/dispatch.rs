use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::error::{
    JSON_RPC_FORWARD_TARGET_GONE, JSON_RPC_FORWARD_TIMEOUT, JSON_RPC_SELF_INVOCATION,
};
use crate::runtime::lifecycle::{LifecycleBus, LifecycleEvent};
use crate::runtime::services::registry::RuntimeCapabilityRegistry;

use super::pending::{PendingForward, PendingRequests};
use super::router::{
    parse_message, JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, MethodContext,
    Router,
};
use super::session::{SessionId, SessionManager};

#[derive(Clone)]
pub struct Dispatcher {
    router: Arc<Router>,
    registry: Arc<RuntimeCapabilityRegistry>,
    sessions: Arc<SessionManager>,
    pending: Arc<PendingRequests>,
    lifecycle: Arc<LifecycleBus>,
    forward_timeout: Duration,
}

impl Dispatcher {
    pub fn new(
        router: Arc<Router>,
        registry: Arc<RuntimeCapabilityRegistry>,
        sessions: Arc<SessionManager>,
        pending: Arc<PendingRequests>,
        lifecycle: Arc<LifecycleBus>,
        forward_timeout: Duration,
    ) -> Self {
        Self {
            router,
            registry,
            sessions,
            pending,
            lifecycle,
            forward_timeout,
        }
    }

    pub fn handle_inbound(&self, caller: SessionId, raw: String) {
        let value = match serde_json::from_str::<Value>(&raw) {
            Ok(value) => value,
            Err(_) => {
                self.send_response(
                    caller,
                    JsonRpcResponse::failure(JsonRpcError::parse_error(), Value::Null),
                );
                return;
            }
        };

        match parse_message(value) {
            Ok(JsonRpcMessage::Response(response)) => self.handle_response(caller, response),
            Ok(JsonRpcMessage::Request(request)) => self.handle_request(caller, request),
            Err(error) => self.send_response(caller, JsonRpcResponse::failure(error, Value::Null)),
        }
    }

    pub fn handle_connect(&self, session_id: SessionId) {
        self.lifecycle
            .emit(LifecycleEvent::SessionConnected { session_id });
    }

    pub fn handle_disconnect(&self, session_id: SessionId) {
        for (_, forward) in self.pending.drain_owner(session_id) {
            self.send_response(
                forward.caller,
                JsonRpcResponse::failure(
                    JsonRpcError::server_error(
                        JSON_RPC_FORWARD_TARGET_GONE,
                        format!("forward target disconnected for method {}", forward.method),
                    ),
                    forward.original_id,
                ),
            );
        }
        self.pending.drain_caller(session_id);

        for capability in self.registry.unregister_session(session_id) {
            self.lifecycle.emit(LifecycleEvent::CapabilityUnregistered {
                session_id,
                capability,
            });
        }
        self.lifecycle
            .emit(LifecycleEvent::SessionDisconnected { session_id });
    }

    pub fn reap_expired_pending(&self) {
        for (_, forward) in self.pending.take_expired(Instant::now()) {
            self.send_response(
                forward.caller,
                JsonRpcResponse::failure(
                    JsonRpcError::server_error(
                        JSON_RPC_FORWARD_TIMEOUT,
                        format!("forward timed out for method {}", forward.method),
                    ),
                    forward.original_id,
                ),
            );
        }
    }

    pub fn next_pending_deadline(&self) -> Option<Instant> {
        self.pending.next_deadline()
    }

    fn handle_response(&self, responder: SessionId, mut response: JsonRpcResponse) {
        if let Some(id) = response.id.as_u64() {
            if let Some(forward) = self.pending.take(id, responder) {
                response.id = forward.original_id;
                self.send_response(forward.caller, response);
            }
        }
    }

    fn handle_request(&self, caller: SessionId, request: JsonRpcRequest) {
        if self.router.has_method(&request.method) {
            if let Some(response) = self
                .router
                .dispatch_request(MethodContext { caller, request })
            {
                self.send_response(caller, response);
            }
            return;
        }

        let Some(owner) = self.registry.owner(&request.method) else {
            if !request.notification {
                self.send_response(
                    caller,
                    JsonRpcResponse::failure(
                        JsonRpcError::method_not_found(&request.method),
                        request.id,
                    ),
                );
            }
            return;
        };

        if owner.session_id == caller {
            if !request.notification {
                self.send_response(
                    caller,
                    JsonRpcResponse::failure(
                        JsonRpcError::server_error(
                            JSON_RPC_SELF_INVOCATION,
                            format!("cannot invoke own registered method: {}", request.method),
                        ),
                        request.id,
                    ),
                );
            }
            return;
        }

        self.forward_request(caller, owner.session_id, request);
    }

    fn forward_request(&self, caller: SessionId, owner: SessionId, request: JsonRpcRequest) {
        if request.notification {
            let payload = request_payload(&request, None);
            if let Ok(payload) = serde_json::to_string(&payload) {
                self.sessions.send(owner, payload);
            }
            return;
        }

        let original_id = request.id.clone();
        let forward_id = self.pending.next_id();
        self.pending.insert(
            forward_id,
            PendingForward {
                caller,
                owner,
                original_id: original_id.clone(),
                method: request.method.clone(),
                deadline: Instant::now() + self.forward_timeout,
            },
        );
        let payload = request_payload(&request, Some(Value::from(forward_id)));
        let Ok(payload) = serde_json::to_string(&payload) else {
            self.pending.cancel(forward_id);
            return;
        };

        if !self.sessions.send(owner, payload) {
            self.pending.cancel(forward_id);
            self.send_response(
                caller,
                JsonRpcResponse::failure(
                    JsonRpcError::server_error(
                        JSON_RPC_FORWARD_TARGET_GONE,
                        format!("forward target disconnected for method {}", request.method),
                    ),
                    original_id,
                ),
            );
        }
    }

    fn send_response(&self, session_id: SessionId, response: JsonRpcResponse) {
        if let Ok(payload) = serde_json::to_string(&response) {
            self.sessions.send(session_id, payload);
        }
    }
}

fn request_payload(request: &JsonRpcRequest, id: Option<Value>) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("jsonrpc".to_string(), Value::String("2.0".to_string()));
    object.insert("method".to_string(), Value::String(request.method.clone()));
    if let Some(params) = request.params.clone() {
        object.insert("params".to_string(), params);
    }
    if let Some(id) = id {
        object.insert("id".to_string(), id);
    }
    Value::Object(object)
}
