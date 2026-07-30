// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

use crate::runtime::TaskHandle;

const MAX_DEBOUNCE_MESSAGES: usize = 64;
const MAX_WINDOW_MULTIPLIER: u32 = 5;

pub enum DebounceResult {

    Aggregated(tokio::sync::oneshot::Receiver<String>),

    Coalesced(tokio::sync::oneshot::Receiver<String>),

    Passthrough(String),
}

struct DebouncerEntry {
    messages: Vec<String>,
    first_at: Instant,
    timer_handle: Arc<Mutex<Option<TaskHandle>>>,

    result_txs: Vec<tokio::sync::oneshot::Sender<String>>,
}

impl DebouncerEntry {
    fn flush(self) {
        let combined = self.messages.join("\n");
        for tx in self.result_txs {
            let _ = tx.send(combined.clone());
        }
    }
}

pub struct MessageDebouncer {
    window: Duration,
    entries: Arc<Mutex<HashMap<String, DebouncerEntry>>>,
}

impl MessageDebouncer {

    pub fn new(window: Duration) -> Self {
        Self {
            window,
            entries: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn enabled(&self) -> bool {
        !self.window.is_zero()
    }

    pub async fn debounce(&self, sender_key: &str, message: &str) -> DebounceResult {
        if !self.enabled() {
            return DebounceResult::Passthrough(message.to_owned());
        }

        let mut entries = self.entries.lock().await;
        let key = sender_key.to_owned();

        if let Some(entry) = entries.get_mut(&key) {
            if let Some(h) = entry.timer_handle.lock().await.take() {
                h.abort();
            }
            entry.messages.push(message.to_owned());

            let (tx, rx) = tokio::sync::oneshot::channel();
            entry.result_txs.push(tx);

            let max_age = self.window.saturating_mul(MAX_WINDOW_MULTIPLIER);
            if entry.messages.len() >= MAX_DEBOUNCE_MESSAGES || entry.first_at.elapsed() >= max_age
            {
                if let Some(entry) = entries.remove(&key) {
                    entry.flush();
                }
                return DebounceResult::Coalesced(rx);
            }

            let key_clone = key.clone();
            let entries_ref = Arc::clone(&self.entries);
            let window = self.window;
            let handle = crate::runtime::spawn_supervised("channels.debounce.timer", async move {
                tokio::time::sleep(window).await;
                fire_debounced(&entries_ref, &key_clone).await;
            });
            *entry.timer_handle.lock().await = Some(handle);

            DebounceResult::Coalesced(rx)
        } else {
            let (tx, rx) = tokio::sync::oneshot::channel();

            let key_clone = key.clone();
            let entries_spawn = Arc::clone(&self.entries);
            let window = self.window;
            let handle = crate::runtime::spawn_supervised("channels.debounce.timer", async move {
                tokio::time::sleep(window).await;
                fire_debounced(&entries_spawn, &key_clone).await;
            });

            entries.insert(
                key,
                DebouncerEntry {
                    messages: vec![message.to_owned()],
                    first_at: Instant::now(),
                    timer_handle: Arc::new(Mutex::new(Some(handle))),
                    result_txs: vec![tx],
                },
            );

            DebounceResult::Aggregated(rx)
        }
    }
}

async fn fire_debounced(entries: &Mutex<HashMap<String, DebouncerEntry>>, key: &str) {
    let mut map = entries.lock().await;
    if let Some(entry) = map.remove(key) {
        entry.flush();
    }
}
