// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::channels::traits::{Channel, ChannelMessage, SendMessage};
use anyhow::{Context, Result};
use async_trait::async_trait;
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NostrProtocol {
    Nip04,
    Nip17,
}

#[derive(Debug, Clone)]
enum AllowList {

    Any,

    Set(Vec<PublicKey>),
}

impl AllowList {

    fn parse(raw: &[String]) -> Result<Self> {
        if raw.is_empty() {
            return Ok(Self::Set(Vec::new()));
        }
        if raw.iter().any(|p| p == "*") {
            return Ok(Self::Any);
        }
        let mut keys = Vec::with_capacity(raw.len());
        for s in raw {
            keys.push(PublicKey::parse(s).with_context(|| format!("Invalid allowed pubkey: {s}"))?);
        }
        Ok(Self::Set(keys))
    }

    fn is_allowed(&self, pubkey: &PublicKey) -> bool {
        match self {
            Self::Any => true,
            Self::Set(keys) => keys.iter().any(|k| k == pubkey),
        }
    }
}

pub struct NostrChannel {
    client: Client,
    public_key: PublicKey,
    allowed: AllowList,

    sender_protocols: Arc<RwLock<HashMap<PublicKey, NostrProtocol>>>,
}

impl NostrChannel {

    pub async fn new(
        private_key: &str,
        relays: Vec<String>,
        allowed_pubkeys: &[String],
    ) -> Result<Self> {
        let keys = Keys::parse(private_key).context("Invalid Nostr private key")?;
        let public_key = keys.public_key();
        let allowed = AllowList::parse(allowed_pubkeys)?;

        let client = Client::builder().signer(keys).build();
        for relay in &relays {
            client
                .add_relay(relay.as_str())
                .await
                .with_context(|| format!("Failed to add relay: {relay}"))?;
        }
        client.connect().await;

        Ok(Self {
            client,
            public_key,
            allowed,
            sender_protocols: Arc::new(RwLock::new(HashMap::new())),
        })
    }
}

#[async_trait]
impl Channel for NostrChannel {
    fn name(&self) -> &str {
        "nostr"
    }

    async fn send(&self, message: &SendMessage) -> Result<()> {
        let recipient =
            PublicKey::parse(&message.recipient).context("Invalid recipient Nostr public key")?;

        let protocol = {
            let map = self.sender_protocols.read().await;
            map.get(&recipient).copied().unwrap_or(NostrProtocol::Nip17)
        };

        match protocol {
            NostrProtocol::Nip17 => {

                self.client
                    .send_private_msg(recipient, &message.content, None)
                    .await
                    .context("Failed to send NIP-17 message")?;
                tracing::debug!(
                    "Sent NIP-17 message to {}",
                    recipient.to_bech32().unwrap_or_default()
                );
            }
            NostrProtocol::Nip04 => {

                let signer = self.client.signer().await.context("No signer on client")?;
                let encrypted = signer
                    .nip04_encrypt(&recipient, &message.content)
                    .await
                    .context("NIP-04 encryption failed")?;
                let builder = EventBuilder::new(Kind::EncryptedDirectMessage, encrypted)
                    .tag(Tag::public_key(recipient));
                self.client
                    .send_event_builder(builder)
                    .await
                    .context("Failed to send NIP-04 message")?;
                tracing::debug!(
                    "Sent NIP-04 message to {}",
                    recipient.to_bech32().unwrap_or_default()
                );
            }
        }

        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<()> {
        let listen_start = Timestamp::now();

        let filter = Filter::new()
            .pubkey(self.public_key)
            .kinds(vec![Kind::EncryptedDirectMessage, Kind::GiftWrap])
            .limit(10);

        self.client
            .subscribe(filter, None)
            .await
            .context("Failed to subscribe to Nostr events")?;

        tracing::info!(
            "Nostr channel listening as {}",
            self.public_key.to_bech32().unwrap_or_default()
        );

        let sender_protocols = Arc::clone(&self.sender_protocols);
        let signer = self.client.signer().await.context("No signer on client")?;

        loop {
            let notification = self
                .client
                .notifications()
                .recv()
                .await
                .context("Notification channel closed")?;

            match notification {
                RelayPoolNotification::Event { event, .. } => {
                    let result = match event.kind {
                        Kind::EncryptedDirectMessage => {

                            if event.created_at < listen_start {
                                continue;
                            }
                            if !self.allowed.is_allowed(&event.pubkey) {
                                tracing::warn!(
                                    "Nostr: ignoring NIP-04 message from unauthorized pubkey: {}",
                                    event.pubkey.to_hex()
                                );
                                continue;
                            }
                            match signer.nip04_decrypt(&event.pubkey, &event.content).await {
                                Ok(content) => {
                                    let sender = event.pubkey;
                                    sender_protocols
                                        .write()
                                        .await
                                        .insert(sender, NostrProtocol::Nip04);
                                    Some((
                                        event.id.to_hex(),
                                        sender.to_hex(),
                                        content,
                                        event.created_at.as_secs(),
                                    ))
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to decrypt NIP-04 message: {e}");
                                    None
                                }
                            }
                        }
                        Kind::GiftWrap => {

                            match self.client.unwrap_gift_wrap(&event).await {
                                Ok(unwrapped) => {
                                    let rumor = unwrapped.rumor;
                                    if rumor.created_at < listen_start {
                                        continue;
                                    }
                                    let sender = rumor.pubkey;
                                    if !self.allowed.is_allowed(&sender) {
                                        tracing::warn!(
                                            "Nostr: ignoring NIP-17 message from unauthorized pubkey: {}",
                                            sender.to_hex()
                                        );
                                        continue;
                                    }
                                    sender_protocols
                                        .write()
                                        .await
                                        .insert(sender, NostrProtocol::Nip17);
                                    Some((
                                        event.id.to_hex(),
                                        sender.to_hex(),
                                        rumor.content.clone(),
                                        rumor.created_at.as_secs(),
                                    ))
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to unwrap NIP-17 gift wrap: {e}");
                                    None
                                }
                            }
                        }
                        _ => None,
                    };

                    if let Some((id, sender_hex, content, timestamp)) = result {
                        let msg = ChannelMessage {
                            id,
                            sender: sender_hex.clone(),
                            reply_target: sender_hex,
                            content,
                            channel: "nostr".to_string(),
                            timestamp,
                            thread_ts: None,
                            interruption_scope_id: None,
                            attachments: vec![],
                        };
                        if tx.send(msg).await.is_err() {
                            tracing::info!("Nostr listener: message bus closed, stopping");
                            break;
                        }
                    }
                }
                RelayPoolNotification::Shutdown => {
                    tracing::info!("Nostr relay pool shut down");
                    break;
                }
                RelayPoolNotification::Message { .. } => {}
            }
        }

        Ok(())
    }

    async fn health_check(&self) -> bool {
        self.client
            .relays()
            .await
            .values()
            .any(|r| r.is_connected())
    }
}
