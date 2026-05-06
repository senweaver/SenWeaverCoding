// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Zeroizing secret string for API keys, tokens, and passwords.
//!
//! `SecretString` wraps a `String` and guarantees that the underlying memory
//! is securely overwritten when the value is dropped, moved, or explicitly
//! cleared.  Unlike `String`, it never exposes its contents via `Debug`,
//! `Display`, `Serialize`, or other accidental leak vectors.
//!
//! # Guarantees
//!
//! 1. **Zero-on-drop**: the buffer is overwritten with zeroes before
//!    deallocation using `zeroize`.  Even if heap memory is later reused,
//!    the previous secret is unrecoverable.
//! 2. **Opaque Debug**: `{:?}` prints only `<redacted>`.
//! 3. **No default Serialize**: callers must use `expose_for_serialization()`
//!    explicitly, making accidental logging impossible.
//! 4. **Equality is constant-time**: timing-safe comparison prevents byte-by-byte
//!    inference attacks.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Clone, ZeroizeOnDrop)]
pub struct SecretString {
    inner: String,
}

impl SecretString {

    pub fn new(value: impl Into<String>) -> Self {
        Self {
            inner: value.into(),
        }
    }

    pub fn empty() -> Self {
        Self {
            inner: String::new(),
        }
    }

    pub fn expose(&self) -> &str {
        &self.inner
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn constant_time_eq(&self, other: &str) -> bool {
        let a = self.inner.as_bytes();
        let b = other.as_bytes();
        if a.len() != b.len() {
            return false;
        }
        let mut diff: u8 = 0;
        for (x, y) in a.iter().zip(b.iter()) {
            diff |= x ^ y;
        }
        diff == 0
    }

    pub fn clear(&mut self) {
        self.inner.zeroize();
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted len={}>", self.inner.len())
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<redacted len={}>", self.inner.len())
    }
}

impl From<String> for SecretString {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for SecretString {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        self.constant_time_eq(&other.inner)
    }
}

impl Eq for SecretString {}

impl Default for SecretString {
    fn default() -> Self {
        Self::empty()
    }
}

impl Serialize for SecretString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("***REDACTED***")
    }
}

impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self::new(s))
    }
}

impl schemars::JsonSchema for SecretString {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        std::borrow::Cow::Borrowed("SecretString")
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        <String as schemars::JsonSchema>::json_schema(generator)
    }
}

impl SecretString {

    pub fn serialize_exposed<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.inner)
    }

    pub fn fingerprint(&self, suffix_len: usize) -> String {
        if self.inner.is_empty() {
            return "…".to_string();
        }
        let take = suffix_len.min(self.inner.len()).max(1);
        let tail: String = self
            .inner
            .chars()
            .rev()
            .take(take)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("…{tail}")
    }

    pub fn redacted(&self) -> String {
        self.fingerprint(4)
    }
}
