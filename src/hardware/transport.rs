// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use super::protocol::{ZcCommand, ZcResponse};
use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {

    #[error("transport timeout after {0}s")]
    Timeout(u64),

    #[error("transport disconnected")]
    Disconnected,

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("transport I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportKind {

    Serial,

    Swd,

    Uf2,

    Native,

    Aardvark,
}

impl std::fmt::Display for TransportKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serial => write!(f, "serial"),
            Self::Swd => write!(f, "swd"),
            Self::Uf2 => write!(f, "uf2"),
            Self::Native => write!(f, "native"),
            Self::Aardvark => write!(f, "aardvark"),
        }
    }
}

#[async_trait]
pub trait Transport: Send + Sync {

    async fn send(&self, cmd: &ZcCommand) -> Result<ZcResponse, TransportError>;

    fn kind(&self) -> TransportKind;

    fn is_connected(&self) -> bool;
}
