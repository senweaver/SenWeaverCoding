// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Stateless wiring helpers extracted from [`crate::agent::loop_::run`].
//!
//! Each helper is intentionally narrow: it takes a `&Config` slice and
//! returns an owned `Arc<dyn Trait>`.  No mutable state, no async
//! (besides what the constructors demand), no implicit global
//! registrations beyond what the helper documents.

use std::sync::Arc;

use anyhow::Result;

use crate::config::Config;
use crate::memory::{self, Memory};
use crate::observability::{self, traits::Observer};
use crate::runtime;
use crate::security::SecurityPolicy;

pub fn build_observer(config: &Config) -> Arc<dyn Observer> {
    let base = observability::create_observer(&config.observability);
    Arc::from(base)
}

pub fn build_runtime(config: &Config) -> Result<Arc<dyn runtime::RuntimeAdapter>> {
    Ok(Arc::from(runtime::create_runtime(&config.runtime)?))
}

pub fn build_security(config: &Config) -> Arc<SecurityPolicy> {
    Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ))
}

pub fn build_memory(config: &Config) -> Result<Arc<dyn Memory>> {
    let mem = memory::create_memory_with_storage_and_routes(
        &config.memory,
        &config.embedding_routes,
        Some(&config.storage.provider.config),
        &config.workspace_dir,
        config.api_key.as_deref(),
    )?;
    Ok(Arc::from(mem))
}
