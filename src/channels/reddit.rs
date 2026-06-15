// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use anyhow::{Result, bail};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::Deserialize;
use std::time::{Duration, Instant};

pub struct RedditChannel {
    client_id: String,
    client_secret: String,
    refresh_token: String,
    username: String,
    subreddit: Option<String>,
    auth: Mutex<RedditAuth>,
}

struct RedditAuth {
    access_token: String,
    expires_at: Instant,
}

#[derive(Deserialize)]
struct RedditTokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct RedditListing {
    data: RedditListingData,
}

#[derive(Deserialize)]
struct RedditListingData {
    children: Vec<RedditChild>,
}

#[derive(Deserialize)]
struct RedditChild {
    data: RedditItemData,
}
#[derive(Deserialize)]
#[allow(dead_code)]
struct RedditItemData {
    name: Option<String>,
    author: Option<String>,
    body: Option<String>,
    subject: Option<String>,
    parent_id: Option<String>,
    link_id: Option<String>,
    subreddit: Option<String>,
    created_utc: Option<f64>,
    new: Option<bool>,
    #[serde(rename = "type")]
    message_type: Option<String>,
    context: Option<String>,
}

const REDDIT_API_BASE: &str = "https://oauth.reddit.com";
const REDDIT_TOKEN_URL: &str = "https://www.reddit.com/api/v1/access_token";
const USER_AGENT: &str = "sen:channel:v0.1.0 (by /u/sen-bot)";

const POLL_INTERVAL: Duration = Duration::from_secs(5);

impl RedditChannel {
    pub fn new(
        client_id: String,
        client_secret: String,
        refresh_token: String,
        username: String,
        subreddit: Option<String>,
    ) -> Self {
        Self {
            client_id,
            client_secret,
            refresh_token,
            username,
            subreddit,
            auth: Mutex::new(RedditAuth {
                access_token: String::new(),
                expires_at: Instant::now(),
            }),
        }
    }

    fn http_client(&self) -> reqwest::Client {
        crate::services::require_services()
            .proxy_runtime()
            .build_client("channel.reddit")
    }

    async fn refresh_access_token(&self) -> Result<()> {
        let client = self.http_client();
        let resp = client
            .post(REDDIT_TOKEN_URL)
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .header("User-Agent", USER_AGENT)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &self.refresh_token),
            ])
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response: {e}>"));
            bail!("Reddit token refresh failed ({status}): {body}");
        }

        let token_resp: RedditTokenResponse = resp.json().await?;
        let mut auth = self.auth.lock();
        auth.access_token = token_resp.access_token;
        auth.expires_at =
            Instant::now() + Duration::from_secs(token_resp.expires_in.saturating_sub(60));
        Ok(())
    }

    async fn get_access_token(&self) -> Result<String> {
        {
            let auth = self.auth.lock();
            if !auth.access_token.is_empty() && Instant::now() < auth.expires_at {
                return Ok(auth.access_token.clone());
            }
        }
        self.refresh_access_token().await?;
        let auth = self.auth.lock();
        Ok(auth.access_token.clone())
    }

    async fn fetch_inbox(&self) -> Result<Vec<RedditChild>> {
        let token = self.get_access_token().await?;
        let client = self.http_client();

        let resp = client
            .get(format!("{REDDIT_API_BASE}/message/unread"))
            .bearer_auth(&token)
            .header("User-Agent", USER_AGENT)
            .query(&[("limit", "25")])
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|e| format!("<failed to read response: {e}>"));
            tracing::warn!("Reddit inbox fetch failed ({status}): {body}");
            return Ok(Vec::new());
        }

        let listing: RedditListing = resp.json().await?;
        Ok(listing.data.children)
    }

    async fn mark_read(&self, fullnames: &[String]) -> Result<()> {
        if fullnames.is_empty() {
            return Ok(());
        }
        let token = self.get_access_token().await?;
        let client = self.http_client();

        let ids = fullnames.join(",");
        let resp = client
            .post(format!("{REDDIT_API_BASE}/api/read_message"))
            .bearer_auth(&token)
            .header("User-Agent", USER_AGENT)
            .form(&[("id", ids.as_str())])
            .send()
            .await?;

        if !resp.status().is_success() {
            tracing::warn!("Reddit mark_read failed: {}", resp.status());
        }
        Ok(())
    }

    fn parse_item(&self, item: &RedditItemData) -> Option<ChannelMessage> {
        let author = item.author.as_deref().unwrap_or("");
        let body = item.body.as_deref().unwrap_or("");
        let name = item.name.as_deref().unwrap_or("");

        if author.eq_ignore_ascii_case(&self.username) || author.is_empty() || body.is_empty() {
            return None;
        }

        if let Some(ref sub) = self.subreddit {
            if let Some(ref item_sub) = item.subreddit {
                if !item_sub.eq_ignore_ascii_case(sub) {
                    return None;
                }
            }
        }

        let reply_target =
            if item.message_type.as_deref() == Some("comment_reply") || item.parent_id.is_some() {

                item.parent_id.clone().unwrap_or_else(|| name.to_string())
            } else {

                author.to_string()
            };

        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let timestamp = item.created_utc.unwrap_or(0.0) as u64;

        Some(ChannelMessage {
            id: format!("reddit_{name}"),
            sender: author.to_string(),
            reply_target,
            content: body.to_string(),
            channel: "reddit".to_string(),
            timestamp,
            thread_ts: item.parent_id.clone(),
            interruption_scope_id: None,
            attachments: vec![],
        })
    }
}

