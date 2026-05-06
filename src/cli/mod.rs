// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//! CLI subsystem — SDK I/O protocol, remote transports, headless driver,
//! background session management, and CLI handler subcommands.

pub mod bg;
pub mod dispatch;
pub mod exit;

pub mod headless;
pub mod input;
pub mod ndjson;
pub mod remote_io;
pub mod structured_io;
pub mod transports;
