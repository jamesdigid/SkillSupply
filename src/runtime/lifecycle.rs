use std::sync::{Arc, Mutex};

use serde_json::Value;

use crate::runtime::services::registry::RegisteredCapability;
use crate::runtime::transport::{SessionId, SessionManager};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LifecycleEvent {
    SessionConnected {
        session_id: SessionId,
    },
    CapabilityRegistered {
        session_id: SessionId,
        capability: RegisteredCapability,
    },
    CapabilityUnregistered {
        session_id: SessionId,
        capability: RegisteredCapability,
    },
    SessionDisconnected {
        session_id: SessionId,
    },
    RuntimeShuttingDown,
}

pub trait LifecycleObserver: Send + Sync {
    fn on_event(&self, event: &LifecycleEvent);
}

#[derive(Default)]
pub struct LifecycleBus {
    observers: Mutex<Vec<Arc<dyn LifecycleObserver>>>,
}

impl LifecycleBus {
    pub fn register(&self, observer: Arc<dyn LifecycleObserver>) {
        self.observers
            .lock()
            .expect("lifecycle observers mutex poisoned")
            .push(observer);
    }

    pub fn emit(&self, event: LifecycleEvent) {
        let observers = self
            .observers
            .lock()
            .expect("lifecycle observers mutex poisoned")
            .clone();

        for observer in observers {
            observer.on_event(&event);
        }
    }
}

pub struct NotificationBroadcaster {
    sessions: Arc<SessionManager>,
}

impl NotificationBroadcaster {
    pub fn new(sessions: Arc<SessionManager>) -> Self {
        Self { sessions }
    }
}

impl LifecycleObserver for NotificationBroadcaster {
    fn on_event(&self, event: &LifecycleEvent) {
        if let Ok(payload) = serde_json::to_string(&notification_for(event)) {
            self.sessions.broadcast(payload);
        }
    }
}

fn notification_for(event: &LifecycleEvent) -> Value {
    let (method, params) = match event {
        LifecycleEvent::SessionConnected { session_id } => (
            "lifecycle.session_connected",
            serde_json::json!({
                "session_id": session_id.to_string(),
            }),
        ),
        LifecycleEvent::CapabilityRegistered {
            session_id,
            capability,
        } => (
            "lifecycle.capability_registered",
            serde_json::json!({
                "session_id": session_id.to_string(),
                "capability": capability,
            }),
        ),
        LifecycleEvent::CapabilityUnregistered {
            session_id,
            capability,
        } => (
            "lifecycle.capability_unregistered",
            serde_json::json!({
                "session_id": session_id.to_string(),
                "capability": capability,
            }),
        ),
        LifecycleEvent::SessionDisconnected { session_id } => (
            "lifecycle.session_disconnected",
            serde_json::json!({
                "session_id": session_id.to_string(),
            }),
        ),
        LifecycleEvent::RuntimeShuttingDown => {
            ("lifecycle.runtime_shutdown", serde_json::json!({}))
        }
    };

    serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadcaster_sends_json_rpc_notification() {
        let sessions = Arc::new(SessionManager::default());
        let id = sessions.register(None);
        let receiver = sessions.attach_outbound(id);
        let broadcaster = NotificationBroadcaster::new(Arc::clone(&sessions));

        broadcaster.on_event(&LifecycleEvent::SessionConnected { session_id: id });

        let payload = receiver.recv().expect("notification");
        let value: Value = serde_json::from_str(&payload).expect("json notification");
        assert_eq!(value["method"], "lifecycle.session_connected");
        assert_eq!(value["params"]["session_id"], id.to_string());
    }
}
