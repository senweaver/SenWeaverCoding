// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod extract;
pub mod hash;
pub mod log;

pub use extract::{extract_window, list_frames, CropRect, ExtractedFrame};
pub use log::{load_manifest, FrameLog, FrameManifest, FrameRecord};
