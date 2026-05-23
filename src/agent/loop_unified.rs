// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Result;

use crate::agent::loop_policy::PolicyBundle;
use crate::providers::traits::ChatMessage;

pub struct UnifiedLoop<'a> {
    policy: PolicyBundle<'a>,
}

impl<'a> UnifiedLoop<'a> {
    #[must_use]
    pub fn new(policy: PolicyBundle<'a>) -> Self {
        Self { policy }
    }

    #[must_use]
    pub fn policy(&self) -> &PolicyBundle<'a> {
        &self.policy
    }

    #[must_use]
    pub fn into_policy(self) -> PolicyBundle<'a> {
        self.policy
    }

    pub async fn run(self, history: &mut Vec<ChatMessage>) -> Result<String> {
        crate::agent::loop_::run_unified_loop_impl(self.policy, history).await
    }
}
