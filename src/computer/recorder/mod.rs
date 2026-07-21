// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod hook;
pub mod replay;
pub mod session;
pub mod skillgen;
pub mod template_match;
pub mod text_capture;
pub mod types;

pub use replay::{replay_recording, replay_recording_smart, ReplayRepeat};
pub use session::{
    delete_recording, discard_recording, generate_skill, is_recording, last_saved_recording,
    list_recordings, load_recording, load_skill_instructions, rename_recording,
    save_recording_manifest, start_recording, stop_recording,
};
pub use types::{
    RecordedStep, RecorderEvent, RecorderStatus, RecorderStepEvent, RecordingManifest,
    RecordingSummary, RunConfig,
};
