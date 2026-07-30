// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[cfg(feature = "crdt-coordination")]
pub mod crdt;

#[cfg(feature = "crdt-coordination")]
pub mod session_store;

#[cfg(feature = "crdt-coordination")]
pub use crdt::{CrdtError, CrdtUpdate, Document};

#[cfg(feature = "crdt-coordination")]
pub use session_store::{
    coordination_identity, flush_path, invalidate, mark_needs_resync, merge_remote_for_path,
    observe_after_disk_write, pull_remote_before_edit,
};
