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

/// A per-session live feed of the currently-running turn's outbound wire frames.
///
/// A desktop turn runs inside the connection task that started it, but the task
/// (and the turn) outlives a dropped socket. This feed lets a reconnecting or a
/// second client attach to the in-flight turn: the turn tees every wire frame it
/// emits into `tx`, and a newly connected socket subscribes to receive all frames
/// emitted from the attach point forward. Because a broadcast subscriber only
/// observes messages sent after it subscribes, attach frames never overlap with
/// the committed history a client reloads on connect, so there is no double-apply.
///
/// The feed also carries the running turn's cancel handles so that a
/// `stop_generation` arriving on a *different* connection than the one that
/// started the turn can still stop it.
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

    /// Publish one outbound wire frame to any attached subscribers. Never blocks
    /// and never errors when there are no subscribers.
    pub fn publish(&self, frame: &str) {
        let _ = self.tx.send(frame.to_string());
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    /// Whether any other connection is currently mirroring this session's live
    /// stream. Lets the hot path skip serializing per-token deltas for the feed
    /// when only the originating connection is attached.
    #[must_use]
    pub fn has_subscribers(&self) -> bool {
        self.tx.receiver_count() > 0
    }

    /// The current turn's cancellation token. Child work (e.g. delegated
    /// subagents) can derive a child token from this so a turn/session cancel
    /// propagates down and never leaves orphaned in-flight subagents.
    #[must_use]
    pub fn current_cancel_token(&self) -> CancellationToken {
        self.cancel_signal.load_full().as_ref().clone()
    }

    /// Signal the running turn to cancel (cross-connection stop). Mirrors what a
    /// reader's `stop_generation` does to its own agent, but targets the turn that
    /// actually owns this session right now.
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

/// Register the feed for a session's turn. Returns the feed so the caller can tee
/// frames into it. Overwrites any stale entry for the same session id.
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

/// RAII guard that removes a session's turn feed when the turn scope exits, so a
/// panicking or early-returning turn cannot leak a stale feed.
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