#[async_trait]
impl Channel for RedditChannel {
    fn name(&self) -> &str {
        "reddit"
    }

    async fn send(&self, message: &SendMessage) -> Result<()> {
        let token = self.get_access_token().await?;
        let client = self.http_client();

        if message.recipient.starts_with("t1_")
            || message.recipient.starts_with("t3_")
            || message.recipient.starts_with("t4_")
        {

            let resp = client
                .post(format!("{REDDIT_API_BASE}/api/comment"))
                .bearer_auth(&token)
                .header("User-Agent", USER_AGENT)
                .form(&[
                    ("thing_id", message.recipient.as_str()),
                    ("text", &message.content),
                ])
                .send()
                .await?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp
                    .text()
                    .await
                    .unwrap_or_else(|e| format!("<failed to read response: {e}>"));
                bail!("Reddit comment reply failed ({status}): {body}");
            }
        } else {

            let subject = message
                .subject
                .as_deref()
                .unwrap_or("Message from SenWeaverCoding");
            let resp = client
                .post(format!("{REDDIT_API_BASE}/api/compose"))
                .bearer_auth(&token)
                .header("User-Agent", USER_AGENT)
                .form(&[
                    ("to", message.recipient.as_str()),
                    ("subject", subject),
                    ("text", &message.content),
                ])
                .send()
                .await?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp
                    .text()
                    .await
                    .unwrap_or_else(|e| format!("<failed to read response: {e}>"));
                bail!("Reddit DM failed ({status}): {body}");
            }
        }

        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> Result<()> {

        self.refresh_access_token().await?;

        tracing::info!(
            "Reddit channel listening as u/{} {}...",
            self.username,
            self.subreddit
                .as_ref()
                .map(|s| format!("in r/{s}"))
                .unwrap_or_default()
        );

        loop {
            tokio::time::sleep(POLL_INTERVAL).await;

            let items = match self.fetch_inbox().await {
                Ok(items) => items,
                Err(e) => {
                    tracing::warn!("Reddit poll error: {e}");
                    continue;
                }
            };

            let mut read_ids = Vec::new();
            'batch: for child in &items {
                match self.parse_item(&child.data) {
                    Some(msg) => {
                        match crate::channels::forward_channel_message("reddit", &tx, msg) {
                            crate::channels::ForwardOutcome::Delivered => {
                                if let Some(ref name) = child.data.name {
                                    read_ids.push(name.clone());
                                }
                            }
                            crate::channels::ForwardOutcome::Dropped => {
                                break 'batch;
                            }
                            crate::channels::ForwardOutcome::Closed => {
                                return Ok(());
                            }
                        }
                    }
                    None => {
                        if let Some(ref name) = child.data.name {
                            read_ids.push(name.clone());
                        }
                    }
                }
            }

            if let Err(e) = self.mark_read(&read_ids).await {
                tracing::warn!("Reddit mark_read error: {e}");
            }
        }
    }

    async fn health_check(&self) -> bool {
        self.get_access_token().await.is_ok()
    }
}
