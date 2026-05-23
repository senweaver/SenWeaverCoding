// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct OpenFile {
    pub path: PathBuf,
    pub language: Option<String>,
    pub content_ref: Option<Arc<String>>,

    pub cursor_pos: Option<(u32, u32)>,
}

impl OpenFile {

    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            language: None,
            content_ref: None,
            cursor_pos: None,
        }
    }
}

#[async_trait]
pub trait OpenFilesSource: Send + Sync {
    async fn list(&self) -> Vec<OpenFile>;
}

#[derive(Debug, Default, Clone)]
pub struct NoOpenFilesSource;

#[async_trait]
impl OpenFilesSource for NoOpenFilesSource {
    async fn list(&self) -> Vec<OpenFile> {
        Vec::new()
    }
}
