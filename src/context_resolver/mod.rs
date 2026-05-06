// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Unified `@` context resolver.
//!
//! Every chat surface (CLI / TUI / GUI) accepts `@file:...`,
//! `@symbol:...`, `@folder:...`, `@url:...`, `@diff:...`, `@test:...`,
//! `@recent`, and `@selection` references.  The parser and resolver
//! live here so the exact same tag semantics apply on every surface
//! and the Prometheus metrics show a single aggregated latency view.
//!
//! Design notes:
//! - [`ContextTag`] is an enum (not a free-form string) so providers
//!   can match exhaustively.
//! - [`ContextResolver`] is a `trait` with a `DefaultResolver`
//!   composed of per-tag `handlers::*` implementations.  Each handler
//!   decides how much content to return based on the remaining token
//!   budget provided by the caller.
//! - Heavy dependencies (SymbolGraph, Tantivy, git) remain opt-in via
//!   features; the resolver degrades gracefully when they are
//!   missing.

pub mod budget;

pub mod codebase;
pub mod handlers;
pub mod parser;
pub mod resolver;
pub mod types;

pub use budget::{ContextBudget, Priority};
pub use parser::{parse_context_tags, strip_context_tags};
pub use resolver::{ContextResolver, DefaultResolver};
pub use types::{ContextItem, ContextResolveError, ContextTag};
