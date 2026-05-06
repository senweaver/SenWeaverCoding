// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//!
//! Unified RPC layer for SenWeaverCoding — JSON-RPC 2.0 over multiple transports.
//!
//! This module provides a clean IPC interface that external callers (Python, JavaScript,
//! or any language) can use to interact with the SenWeaverCoding agent kernel.
//!
//! ## Supported transports
//!
//! | Transport | Use case |
//! |-----------|---------|
//! | `Stdio`   | IDE integration, subprocess invocation |
//! | `UnixSocket` | Local Python/CLI clients |
//! | `Http`    | Network clients, microservices |
//!
//! ## Quick start (Python)
//!
//! ```python
//! import sen
//! client = sen.Client()
//! session = client.create_session()
//! response = session.prompt("Hello, agent!")
//! ```

pub mod codec;
pub mod methods;
pub mod server;

pub use server::{RpcServer, RpcServerConfig, RpcTransport};
