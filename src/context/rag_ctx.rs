// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! RAG hits attached to the query context.
//!
//! The existing Tantivy search layer already exposes
//! [`crate::code_intel::search::SearchHit`]; this module keeps the
//! same shape via a type alias so the context builder can be
//! back-end-agnostic.

pub use crate::code_intel::search::SearchHit as RagHit;
