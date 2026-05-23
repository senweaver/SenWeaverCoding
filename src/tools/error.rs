// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ToolErrorCause {

    #[error("validation error: {0}")]
    Validation(String),

    #[error("execution error: {0}")]
    Execution(String),

    #[error("timeout after {0:?}")]
    Timeout(std::time::Duration),

    #[error("cancelled")]
    Cancelled,

    #[error("rbac denied: {0}")]
    RbacDenied(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("lock contention: {0}")]
    LockContention(String),

    #[error("precondition failed: {0}")]
    PreconditionFailed(String),

    #[error("no matching agent for capability: {0}")]
    NoMatchingAgent(String),

    #[error("unknown: {0}")]
    Unknown(#[source] anyhow::Error),
}

impl ToolErrorCause {

    pub fn validation(msg: impl Into<String>) -> Self {
        ToolErrorCause::Validation(msg.into())
    }

    pub fn execution(msg: impl Into<String>) -> Self {
        ToolErrorCause::Execution(msg.into())
    }

    pub fn precondition(msg: impl Into<String>) -> Self {
        ToolErrorCause::PreconditionFailed(msg.into())
    }

    pub fn provider(msg: impl Into<String>) -> Self {
        ToolErrorCause::Provider(msg.into())
    }

    pub fn kind_tag(&self) -> &'static str {
        match self {
            ToolErrorCause::Validation(_) => "validation",
            ToolErrorCause::Execution(_) => "execution",
            ToolErrorCause::Timeout(_) => "timeout",
            ToolErrorCause::Cancelled => "cancelled",
            ToolErrorCause::RbacDenied(_) => "rbac_denied",
            ToolErrorCause::Io(_) => "io",
            ToolErrorCause::Provider(_) => "provider",
            ToolErrorCause::LockContention(_) => "lock_contention",
            ToolErrorCause::PreconditionFailed(_) => "precondition_failed",
            ToolErrorCause::NoMatchingAgent(_) => "no_matching_agent",
            ToolErrorCause::Unknown(_) => "unknown",
        }
    }
}
