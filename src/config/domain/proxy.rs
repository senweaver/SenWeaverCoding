// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! Proxy configuration domain — canonical definitions live in `schema.rs`.
//!
//! This module exists as a namespace marker for `domain::proxy::*` access patterns.
//! All canonical proxy types, impls, and helpers are defined in `schema.rs`.

pub use crate::config::schema::{ProxyConfig, ProxyScope};
