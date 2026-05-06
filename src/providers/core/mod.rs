// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Shared utilities for all providers.
//!
//! This module contains common functionality that is used across multiple provider
//! implementations, extracted to reduce duplication and ensure consistency.
//!
//! Re-exports sub-modules for ergonomic access from parent modules.

pub mod idempotency;
pub mod openai_sse;
pub mod rate_limit;
pub mod retry;
pub mod sse;
pub mod vcr;
