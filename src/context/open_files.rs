// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! open-file context source.
//!
//! The agent needs a "what is the user currently looking at" signal so
//! the [`crate::context::builder::ContextBuilder`] can attach relevant
//! file blocks to the query.  The concrete source varies: GUI editor
//! tabs, the LSP list of opened documents, or an `inline_completion`
//! recent-files tracker.  To keep the builder free of UI-specific
//! dependencies the source is abstracted behind a trait, and the
//! builder simply holds an `Arc<dyn OpenFilesSource>`.
//!
//! # Example
//! ```no_run
//! use std::sync::Arc;
//! use senagentos_cli::context::open_files::{OpenFile, OpenFilesSource};
//! use async_trait::async_trait;
//!
//! struct Empty;
//! #[async_trait]
//! impl OpenFilesSource for Empty {
//!     async fn list(&self) -> Vec<OpenFile> { Vec::new() }
//! }
//! ```

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
