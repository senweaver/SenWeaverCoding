// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

const TWITTER_API_BASE: &str = "https://api.x.com/2";

pub struct TwitterChannel {
    bearer_token: String,
    allowed_users: Vec<String>,

    dedup: Arc<RwLock<HashSet<String>>>,
}

const DEDUP_CAPACITY: usize = 10_000;

impl TwitterChannel {
    pub fn new(bearer_token: String, allowed_users: Vec<String>) -> Self {
        Self {
            bearer_token,
            allowed_users,
            dedup: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    fn http_client(&self) -> reqwest::Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client("channel.twitter")
    }

    fn is_user_allowed(&self, user_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == user_id)
    }

    async fn is_duplicate(&self, tweet_id: &str) -> bool {
        if tweet_id.is_empty() {
            return false;
        }

        let mut dedup = self.dedup.write().await;

        if dedup.contains(tweet_id) {
            return true;
        }

        if dedup.len() >= DEDUP_CAPACITY {
            let to_remove: Vec<String> = dedup.iter().take(DEDUP_CAPACITY / 2).cloned().collect();
            for key in to_remove {
                dedup.remove(&key);
            }
        }

        dedup.insert(tweet_id.to_string());
        false
    }

    async fn get_authenticated_user_id(&self) -> anyhow::Result<String> {
        let resp = self
            .http_client()
            .get(format!("{TWITTER_API_BASE}/users/me"))
            .bearer_auth(&self.bearer_token)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("Twitter users/me failed ({status}): {err}");
        }

        let data: serde_json::Value = resp.json().await?;
        let user_id = data
            .get("data")
            .and_then(|d| d.get("id"))
            .and_then(|id| id.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing user id in Twitter response"))?
            .to_string();

        Ok(user_id)
    }

    async fn create_tweet(
        &self,
        text: &str,
        reply_tweet_id: Option<&str>,
    ) -> anyhow::Result<String> {
        let mut body = json!({ "text": text });

        if let Some(reply_id) = reply_tweet_id {
            body["reply"] = json!({ "in_reply_to_tweet_id": reply_id });
        }

        let resp = self
            .http_client()
            .post(format!("{TWITTER_API_BASE}/tweets"))
            .bearer_auth(&self.bearer_token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("Twitter create tweet failed ({status}): {err}");
        }

        let data: serde_json::Value = resp.json().await?;
        let tweet_id = data
            .get("data")
            .and_then(|d| d.get("id"))
            .and_then(|id| id.as_str())
            .unwrap_or("")
            .to_string();

        Ok(tweet_id)
    }

    async fn send_dm(&self, recipient_id: &str, text: &str) -> anyhow::Result<()> {
        let body = json!({
            "text": text,
        });

        let resp = self
            .http_client()
            .post(format!(
                "{TWITTER_API_BASE}/dm_conversations/with/{recipient_id}/messages"
            ))
            .bearer_auth(&self.bearer_token)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("Twitter DM send failed ({status}): {err}");
        }

        Ok(())
    }
}

