// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextTag {
    File(PathBuf),
    Symbol(String),
    Folder(PathBuf),
    Url(String),
    Doc(String),
    Diff(String),
    Test(String),
    Recent,
    Selection,

    Codebase(String),

    Problems,
}

impl ContextTag {

    pub fn label(&self) -> &'static str {
        match self {
            Self::File(_) => "file",
            Self::Symbol(_) => "symbol",
            Self::Folder(_) => "folder",
            Self::Url(_) => "url",
            Self::Doc(_) => "doc",
            Self::Diff(_) => "diff",
            Self::Test(_) => "test",
            Self::Recent => "recent",
            Self::Selection => "selection",
            Self::Codebase(_) => "codebase",
            Self::Problems => "problems",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextItem {
    pub tag: String,
    pub title: String,

    pub body: String,

    pub approx_tokens: usize,
    pub source: &'static str,
}

impl ContextItem {
    pub fn new(tag: impl Into<String>, title: impl Into<String>, body: impl Into<String>) -> Self {
        let body = body.into();
        let approx = crate::providers::traits::estimate_content_tokens(&body);
        Self {
            tag: tag.into(),
            title: title.into(),
            body,
            approx_tokens: approx,
            source: "",
        }
    }

    pub fn with_source(mut self, source: &'static str) -> Self {
        self.source = source;
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ContextResolveError {
    #[error("tag '{tag}' not found: {reason}")]
    NotFound { tag: String, reason: String },
    #[error("budget exhausted (need {want} tokens, have {have})")]
    BudgetExhausted { want: usize, have: usize },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("unsupported tag '{0}'")]
    Unsupported(String),
}
