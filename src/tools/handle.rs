// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! Lightweight handle used by the tool dispatch hot-path.
//!
//! Historically `find_tool` only returned `&'a dyn Tool`, which forced every
//! caller to resolve tools via a linear scan over `&[Box<dyn Tool>]`.  With
//! the introduction of [`crate::tools::registry::ToolRegistry`] (DashMap
//! backed, O(1) name lookup) we need an abstraction that can hold either a
//! borrowed reference from the legacy slice or an `Arc<dyn Tool>` handed back
//! from the registry.
//!
//! `ToolHandle<'a>` is that abstraction: it derefs to `dyn Tool`, so existing
//! method calls (`tool.name()`, `tool.execute(...)`) continue to work
//! transparently.

use std::sync::Arc;

use super::traits::Tool;

pub enum ToolHandle<'a> {

    Borrowed(&'a dyn Tool),

    Owned(Arc<dyn Tool>),
}

impl<'a> ToolHandle<'a> {

    #[inline]
    pub fn is_registry_hit(&self) -> bool {
        matches!(self, Self::Owned(_))
    }

    #[inline]
    pub fn as_tool(&self) -> &dyn Tool {
        match self {
            Self::Borrowed(t) => *t,
            Self::Owned(a) => a.as_ref(),
        }
    }
}

