use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use super::router::JsonRpcResponse;

#[derive(Debug, Default)]
pub struct PendingRequests {
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, Sender<JsonRpcResponse>>>,
}

impl PendingRequests {
    pub fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn insert(&self, id: u64) -> Receiver<JsonRpcResponse> {
        let (sender, receiver) = channel();
        self.pending
            .lock()
            .expect("pending requests mutex poisoned")
            .insert(id, sender);
        receiver
    }

    pub fn deliver(&self, id: u64, response: JsonRpcResponse) -> bool {
        let sender = self
            .pending
            .lock()
            .expect("pending requests mutex poisoned")
            .remove(&id);

        sender
            .map(|sender| sender.send(response).is_ok())
            .unwrap_or(false)
    }

    pub fn cancel(&self, id: u64) {
        self.pending
            .lock()
            .expect("pending requests mutex poisoned")
            .remove(&id);
    }

    pub fn len(&self) -> usize {
        self.pending
            .lock()
            .expect("pending requests mutex poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending
            .lock()
            .expect("pending requests mutex poisoned")
            .is_empty()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::runtime::transport::router::JsonRpcResponse;

    #[test]
    fn insert_and_deliver_correlates_response() {
        let pending = PendingRequests::default();
        let receiver = pending.insert(42);
        let response = JsonRpcResponse::success(Value::from("ok"), Value::from(42));

        assert!(pending.deliver(42, response.clone()));
        assert_eq!(receiver.recv().expect("response"), response);
        assert!(pending.is_empty());
    }

    #[test]
    fn cancel_removes_pending_request() {
        let pending = PendingRequests::default();
        let _receiver = pending.insert(42);

        pending.cancel(42);

        assert_eq!(pending.len(), 0);
    }
}
