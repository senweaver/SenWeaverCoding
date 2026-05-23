// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Result, bail};
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn sign_request(
    shared_secret: &str,
    payload: &[u8],
    timestamp: i64,
    nonce: &str,
) -> Result<String> {
    let mut mac = HmacSha256::new_from_slice(shared_secret.as_bytes())
        .map_err(|e| anyhow::anyhow!("HMAC key error: {e}"))?;
    mac.update(&timestamp.to_le_bytes());
    mac.update(nonce.as_bytes());
    mac.update(payload);
    Ok(hex::encode(mac.finalize().into_bytes()))
}

pub fn verify_request(
    shared_secret: &str,
    payload: &[u8],
    timestamp: i64,
    nonce: &str,
    signature: &str,
    max_age_secs: i64,
) -> Result<bool> {
    let now = Utc::now().timestamp();
    if (now - timestamp).abs() > max_age_secs {
        bail!("Request timestamp too old or too far in future");
    }

    let expected = sign_request(shared_secret, payload, timestamp, nonce)?;
    Ok(constant_time_eq(expected.as_bytes(), signature.as_bytes()))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

pub struct NodeTransport {
    http: reqwest::Client,
    shared_secret: String,
    max_request_age_secs: i64,
}

impl NodeTransport {
    pub fn new(shared_secret: String) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("HTTP client build"),
            shared_secret,
            max_request_age_secs: 300,
        }
    }

    pub async fn send(
        &self,
        node_address: &str,
        endpoint: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let body = serde_json::to_vec(&payload)?;
        let timestamp = Utc::now().timestamp();
        let nonce = uuid::Uuid::new_v4().to_string();
        let signature = sign_request(&self.shared_secret, &body, timestamp, &nonce)?;

        let url = format!("https://{node_address}/api/node-control/{endpoint}");
        let resp = self
            .http
            .post(&url)
            .header("X-SenWeaverCoding-Timestamp", timestamp.to_string())
            .header("X-SenWeaverCoding-Nonce", &nonce)
            .header("X-SenWeaverCoding-Signature", &signature)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?;

        if !resp.status().is_success() {
            bail!(
                "Node request failed: {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
        }

        Ok(resp.json().await?)
    }

    pub fn verify_incoming(
        &self,
        payload: &[u8],
        timestamp_header: &str,
        nonce_header: &str,
        signature_header: &str,
    ) -> Result<bool> {
        let timestamp: i64 = timestamp_header
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid timestamp header"))?;
        verify_request(
            &self.shared_secret,
            payload,
            timestamp,
            nonce_header,
            signature_header,
            self.max_request_age_secs,
        )
    }
}
