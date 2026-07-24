// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwap;
use parking_lot::RwLock;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

const TURN_FEED_BROADCAST_CAPACITY: usize = 512;

pub struct SessionTurnFeed {
    tx: broadcast::Sender<String>,
    cancelled: Arc<AtomicBool>,
    cancel_signal: Arc<ArcSwap<CancellationToken>>,
}

impl SessionTurnFeed {
    fn new(
        cancelled: Arc<AtomicBool>,
        cancel_signal: Arc<ArcSwap<CancellationToken>>,
    ) -> Self {
        let (tx, _rx) = broadcast::channel(TURN_FEED_BROADCAST_CAPACITY);
        Self {
            tx,
            cancelled,
            cancel_signal,
        }
    }

    pub fn publish(&self, frame: &str) {
        let _ = self.tx.send(frame.to_string());
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    #[must_use]
    pub fn has_subscribers(&self) -> bool {
        self.tx.receiver_count() > 0
    }

    #[must_use]
    pub fn current_cancel_token(&self) -> CancellationToken {
        self.cancel_signal.load_full().as_ref().clone()
    }

    pub fn request_cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.cancel_signal.load_full().cancel();
    }
}

type FeedMap = RwLock<HashMap<String, Arc<SessionTurnFeed>>>;

fn registry() -> &'static FeedMap {
    static FEEDS: OnceLock<FeedMap> = OnceLock::new();
    FEEDS.get_or_init(|| RwLock::new(HashMap::new()))
}

#[must_use]
pub fn register_turn_feed(
    session_id: &str,
    cancelled: Arc<AtomicBool>,
    cancel_signal: Arc<ArcSwap<CancellationToken>>,
) -> Arc<SessionTurnFeed> {
    let feed = Arc::new(SessionTurnFeed::new(cancelled, cancel_signal));
    registry()
        .write()
        .insert(session_id.to_string(), Arc::clone(&feed));
    feed
}

#[must_use]
pub fn get_turn_feed(session_id: &str) -> Option<Arc<SessionTurnFeed>> {
    registry().read().get(session_id).cloned()
}

pub fn deregister_turn_feed(session_id: &str) {
    registry().write().remove(session_id);
}

pub struct TurnFeedGuard {
    session_id: String,
}

impl TurnFeedGuard {
    #[must_use]
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
        }
    }
}

impl Drop for TurnFeedGuard {
    fn drop(&mut self) {
        deregister_turn_feed(&self.session_id);
    }
}
