// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod crypto;
pub mod discovery;
pub mod identity;
pub mod protocol;
pub mod service;
pub mod share;
pub mod store;
pub mod transport;

pub use discovery::{PeerRegistry, PeerView};
pub use share::ShareService;
pub use identity::{IdentitySnapshot, LanIdentity};
pub use service::LanService;
pub use store::{ConversationView, MessageView, TransferView};

pub fn guess_mime(name: &str) -> String {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "tif" | "tiff" => "image/tiff",
        _ => "application/octet-stream",
    }
    .to_string()
}

pub fn is_image_name(name: &str) -> bool {
    guess_mime(name).starts_with("image/")
}
