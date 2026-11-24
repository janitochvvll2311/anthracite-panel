//! The access log: one event per request, fanned out to the current viewers.
//!
//! Each viewer has a bounded queue. A slow consumer is not allowed to stall a
//! request: the event is dropped and counted, and `status.dropped` reports how
//! many were lost. The engine frames each chunk as one SSE event.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, error::TrySendError, Receiver};

/// One request, as seen by the proxy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct AccessEvent {
    pub at: u64,
    pub host: String,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub bytes: u64,
    pub duration_ms: u64,
    pub upstream: String,
}

impl AccessEvent {
    fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_else(|_| b"{}".to_vec())
    }
}

pub struct LogHub {
    subscribers: Mutex<Vec<mpsc::Sender<Vec<u8>>>>,
    capacity: usize,
    dropped: AtomicU64,
}

impl LogHub {
    pub fn new(capacity: usize) -> Self {
        Self {
            subscribers: Mutex::new(Vec::new()),
            capacity: capacity.max(1),
            dropped: AtomicU64::new(0),
        }
    }

    pub fn subscribe(&self) -> Receiver<Vec<u8>> {
        let (sender, receiver) = mpsc::channel(self.capacity);
        let mut subscribers = self.subscribers.lock().expect("log subscribers");
        subscribers.retain(|sender| !sender.is_closed());
        subscribers.push(sender);
        receiver
    }

    pub fn record(&self, event: &AccessEvent) {
        let bytes = event.encode();
        let mut subscribers = self.subscribers.lock().expect("log subscribers");
        subscribers.retain(|sender| !sender.is_closed());
        for sender in subscribers.iter() {
            match sender.try_send(bytes.clone()) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) | Err(TrySendError::Closed(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    pub fn subscribers(&self) -> usize {
        self.subscribers.lock().expect("log subscribers").len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(path: &str) -> AccessEvent {
        AccessEvent {
            at: 1,
            host: "example.com".to_string(),
            method: "GET".to_string(),
            path: path.to_string(),
            status: 200,
            bytes: 3,
            duration_ms: 1,
            upstream: "app".to_string(),
        }
    }

    #[test]
    fn an_event_is_one_json_object() {
        let hub = LogHub::new(4);
        let mut receiver = hub.subscribe();
        hub.record(&event("/one"));
        let chunk = receiver.try_recv().expect("event");
        let parsed: AccessEvent = serde_json::from_slice(&chunk).expect("json");
        assert_eq!(parsed.path, "/one");
        assert_eq!(hub.dropped(), 0);
    }

    #[test]
    fn a_slow_consumer_drops_and_counts() {
        let hub = LogHub::new(1);
        let _receiver = hub.subscribe();
        hub.record(&event("/one"));
        hub.record(&event("/two"));
        hub.record(&event("/three"));
        assert_eq!(hub.dropped(), 2);
    }

    #[test]
    fn a_closed_viewer_is_pruned() {
        let hub = LogHub::new(1);
        let receiver = hub.subscribe();
        assert_eq!(hub.subscribers(), 1);
        drop(receiver);
        hub.record(&event("/one"));
        assert_eq!(hub.subscribers(), 0);
    }
}
