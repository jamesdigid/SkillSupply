use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

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
    let reaper = start_pending_reaper(
        dispatcher.clone(),
        Arc::clone(&sessions),
        Arc::clone(&shutdown),
        config.reaper_tick(),
        config.session_ttl(),
    );

    let result = transport.serve(dispatcher, sessions, Arc::clone(&shutdown));
    lifecycle.emit(LifecycleEvent::RuntimeShuttingDown);
    shutdown.store(true, Ordering::SeqCst);
    let _ = reaper.join();
    result
}

fn start_pending_reaper(
    dispatcher: Dispatcher,
    sessions: Arc<SessionManager>,
    shutdown: Arc<AtomicBool>,
    reaper_tick: Duration,
    session_ttl: Duration,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while !shutdown.load(Ordering::SeqCst) {
            dispatcher.reap_expired_pending();
            sessions.reap_disconnected(session_ttl);

            let sleep_for = dispatcher
                .next_pending_deadline()
                .map(|deadline| {
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(reaper_tick)
                })
                .unwrap_or(reaper_tick);
            thread::sleep(sleep_for);
        }

        dispatcher.reap_expired_pending();
        sessions.reap_disconnected(session_ttl);
    })
}
