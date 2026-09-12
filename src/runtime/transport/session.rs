use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SessionId(u64);

impl SessionId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

impl Display for SessionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "session-{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Connected,
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatState {
    Unknown,
    Alive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportMetadata {
    pub peer_addr: Option<SocketAddr>,
    pub connected_at: SystemTime,
    pub disconnected_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub state: SessionState,
    pub heartbeat: HeartbeatState,
    pub last_seen_at: SystemTime,
    pub transport: TransportMetadata,
}

#[derive(Debug, Default)]
pub struct SessionManager {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<SessionId, Session>>,
    outbound: Mutex<HashMap<SessionId, Sender<String>>>,
}

impl SessionManager {
    pub fn register(&self, peer_addr: Option<SocketAddr>) -> SessionId {
        let id = SessionId::new(self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        let now = SystemTime::now();
        let session = Session {
            id,
            state: SessionState::Connected,
            heartbeat: HeartbeatState::Unknown,
            last_seen_at: now,
            transport: TransportMetadata {
                peer_addr,
                connected_at: now,
                disconnected_at: None,
            },
        };

        self.sessions
            .lock()
            .expect("session manager mutex poisoned")
            .insert(id, session);
        id
    }

    pub fn mark_disconnected(&self, id: SessionId) {
        if let Some(session) = self
            .sessions
            .lock()
            .expect("session manager mutex poisoned")
            .get_mut(&id)
        {
            session.state = SessionState::Disconnected;
            session.transport.disconnected_at = Some(SystemTime::now());
        }
    }

    pub fn reap_disconnected(&self, ttl: Duration) -> Vec<SessionId> {
        let now = SystemTime::now();
        let mut sessions = self
            .sessions
            .lock()
            .expect("session manager mutex poisoned");
        let expired = sessions
            .iter()
            .filter_map(|(id, session)| {
                if session.state != SessionState::Disconnected {
                    return None;
                }

                let disconnected_at = session.transport.disconnected_at?;
                let disconnected_for = now.duration_since(disconnected_at).ok()?;
                (disconnected_for >= ttl).then_some(*id)
            })
            .collect::<Vec<_>>();

        for id in &expired {
            sessions.remove(id);
        }
        drop(sessions);

        let mut outbound = self
            .outbound
            .lock()
            .expect("session manager outbound mutex poisoned");
        for id in &expired {
            outbound.remove(id);
        }

        expired
    }

    pub fn touch(&self, id: SessionId) {
        if let Some(session) = self
            .sessions
            .lock()
            .expect("session manager mutex poisoned")
            .get_mut(&id)
        {
            session.heartbeat = HeartbeatState::Alive;
            session.last_seen_at = SystemTime::now();
        }
    }

    pub fn connected_count(&self) -> usize {
        self.sessions
            .lock()
            .expect("session manager mutex poisoned")
            .values()
            .filter(|session| session.state == SessionState::Connected)
            .count()
    }

    pub fn count(&self) -> usize {
        self.sessions
            .lock()
            .expect("session manager mutex poisoned")
            .len()
    }

    pub fn list(&self) -> Vec<Session> {
        self.sessions
            .lock()
            .expect("session manager mutex poisoned")
            .values()
            .cloned()
            .collect()
    }

    pub fn get(&self, id: SessionId) -> Option<Session> {
        self.sessions
            .lock()
            .expect("session manager mutex poisoned")
            .get(&id)
            .cloned()
    }

    pub fn attach_outbound(&self, id: SessionId) -> Receiver<String> {
        let (sender, receiver) = channel();
        self.outbound
            .lock()
            .expect("session manager outbound mutex poisoned")
            .insert(id, sender);
        receiver
    }

    pub fn detach_outbound(&self, id: SessionId) {
        self.outbound
            .lock()
            .expect("session manager outbound mutex poisoned")
            .remove(&id);
    }

    pub fn send(&self, id: SessionId, payload: String) -> bool {
        let sender = self
            .outbound
            .lock()
            .expect("session manager outbound mutex poisoned")
            .get(&id)
            .cloned();

        sender
            .map(|sender| sender.send(payload).is_ok())
            .unwrap_or(false)
    }

    pub fn broadcast(&self, payload: String) -> usize {
        let senders = self
            .outbound
            .lock()
            .expect("session manager outbound mutex poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();

        senders
            .into_iter()
            .filter(|sender| sender.send(payload.clone()).is_ok())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_tracks_connected_sessions() {
        let manager = SessionManager::default();

        let id = manager.register(None);

        assert_eq!(manager.count(), 1);
        assert_eq!(manager.connected_count(), 1);
        assert_eq!(
            manager.get(id).expect("session").state,
            SessionState::Connected
        );
    }

    #[test]
    fn mark_disconnected_updates_session_state() {
        let manager = SessionManager::default();
        let id = manager.register(None);

        manager.mark_disconnected(id);

        let session = manager.get(id).expect("session");
        assert_eq!(session.state, SessionState::Disconnected);
        assert!(session.transport.disconnected_at.is_some());
        assert_eq!(manager.connected_count(), 0);
    }

    #[test]
    fn reap_disconnected_removes_expired_sessions() {
        let manager = SessionManager::default();
        let id = manager.register(None);
        manager.mark_disconnected(id);

        assert_eq!(
            manager.reap_disconnected(Duration::from_millis(0)),
            vec![id]
        );
        assert_eq!(manager.count(), 0);
    }

    #[test]
    fn sends_to_attached_outbound_channel() {
        let manager = SessionManager::default();
        let id = manager.register(None);
        let receiver = manager.attach_outbound(id);

        assert!(manager.send(id, "hello".to_string()));
        assert_eq!(receiver.recv().expect("outbound message"), "hello");
    }

    #[test]
    fn broadcast_sends_to_all_attached_channels() {
        let manager = SessionManager::default();
        let first = manager.register(None);
        let second = manager.register(None);
        let first_rx = manager.attach_outbound(first);
        let second_rx = manager.attach_outbound(second);

        assert_eq!(manager.broadcast("event".to_string()), 2);
        assert_eq!(first_rx.recv().expect("first message"), "event");
        assert_eq!(second_rx.recv().expect("second message"), "event");
    }
}
