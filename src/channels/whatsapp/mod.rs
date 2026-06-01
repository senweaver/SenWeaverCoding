// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod core;
pub mod web;
#[cfg(feature = "whatsapp-web")]
pub mod storage;

pub use core::WhatsAppChannel;
pub use web::WhatsAppWebChannel;