#[async_trait]
impl Channel for TwitterChannel {
    fn name(&self) -> &str {
        "twitter"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {

        if let Some(user_id) = message.recipient.strip_prefix("dm:") {

            self.send_dm(user_id, &message.content).await
        } else if let Some(tweet_id) = message.recipient.strip_prefix("tweet:") {

            let chunks = split_tweet_text(&message.content, 280);
            let mut reply_to = tweet_id.to_string();
            for chunk in chunks {
                reply_to = self.create_tweet(&chunk, Some(&reply_to)).await?;
            }
            Ok(())
        } else {

            let chunks = split_tweet_text(&message.content, 280);
            let mut reply_to = message.recipient.clone();
            for chunk in chunks {
                reply_to = self.create_tweet(&chunk, Some(&reply_to)).await?;
            }
            Ok(())
        }
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {
        tracing::info!("Twitter: authenticating...");
        let bot_user_id = self.get_authenticated_user_id().await?;
        tracing::info!("Twitter: authenticated as user {bot_user_id}");

        let mut since_id: Option<String> = None;
        let poll_interval = std::time::Duration::from_secs(15);

        loop {
            let mut url = format!(
                "{TWITTER_API_BASE}/users/{bot_user_id}/mentions?tweet.fields=author_id,conversation_id,created_at&expansions=author_id&max_results=20"
            );

            if let Some(ref id) = since_id {
                use std::fmt::Write;
                let _ = write!(url, "&since_id={id}");
            }

            match self
                .http_client()
                .get(&url)
                .bearer_auth(&self.bearer_token)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let data: serde_json::Value = match resp.json().await {
                        Ok(d) => d,
                        Err(e) => {
                            tracing::warn!("Twitter: failed to parse mentions response: {e}");
                            tokio::time::sleep(poll_interval).await;
                            continue;
                        }
                    };

                    if let Some(tweets) = data.get("data").and_then(|d| d.as_array()) {

                        let user_map: std::collections::HashMap<String, String> = data
                            .get("includes")
                            .and_then(|i| i.get("users"))
                            .and_then(|u| u.as_array())
                            .map(|users| {
                                users
                                    .iter()
                                    .filter_map(|u| {
                                        let id = u.get("id")?.as_str()?.to_string();
                                        let username = u.get("username")?.as_str()?.to_string();
                                        Some((id, username))
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();

                        for tweet in tweets.iter().rev() {
                            let tweet_id = tweet.get("id").and_then(|i| i.as_str()).unwrap_or("");
                            let author_id = tweet
                                .get("author_id")
                                .and_then(|a| a.as_str())
                                .unwrap_or("");
                            let text = tweet.get("text").and_then(|t| t.as_str()).unwrap_or("");

                            if author_id == bot_user_id {
                                continue;
                            }

                            if self.is_duplicate(tweet_id).await {
                                continue;
                            }

                            let username = user_map
                                .get(author_id)
                                .cloned()
                                .unwrap_or_else(|| author_id.to_string());

                            if !self.is_user_allowed(&username) && !self.is_user_allowed(author_id)
                            {
                                tracing::debug!(
                                    "Twitter: ignoring mention from unauthorized user: {username}"
                                );
                                continue;
                            }

                            let clean_text = strip_at_mention(text, &bot_user_id);

                            if clean_text.trim().is_empty() {
                                continue;
                            }

                            let reply_target = format!("tweet:{tweet_id}");

                            let channel_msg = ChannelMessage {
                                id: Uuid::new_v4().to_string(),
                                sender: username,
                                reply_target,
                                content: clean_text,
                                channel: "twitter".to_string(),
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                thread_ts: tweet
                                    .get("conversation_id")
                                    .and_then(|c| c.as_str())
                                    .map(|s| s.to_string()),
                                interruption_scope_id: None,
                                attachments: vec![],
                            };

                            if tx.send(channel_msg).await.is_err() {
                                tracing::warn!("Twitter: message channel closed");
                                return Ok(());
                            }

                            if since_id.as_deref().map_or(true, |s| tweet_id > s) {
                                since_id = Some(tweet_id.to_string());
                            }
                        }
                    }

                    if let Some(newest) = data
                        .get("meta")
                        .and_then(|m| m.get("newest_id"))
                        .and_then(|n| n.as_str())
                    {
                        since_id = Some(newest.to_string());
                    }
                }
                Ok(resp) => {
                    let status = resp.status();
                    if status.as_u16() == 429 {

                        tracing::warn!("Twitter: rate limited, backing off 60s");
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        continue;
                    }
                    let err = resp.text().await.unwrap_or_default();
                    tracing::warn!("Twitter: mentions request failed ({status}): {err}");
                }
                Err(e) => {
                    tracing::warn!("Twitter: mentions request error: {e}");
                }
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    async fn health_check(&self) -> bool {
        self.get_authenticated_user_id().await.is_ok()
    }
}

fn strip_at_mention(text: &str, _bot_user_id: &str) -> String {

    let mut result = text;
    while let Some(rest) = result.strip_prefix('@') {

        match rest.find(char::is_whitespace) {
            Some(idx) => result = rest[idx..].trim_start(),
            None => return String::new(),
        }
    }
    result.to_string()
}

fn split_tweet_text(text: &str, max_len: usize) -> Vec<String> {
    if text.chars().count() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.chars().count() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        let char_boundary = remaining
            .char_indices()
            .take(max_len + 1)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(remaining.len());

        let split_at = remaining[..char_boundary]
            .rfind(' ')
            .unwrap_or(char_boundary);

        chunks.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start();
    }

    chunks
}
