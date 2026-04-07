// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Bridge messaging — mirrors claude-code-typescript-src`bridge/bridgeMessaging.ts`.

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};

use super::types::{BridgeMessage, MessageContent, MessageSender};

/// Handles message routing between local agent and remote clients.
#[derive(Clone)]
pub struct BridgeMessaging {
    inner: Arc<RwLock<MessagingInner>>,
    tx: broadcast::Sender<BridgeMessage>,
}

struct MessagingInner {
    outbound_queue: VecDeque<BridgeMessage>,
    inbound_queue: VecDeque<BridgeMessage>,
    max_queue_size: usize,
}

impl BridgeMessaging {
    pub fn new(max_queue_size: usize) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(RwLock::new(MessagingInner {
                outbound_queue: VecDeque::new(),
                inbound_queue: VecDeque::new(),
                max_queue_size,
            })),
            tx,
        }
    }

    /// Subscribe to outbound messages (agent → remote client).
    pub fn subscribe(&self) -> broadcast::Receiver<BridgeMessage> {
        self.tx.subscribe()
    }

    /// Enqueue a message to send to remote clients.
    pub async fn send_to_remote(&self, message: BridgeMessage) {
        let _ = self.tx.send(message.clone());
        let mut inner = self.inner.write().await;
        if inner.outbound_queue.len() >= inner.max_queue_size {
            inner.outbound_queue.pop_front();
        }
        inner.outbound_queue.push_back(message);
    }

    /// Enqueue an inbound message from a remote client.
    pub async fn receive_from_remote(&self, message: BridgeMessage) {
        let mut inner = self.inner.write().await;
        if inner.inbound_queue.len() >= inner.max_queue_size {
            inner.inbound_queue.pop_front();
        }
        inner.inbound_queue.push_back(message);
    }

    /// Drain pending inbound messages.
    pub async fn drain_inbound(&self) -> Vec<BridgeMessage> {
        let mut inner = self.inner.write().await;
        inner.inbound_queue.drain(..).collect()
    }

    /// Check if there are pending inbound messages.
    pub async fn has_inbound(&self) -> bool {
        let inner = self.inner.read().await;
        !inner.inbound_queue.is_empty()
    }

    /// Create a text message from the agent.
    pub fn agent_text_message(session_id: &str, text: String) -> BridgeMessage {
        BridgeMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            sender: MessageSender::Agent,
            content: MessageContent::Text { text },
            timestamp_ms: now_ms(),
        }
    }

    /// Create a text message from the user.
    pub fn user_text_message(session_id: &str, text: String) -> BridgeMessage {
        BridgeMessage {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            sender: MessageSender::User,
            content: MessageContent::Text { text },
            timestamp_ms: now_ms(),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
