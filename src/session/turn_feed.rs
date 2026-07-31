// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use arc_swap::ArcSwap;
use parking_lot::{Mutex, RwLock};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

const TURN_FEED_BROADCAST_CAPACITY: usize = 512;
const TURN_FEED_HISTORY_MAX_FRAMES: usize = 2048;
const TURN_FEED_HISTORY_MAX_BYTES: usize = 2 * 1024 * 1024;

struct FeedHistory {
    frames: VecDeque<(u64, Arc<str>)>,
    bytes: usize,
    next_index: u64,
}

pub struct SessionTurnFeed {
    tx: broadcast::Sender<(u64, Arc<str>)>,
    history: Mutex<FeedHistory>,
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
            history: Mutex::new(FeedHistory {
                frames: VecDeque::new(),
                bytes: 0,
                next_index: 0,
            }),
            cancelled,
            cancel_signal,
        }
    }

    pub fn publish(&self, frame: &str) {
        let shared: Arc<str> = Arc::from(frame);
        let indexed = {
            let mut history = self.history.lock();
            let index = history.next_index;
            history.next_index += 1;
            history.frames.push_back((index, Arc::clone(&shared)));
            history.bytes += shared.len();
            while history.frames.len() > TURN_FEED_HISTORY_MAX_FRAMES
                || history.bytes > TURN_FEED_HISTORY_MAX_BYTES
            {
                match history.frames.pop_front() {
                    Some((_, old)) => history.bytes = history.bytes.saturating_sub(old.len()),
                    None => break,
                }
            }
            (index, shared)
        };
        let _ = self.tx.send(indexed);
    }

    #[must_use]
    pub fn subscribe_with_history(
        &self,
    ) -> (Vec<(u64, Arc<str>)>, broadcast::Receiver<(u64, Arc<str>)>) {
        let rx = self.tx.subscribe();
        let snapshot: Vec<(u64, Arc<str>)> = {
            let history = self.history.lock();
            history.frames.iter().cloned().collect()
        };
        (snapshot, rx)
    }

    #[must_use]
    pub fn frames_after(&self, last_index: Option<u64>) -> Vec<(u64, Arc<str>)> {
        let history = self.history.lock();
        history
            .frames
            .iter()
            .filter(|(idx, _)| last_index.is_none_or(|last| *idx > last))
            .cloned()
            .collect()
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
    feed: Arc<SessionTurnFeed>,
}

impl TurnFeedGuard {
    #[must_use]
    pub fn new(session_id: impl Into<String>, feed: Arc<SessionTurnFeed>) -> Self {
        Self {
            session_id: session_id.into(),
            feed,
        }
    }
}

impl Drop for TurnFeedGuard {
    fn drop(&mut self) {
        let mut guard = registry().write();
        if guard
            .get(&self.session_id)
            .is_some_and(|current| Arc::ptr_eq(current, &self.feed))
        {
            guard.remove(&self.session_id);
        }
    }
}
