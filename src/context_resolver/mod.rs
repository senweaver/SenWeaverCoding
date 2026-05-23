// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

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
