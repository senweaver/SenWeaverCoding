// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Bridge module — remote control bridge for WebSocket-based session management.
// Mirrors claude-code's `bridge/` directory.
//
// Provides device pairing, JWT-based authentication, remote session creation,
// message relay, and capacity/wake management for controlling agent sessions
// from mobile/web clients.

pub mod api;
pub mod config;
pub mod device;
pub mod jwt;
pub mod messaging;
pub mod session;
pub mod status;
pub mod transport;
pub mod types;

pub use config::BridgeConfig;
pub use device::TrustedDevice;
pub use messaging::BridgeMessaging;
pub use session::BridgeSession;
pub use transport::BridgeTransport;
pub use types::{BridgeEvent, BridgeMessage, BridgeStatus};
