// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Approximate-Nearest-Neighbour index façade.
//!
//! C.7b — exposes the canonical name **`AnnIndex`** (aliased
//! to [`crate::memory::vector_index::VectorIndex`]) plus a factory
//! helper so external callers stabilise on one import path.  Under
//! feature `vector-index-hnsw` a real in-memory HNSW implementation
//! (`crate::memory::hnsw::HnswMemIndex`) becomes available through the
//! same trait.

pub use crate::memory::vector_index::{
    LinearIndex, VectorBackend, VectorIndex as AnnIndex, build_backend as build_vector_backend,
};

#[cfg(feature = "vector-index-hnsw")]
pub use crate::memory::hnsw::{HnswMemIndex, HnswParams};

pub fn build_named(kind: &str) -> Box<dyn AnnIndex> {
    match kind.to_ascii_lowercase().as_str() {
        "linear" => Box::new(LinearIndex::new()),
        #[cfg(feature = "vector-index-hnsw")]
        "hnsw" => Box::new(HnswMemIndex::new()),
        other => {
            if let Some(b) = VectorBackend::from_str_lenient(other) {
                build_vector_backend(b)
            } else {
                Box::new(LinearIndex::new())
            }
        }
    }
}
