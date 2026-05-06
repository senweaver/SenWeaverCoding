// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! CRDT proof-of-concept umbrella module.
//!
//! Compiled only when the `crdt-coordination` feature is on.  The
//! umbrella module is intentionally tiny so future cfg-gated
//! sub-modules (e.g. a Loro-backed binary update encoder) can be
//! added without touching `src/lib.rs` again.

#[cfg(feature = "crdt-coordination")]
pub mod crdt;

#[cfg(feature = "crdt-coordination")]
pub use crdt::{CrdtError, Document};
