// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::traits::{Channel, ChannelMessage, SendMessage};
use async_trait::async_trait;

pub struct WeComChannel {
    webhook_key: String,
    allowed_users: Vec<String>,
}

impl WeComChannel {
    pub fn new(webhook_key: String, allowed_users: Vec<String>) -> Self {
        Self {
            webhook_key,
            allowed_users,
        }
    }

    fn http_client(&self) -> reqwest::Client {
        crate::services::get_services()
            .proxy_runtime()
            .build_client("channel.wecom")
    }

    fn webhook_url(&self) -> String {
        format!(
            "https://qyapi.weixin.qq.com/cgi-bin/webhook/send?key={}",
            self.webhook_key
        )
    }

    fn is_user_allowed(&self, user_id: &str) -> bool {
        self.allowed_users.iter().any(|u| u == "*" || u == user_id)
    }
}

#[async_trait]
impl Channel for WeComChannel {
    fn name(&self) -> &str {
        "wecom"
    }

    async fn send(&self, message: &SendMessage) -> anyhow::Result<()> {
        let body = serde_json::json!({
            "msgtype": "text",
            "text": {
                "content": message.content,
            }
        });

        let resp = self
            .http_client()
            .post(self.webhook_url())
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            anyhow::bail!("WeCom webhook send failed ({status}): {err}");
        }

        let result: serde_json::Value = resp.json().await?;
        let errcode = result.get("errcode").and_then(|v| v.as_i64()).unwrap_or(-1);
        if errcode != 0 {
            let errmsg = result
                .get("errmsg")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            anyhow::bail!("WeCom API error (errcode={errcode}): {errmsg}");
        }

        Ok(())
    }

    async fn listen(&self, tx: tokio::sync::mpsc::Sender<ChannelMessage>) -> anyhow::Result<()> {

        tracing::info!("WeCom: channel ready (send-only via Bot Webhook)");
        tx.closed().await;
        Ok(())
    }

    async fn health_check(&self) -> bool {

        let resp = self
            .http_client()
            .post(self.webhook_url())
            .json(&serde_json::json!({
                "msgtype": "text",
                "text": {
                    "content": "health_check"
                }
            }))
            .send()
            .await;

        match resp {
            Ok(r) => r.status().is_success(),
            Err(_) => false,
        }
    }
}
