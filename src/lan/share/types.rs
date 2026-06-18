// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareWire {
    pub id: String,
    pub name: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
    pub size: i64,
    #[serde(default)]
    pub note: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MyShareView {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
    pub size: i64,
    pub note: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ShareView {
    pub id: String,
    #[serde(rename = "ownerId")]
    pub owner_id: String,
    #[serde(rename = "ownerNickname")]
    pub owner_nickname: String,
    pub name: String,
    #[serde(rename = "isDir")]
    pub is_dir: bool,
    pub size: i64,
    pub note: String,
    pub online: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

pub enum ShareInbound {
    ListRequest,
    ListResponse { shares: Vec<ShareWire> },
    DownloadRequest { share_id: String },
}
