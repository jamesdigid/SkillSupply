use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use serde_json::Value;

use super::session::SessionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingForward {
    pub caller: SessionId,
    pub owner: SessionId,
    pub original_id: Value,
    pub method: String,
    pub deadline: Instant,
}

#[derive(Debug, Default)]
pub struct PendingRequests {
    next_id: AtomicU64,
    inner: Mutex<PendingInner>,
}

#[derive(Debug, Default)]
struct PendingInner {
    entries: HashMap<u64, PendingForward>,
    by_caller: HashMap<SessionId, HashSet<u64>>,
    by_owner: HashMap<SessionId, HashSet<u64>>,
    deadlines: BinaryHeap<Reverse<(Instant, u64)>>,
}

impl PendingRequests {
    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn insert(&self, id: u64, forward: PendingForward) {
        let mut inner = self.inner.lock().expect("pending requests mutex poisoned");
        inner.remove(id);
        inner
            .by_caller
            .entry(forward.caller)
            .or_default()
            .insert(id);
        inner.by_owner.entry(forward.owner).or_default().insert(id);
        inner.deadlines.push(Reverse((forward.deadline, id)));
        inner.entries.insert(id, forward);
    }

    pub fn take(&self, id: u64, responder: SessionId) -> Option<PendingForward> {
        let mut inner = self.inner.lock().expect("pending requests mutex poisoned");
        let forward = inner.entries.get(&id)?;
        if forward.owner != responder {
            return None;
        }
        inner.remove(id)
    }

    pub fn cancel(&self, id: u64) -> Option<PendingForward> {
        self.inner
            .lock()
            .expect("pending requests mutex poisoned")
            .remove(id)
    }

    pub fn take_expired(&self, now: Instant) -> Vec<(u64, PendingForward)> {
        let mut inner = self.inner.lock().expect("pending requests mutex poisoned");
        let mut expired = Vec::new();

        while let Some(Reverse((deadline, id))) = inner.deadlines.peek().copied() {
            let Some(forward) = inner.entries.get(&id) else {
                inner.deadlines.pop();
                continue;
            };

            if forward.deadline != deadline {
                inner.deadlines.pop();
                continue;
            }

            if deadline > now {
                break;
            }

            inner.deadlines.pop();
            if let Some(forward) = inner.remove(id) {
                expired.push((id, forward));
            }
        }

        expired
    }

    pub fn drain_owner(&self, owner: SessionId) -> Vec<(u64, PendingForward)> {
        self.drain_index(owner, PendingIndex::Owner)
    }

    pub fn drain_caller(&self, caller: SessionId) -> Vec<(u64, PendingForward)> {
        self.drain_index(caller, PendingIndex::Caller)
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        let mut inner = self.inner.lock().expect("pending requests mutex poisoned");
        while let Some(Reverse((deadline, id))) = inner.deadlines.peek().copied() {
            let Some(forward) = inner.entries.get(&id) else {
                inner.deadlines.pop();
                continue;
            };

            if forward.deadline != deadline {
                inner.deadlines.pop();
                continue;
            }

            return Some(deadline);
        }

        None
    }

    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .expect("pending requests mutex poisoned")
            .entries
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner
            .lock()
            .expect("pending requests mutex poisoned")
            .entries
            .is_empty()
    }

    fn drain_index(
        &self,
        session_id: SessionId,
        index: PendingIndex,
    ) -> Vec<(u64, PendingForward)> {
        let mut inner = self.inner.lock().expect("pending requests mutex poisoned");
        let ids = match index {
            PendingIndex::Caller => inner.by_caller.remove(&session_id),
            PendingIndex::Owner => inner.by_owner.remove(&session_id),
        }
        .unwrap_or_default();

        ids.into_iter()
            .filter_map(|id| inner.remove(id).map(|forward| (id, forward)))
            .collect()
    }
}

impl PendingInner {
    fn remove(&mut self, id: u64) -> Option<PendingForward> {
        let forward = self.entries.remove(&id)?;
        remove_index(&mut self.by_caller, forward.caller, id);
        remove_index(&mut self.by_owner, forward.owner, id);
        Some(forward)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingIndex {
    Caller,
    Owner,
}

fn remove_index(index: &mut HashMap<SessionId, HashSet<u64>>, session_id: SessionId, id: u64) {
    if let Some(ids) = index.get_mut(&session_id) {
        ids.remove(&id);
        if ids.is_empty() {
            index.remove(&session_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::Value;

    use super::*;

    fn forward(caller: u64, owner: u64, deadline: Instant) -> PendingForward {
        PendingForward {
            caller: SessionId::new(caller),
            owner: SessionId::new(owner),
            original_id: Value::from(caller * 100),
            method: "math.add".to_string(),
            deadline,
        }
    }

    #[test]
    fn insert_and_take_correlates_owner_response() {
        let pending = PendingRequests::default();
        let deadline = Instant::now() + Duration::from_secs(1);
        pending.insert(42, forward(1, 2, deadline));

        assert!(pending.take(42, SessionId::new(1)).is_none());
        assert!(pending.take(42, SessionId::new(2)).is_some());
        assert!(pending.is_empty());
    }

    #[test]
    fn cancel_removes_pending_request() {
        let pending = PendingRequests::default();
        pending.insert(42, forward(1, 2, Instant::now()));

        assert!(pending.cancel(42).is_some());
        assert_eq!(pending.len(), 0);
    }

    #[test]
    fn take_expired_removes_due_entries() {
        let pending = PendingRequests::default();
        let now = Instant::now();
        pending.insert(1, forward(1, 2, now - Duration::from_millis(1)));
        pending.insert(2, forward(1, 2, now + Duration::from_secs(1)));

        let expired = pending.take_expired(now);

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].0, 1);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn drain_owner_removes_owner_entries() {
        let pending = PendingRequests::default();
        let deadline = Instant::now() + Duration::from_secs(1);
        pending.insert(1, forward(1, 2, deadline));
        pending.insert(2, forward(1, 3, deadline));

        let drained = pending.drain_owner(SessionId::new(2));

        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].0, 1);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn drain_caller_removes_caller_entries() {
        let pending = PendingRequests::default();
        let deadline = Instant::now() + Duration::from_secs(1);
        pending.insert(1, forward(1, 2, deadline));
        pending.insert(2, forward(3, 2, deadline));

        let drained = pending.drain_caller(SessionId::new(1));

        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].0, 1);
        assert_eq!(pending.len(), 1);
    }

    #[test]
    fn next_deadline_skips_canceled_entries() {
        let pending = PendingRequests::default();
        let first = Instant::now() + Duration::from_secs(1);
        let second = first + Duration::from_secs(1);
        pending.insert(1, forward(1, 2, first));
        pending.insert(2, forward(1, 2, second));
        pending.cancel(1);

        assert_eq!(pending.next_deadline(), Some(second));
    }
}
