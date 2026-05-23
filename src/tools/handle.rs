// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use super::traits::Tool;

pub enum ToolHandle<'a> {

    Borrowed(&'a dyn Tool),

    Owned(Arc<dyn Tool>),
}

impl ToolHandle<'_> {

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

