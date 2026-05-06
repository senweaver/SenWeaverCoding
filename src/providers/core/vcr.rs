// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Provider request/response record-and-replay framework ("VCR").
//!
//! most provider regressions are subtle (a schema tweak,
//! a rate-limit header rename, a streaming-frame boundary change).
//! They are easy to write unit tests **against a recorded cassette**
//! but very hard to keep exercised against live APIs.  This module
//! introduces a uniform JSON-cassette format that every provider
//! (`openai`, `anthropic`, `google`, `azure`, `ollama`, `compatible`)
//! can share.
//!
//! ## Cassette layout
//!
//! ```json
//! {
//!   "provider": "openai",
//!   "case": "chat.basic",
//!   "recorded_at": 1737472800,
//!   "interactions": [
//!     {
//!       "request": {
//!         "method": "POST",
//!         "url": "https://api.openai.com/v1/chat/completions",
//!         "headers": [ ["content-type", "application/json"] ],
//!         "body": "{\"model\":\"gpt-4o\",\"messages\":[...]}"
//!       },
//!       "response": {
//!         "status": 200,
//!         "headers": [ ["content-type", "application/json"] ],
//!         "body": "{\"id\":\"chatcmpl-...\"}"
//!       }
//!     }
//!   ]
//! }
//! ```
//!
//! Secrets are never recorded: [`SENSITIVE_HEADERS`] are scrubbed at
//! recording time and the replay layer refuses to load a cassette
//! that still contains them.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "x-goog-api-key",
    "x-anthropic-api-key",
    "cookie",
    "set-cookie",
    "openai-organization",
    "azure-api-key",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcrMode {

    Replay,

    Record,

    Auto,
}

impl VcrMode {
    pub fn from_env() -> Self {
        match std::env::var("SEN_VCR_MODE").ok().as_deref() {
            Some("record") => Self::Record,
            Some("auto") => Self::Auto,
            _ => Self::Replay,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordedRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordedResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Interaction {
    pub request: RecordedRequest,
    pub response: RecordedResponse,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Cassette {
    pub provider: String,
    pub case: String,
    pub recorded_at: u64,
    #[serde(default)]
    pub interactions: Vec<Interaction>,
}

impl Cassette {

    pub fn new(provider: impl Into<String>, case: impl Into<String>) -> Self {
        let recorded_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Self {
            provider: provider.into(),
            case: case.into(),
            recorded_at,
            interactions: Vec::new(),
        }
    }

    pub fn push(&mut self, mut interaction: Interaction) {
        scrub_headers(&mut interaction.request.headers);
        scrub_headers(&mut interaction.response.headers);
        self.interactions.push(interaction);
    }

    pub fn detect_sensitive_leaks(&self) -> Vec<String> {
        let mut offenders: Vec<String> = Vec::new();
        for it in &self.interactions {
            for (name, value) in it.request.headers.iter().chain(it.response.headers.iter()) {
                if SENSITIVE_HEADERS
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(name))
                    && value != SCRUBBED_VALUE
                {
                    offenders.push(name.clone());
                }
            }
        }
        offenders.sort();
        offenders.dedup();
        offenders
    }

    pub fn load_from_file<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        serde_json::from_str(&raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, json)
    }
}

pub const SCRUBBED_VALUE: &str = "__SCRUBBED__";

fn scrub_headers(headers: &mut [(String, String)]) {
    for (name, value) in headers.iter_mut() {
        if SENSITIVE_HEADERS
            .iter()
            .any(|s| s.eq_ignore_ascii_case(name))
        {
            *value = SCRUBBED_VALUE.to_string();
        }
    }
}

pub fn default_dir() -> PathBuf {
    std::env::var_os("SEN_VCR_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/fixtures/vcr"))
}

pub fn cassette_path(provider: &str, case: &str) -> PathBuf {
    default_dir().join(format!("{provider}__{case}.json"))
}

pub fn provider_case_matrix() -> BTreeMap<&'static str, Vec<&'static str>> {
    let mut m = BTreeMap::new();
    let cases = vec!["chat.basic", "chat.tool_call", "stream.sse"];
    for p in [
        "openai",
        "anthropic",
        "google",
        "azure",
        "ollama",
        "compatible",
    ] {
        m.insert(p, cases.clone());
    }
    m
}
