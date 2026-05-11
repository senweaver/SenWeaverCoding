// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReflectionTriggerCause {
    Manual,
    SessionEnd,
    Scheduled,
    FailureThreshold,
    UserThumbsDown,
}

impl ReflectionTriggerCause {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::SessionEnd => "session_end",
            Self::Scheduled => "scheduled",
            Self::FailureThreshold => "failure_threshold",
            Self::UserThumbsDown => "user_thumbs_down",
        }
    }
}
