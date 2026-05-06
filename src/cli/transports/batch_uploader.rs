// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Ordered batch POST uploader with backoff and backpressure.
//!
//! Collects events into batches and posts them in order.
//! Failed batches are retried with exponential backoff.

use anyhow::Result;
use reqwest;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub struct BatchUploaderConfig {
    pub url: String,
    pub auth_headers: Vec<(String, String)>,
    pub max_batch_size: usize,
    pub flush_interval_ms: u64,
    pub max_retries: u32,
    pub initial_backoff_ms: u64,
    pub max_backoff_ms: u64,
}

impl Default for BatchUploaderConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            auth_headers: Vec::new(),
            max_batch_size: 50,
            flush_interval_ms: 100,
            max_retries: 5,
            initial_backoff_ms: 500,
            max_backoff_ms: 30_000,
        }
    }
}

pub struct BatchUploader {
    config: BatchUploaderConfig,
    sender: mpsc::Sender<String>,
    events_sent: AtomicU64,
    events_dropped: AtomicU64,
}

impl BatchUploader {

    pub fn new(config: BatchUploaderConfig) -> Self {
        let (tx, mut rx) = mpsc::channel::<String>(1024);
        let url = config.url.clone();
        let max_batch = config.max_batch_size;
        let flush_ms = config.flush_interval_ms;

        crate::runtime::spawn_supervised("cli.transports.batch_uploader.flush", async move {
            let mut batch: Vec<String> = Vec::with_capacity(max_batch);
            let mut interval = tokio::time::interval(Duration::from_millis(flush_ms));

            loop {
                tokio::select! {
                    maybe_event = rx.recv() => {
                        match maybe_event {
                            Some(event) => {
                                batch.push(event);
                                if batch.len() >= max_batch {
                                    flush_batch(&url, &mut batch).await;
                                }
                            }
                            None => {
                                if !batch.is_empty() {
                                    flush_batch(&url, &mut batch).await;
                                }
                                break;
                            }
                        }
                    }
                    _ = interval.tick() => {
                        if !batch.is_empty() {
                            flush_batch(&url, &mut batch).await;
                        }
                    }
                }
            }
        });

        Self {
            config,
            sender: tx,
            events_sent: AtomicU64::new(0),
            events_dropped: AtomicU64::new(0),
        }
    }

    pub async fn enqueue(&self, event: String) -> Result<()> {
        self.sender
            .send(event)
            .await
            .map_err(|_| anyhow::anyhow!("Batch uploader channel closed"))?;
        self.events_sent.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn events_sent(&self) -> u64 {
        self.events_sent.load(Ordering::Relaxed)
    }

    pub fn events_dropped(&self) -> u64 {
        self.events_dropped.load(Ordering::Relaxed)
    }

    pub fn config(&self) -> &BatchUploaderConfig {
        &self.config
    }
}

async fn flush_batch(url: &str, batch: &mut Vec<String>) {
    if url.is_empty() {
        batch.clear();
        return;
    }
    let payload = batch.drain(..).collect::<Vec<_>>();
    let body = format!("[{}]", payload.join(","));

    let client = reqwest::Client::new();
    let mut retries = 0u32;
    let max_retries = 5u32;
    let mut backoff_ms = 500u64;

    loop {
        match client
            .post(url)
            .header("Content-Type", "application/json")
            .body(body.clone())
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                tracing::debug!(count = payload.len(), "Batch uploaded successfully");
                return;
            }
            Ok(resp) => {
                tracing::warn!(
                    status = %resp.status(),
                    retry = retries,
                    "Batch upload failed with HTTP error"
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, retry = retries, "Batch upload network error");
            }
        }
        retries += 1;
        if retries > max_retries {
            tracing::error!(
                count = payload.len(),
                "Batch upload exhausted retries, dropping events"
            );
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        backoff_ms = (backoff_ms * 2).min(30_000);
    }
}
