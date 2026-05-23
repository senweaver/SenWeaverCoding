// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[cfg(feature = "crdt-coordination")]
pub mod crdt;

#[cfg(feature = "crdt-coordination")]
pub use crdt::{CrdtError, Document};
