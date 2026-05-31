// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub id: String,
    pub sender: String,
    pub reply_target: String,
    pub content: String,
    pub channel: String,
    pub timestamp: u64,

    pub thread_ts: Option<String>,

    pub interruption_scope_id: Option<String>,

    pub attachments: Vec<super::pipeline::media::MediaAttachment>,
}

#[derive(Debug, Clone)]
pub struct SendMessage {
    pub content: String,
    pub recipient: String,
    pub subject: Option<String>,

    pub thread_ts: Option<String>,

    pub cancellation_token: Option<CancellationToken>,
}

impl SendMessage {

    pub fn new(content: impl Into<String>, recipient: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            recipient: recipient.into(),
            subject: None,
            thread_ts: None,
            cancellation_token: None,
        }
    }

    pub fn with_subject(
        content: impl Into<String>,
        recipient: impl Into<String>,
        subject: impl Into<String>,
    ) -> Self {
        Self {
            content: content.into(),
            recipient: recipient.into(),
            subject: Some(subject.into()),
            thread_ts: None,
            cancellation_token: None,
        }
    }

    pub fn in_thread(mut self, thread_ts: Option<String>) -> Self {
        self.thread_ts = thread_ts;
        self
    }

    pub fn with_cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation_token = Some(token);
        self
    }
}

#[async_trait]
pub trait Channel: Send + Sync {

    fn name(&self) -> &str;

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()>;

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()>;

    async fn health_check(&self) -> bool {
        true
    }

    async fn start_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn stop_typing(&self, _recipient: &str) -> anyhow::Result<()> {
        Ok(())
    }

    fn supports_draft_updates(&self) -> bool {
        false
    }

    fn supports_multi_message_streaming(&self) -> bool {
        false
    }

    fn multi_message_delay_ms(&self) -> u64 {
        800
    }

    async fn send_draft(&self, _message: &SendMessage) -> anyhow::Result<Option<String>> {
        Ok(None)
    }

    async fn update_draft(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn update_draft_progress(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn finalize_draft(
        &self,
        _recipient: &str,
        _message_id: &str,
        _text: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn cancel_draft(&self, _recipient: &str, _message_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn add_reaction(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn remove_reaction(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _emoji: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    async fn pin_message(&self, _channel_id: &str, _message_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn unpin_message(&self, _channel_id: &str, _message_id: &str) -> anyhow::Result<()> {
        Ok(())
    }

    async fn redact_message(
        &self,
        _channel_id: &str,
        _message_id: &str,
        _reason: Option<String>,
    ) -> anyhow::Result<()> {
        Ok(())
    }
}
