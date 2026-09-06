use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use crate::error::Result;

use self::config::RuntimeConfig;
use self::lifecycle::{LifecycleBus, LifecycleEvent, NotificationBroadcaster};
use self::services::registry::RuntimeCapabilityRegistry;
use self::transport::{
    Dispatcher, PendingRequests, Router, SessionManager, WebSocketTransport,
    register_runtime_methods,
};

#[path = "caps/mod.rs"]
pub mod capabilities;
pub mod config;
pub mod lifecycle;
pub mod services;
pub mod transport;

pub fn serve(config: &RuntimeConfig, shutdown: Arc<AtomicBool>) -> Result<()> {
    let sessions = Arc::new(SessionManager::default());
    let registry = Arc::new(RuntimeCapabilityRegistry::default());
    let pending = Arc::new(PendingRequests::default());
    let lifecycle = Arc::new(LifecycleBus::default());
    lifecycle.register(Arc::new(NotificationBroadcaster::new(Arc::clone(
        &sessions,
    ))));

    let mut router = Router::default();
    register_runtime_methods(
        &mut router,
        Arc::clone(&sessions),
        Arc::clone(&registry),
        Arc::clone(&lifecycle),
        Instant::now(),
    );
    let router = Arc::new(router);

    let dispatcher = Dispatcher::new(
        Arc::clone(&router),
        registry,
        Arc::clone(&sessions),
        pending,
        Arc::clone(&lifecycle),
        config.forward_timeout(),
    );

    let transport = WebSocketTransport::bind(config.socket_addr()?)?;
    let result = transport.serve(dispatcher, sessions, shutdown);
    lifecycle.emit(LifecycleEvent::RuntimeShuttingDown);
    result
}
