// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub use crate::memory::vector::index::{
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
