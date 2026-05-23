// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn fingerprint(
    provider: &str,
    model: &str,
    messages_canonical: &str,
    tools_canonical: &str,
) -> IdempotencyKey {
    let mut hasher = Sha256::new();
    hasher.update(provider.as_bytes());
    hasher.update(b"\0");
    hasher.update(model.as_bytes());
    hasher.update(b"\0");
    hasher.update(messages_canonical.as_bytes());
    hasher.update(b"\0");
    hasher.update(tools_canonical.as_bytes());
    IdempotencyKey(format!("{:x}", hasher.finalize()))
}

pub fn fingerprint_json(
    provider: &str,
    model: &str,
    messages: &serde_json::Value,
    tools: &serde_json::Value,
) -> IdempotencyKey {
    let m = canonical_json(messages);
    let t = canonical_json(tools);
    fingerprint(provider, model, &m, &t)
}

fn canonical_json(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let parts: Vec<String> = entries
                .into_iter()
                .map(|(k, val)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(k).unwrap_or_default(),
                        canonical_json(val)
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        serde_json::Value::Array(arr) => {
            let parts: Vec<String> = arr.iter().map(canonical_json).collect();
            format!("[{}]", parts.join(","))
        }
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}
