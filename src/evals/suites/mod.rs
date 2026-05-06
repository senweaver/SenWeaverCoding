// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Built-in eval suites.
//!
//! Each suite exposes a small demo fixture inline so smoke tests
//! pass without downloading gigabytes of dataset.  The full fixture
//! loaders are gated behind the `evals-fixtures` feature (reserved
//! for a follow-up sprint).

pub mod humaneval;
pub mod mbpp;
pub mod swebench;
