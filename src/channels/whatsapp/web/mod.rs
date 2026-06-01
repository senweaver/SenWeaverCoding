// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

#[cfg(feature = "whatsapp-web")]
mod native;
#[cfg(not(feature = "whatsapp-web"))]
mod stub;

#[cfg(feature = "whatsapp-web")]
pub use native::WhatsAppWebChannel;
#[cfg(not(feature = "whatsapp-web"))]
pub use stub::WhatsAppWebChannel;
