// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::runtime::TaskHandle;

pub enum DebounceResult {

    Pending(tokio::sync::oneshot::Receiver<String>),

    Passthrough(String),
}

struct DebouncerEntry {
    messages: Vec<String>,
    timer_handle: Arc<Mutex<Option<TaskHandle>>>,

    result_tx: Option<tokio::sync::oneshot::Sender<String>>,
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
            entry.result_tx = Some(tx);

            let key_clone = key.clone();
            let entries_ref = Arc::clone(&self.entries);
            let window = self.window;
            let handle = crate::runtime::spawn_supervised("channels.debounce.timer", async move {
                tokio::time::sleep(window).await;
                fire_debounced(&entries_ref, &key_clone).await;
            });
            *entry.timer_handle.lock().await = Some(handle);

            DebounceResult::Pending(rx)
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
                    timer_handle: Arc::new(Mutex::new(Some(handle))),
                    result_tx: Some(tx),
                },
            );

            DebounceResult::Pending(rx)
        }
    }
}

async fn fire_debounced(entries: &Mutex<HashMap<String, DebouncerEntry>>, key: &str) {
    let mut map = entries.lock().await;
    if let Some(entry) = map.remove(key) {
        let combined = entry.messages.join("\n");
        if let Some(tx) = entry.result_tx {
            let _ = tx.send(combined);
        }
    }
}
