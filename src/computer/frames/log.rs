// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::hash;

pub const FRAME_MANIFEST_FILE: &str = "frames.json";
pub const FRAME_MANIFEST_VERSION: u32 = 1;
pub const HEARTBEAT_MS: u64 = 5_000;
pub const MAX_FRAMES: usize = 300;
const SCENE_DISTANCE: u32 = 8;
const HEARTBEAT_MIN_DISTANCE: u32 = 3;
const STATIC_HEARTBEAT_MS: u64 = 30_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameRecord {
    pub file: String,
    pub t_ms: i64,
    pub offset_ms: u64,
    pub source: String,
    pub phash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameManifest {
    pub version: u32,
    pub format: String,
    pub heartbeat_ms: u64,
    pub frames: Vec<FrameRecord>,
}

pub struct FrameLog {
    dir: PathBuf,
    started_epoch_ms: i64,
    started: std::time::Instant,
    frames: Vec<FrameRecord>,
    last_phash: Option<u64>,
    last_kept_at: Option<std::time::Instant>,
    limit_hit: bool,
    writes: tokio::task::JoinSet<()>,
}

impl FrameLog {
    pub fn new(dir: PathBuf, started_epoch_ms: i64) -> Self {
        Self {
            dir,
            started_epoch_ms,
            started: std::time::Instant::now(),
            frames: Vec::new(),
            last_phash: None,
            last_kept_at: None,
            limit_hit: false,
            writes: tokio::task::JoinSet::new(),
        }
    }

    pub fn offer(
        &mut self,
        phash: u64,
        width: u32,
        height: u32,
        jpeg: std::sync::Arc<Vec<u8>>,
    ) {
        if self.frames.len() >= MAX_FRAMES {
            self.limit_hit = true;
            return;
        }
        let now = std::time::Instant::now();
        let distance = self.last_phash.map(|prev| hash::hamming(prev, phash));
        let since_kept_ms = self
            .last_kept_at
            .map(|at| now.duration_since(at).as_millis() as u64);

        let reason = match (distance, since_kept_ms) {
            (None, _) => Some("first".to_string()),
            (Some(d), _) if d > SCENE_DISTANCE => Some(format!("scene>{SCENE_DISTANCE}")),
            (Some(d), Some(elapsed))
                if elapsed >= HEARTBEAT_MS
                    && (d >= HEARTBEAT_MIN_DISTANCE || elapsed >= STATIC_HEARTBEAT_MS) =>
            {
                Some("heartbeat".to_string())
            }
            _ => None,
        };
        let Some(reason) = reason else {
            return;
        };

        let offset_ms = now.duration_since(self.started).as_millis() as u64;
        let file = format!("frame_{offset_ms:08}.jpg");
        let source = if reason.starts_with("scene") || reason == "first" {
            "scene"
        } else {
            "heartbeat"
        };
        let record = FrameRecord {
            file: file.clone(),
            t_ms: self.started_epoch_ms + offset_ms as i64,
            offset_ms,
            source: source.to_string(),
            phash: hash::phash_hex(phash),
            reason: Some(reason),
            width,
            height,
        };
        self.frames.push(record);
        self.last_phash = Some(phash);
        self.last_kept_at = Some(now);

        let path = self.dir.join("frames").join(&file);
        self.writes.spawn(async move {
            let _ = tokio::fs::write(path, jpeg.as_slice()).await;
        });
    }

    pub async fn finish(mut self) {
        while self.writes.join_next().await.is_some() {}
        if self.frames.is_empty() {
            return;
        }
        let manifest = FrameManifest {
            version: FRAME_MANIFEST_VERSION,
            format: "jpeg".to_string(),
            heartbeat_ms: HEARTBEAT_MS,
            frames: std::mem::take(&mut self.frames),
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&manifest) {
            let _ = tokio::fs::write(self.dir.join(FRAME_MANIFEST_FILE), bytes).await;
        }
        if self.limit_hit {
            tracing::debug!("frame timeline reached the {MAX_FRAMES}-frame cap");
        }
    }
}

pub fn load_manifest(dir: &std::path::Path) -> Option<FrameManifest> {
    let content = std::fs::read_to_string(dir.join(FRAME_MANIFEST_FILE)).ok()?;
    serde_json::from_str(&content).ok()
}
