// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod core;
#[cfg(feature = "whatsapp-web")]
pub mod storage;
#[cfg(feature = "whatsapp-web")]
pub mod web;

pub use core::WhatsAppChannel;
#[cfg(feature = "whatsapp-web")]
pub use web::WhatsAppWebChannel;
