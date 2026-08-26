// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

pub mod bundle;
pub mod correlate;
pub mod describe_md;

use anyhow::Result;
use std::path::Path;

pub use bundle::{load_bundle, SessionBundle};
pub use correlate::{correlate, CorrelationResult};

pub fn is_processed(dir: &Path) -> bool {
    dir.join(bundle::BUNDLE_FILE).exists()
}

pub async fn process_recording(dir: &Path, session_id: &str, task: &str) -> Result<()> {
    let dir_owned = dir.to_path_buf();
    let session_id = session_id.to_string();
    let task = task.to_string();
    tokio::task::spawn_blocking(move || {
        let events = crate::computer::activity::events::read_events(&dir_owned);
        let frames = crate::computer::frames::list_frames(&dir_owned);
        let correlation = correlate::correlate(&events, &frames);
        std::fs::write(
            dir_owned.join(correlate::CORRELATION_FILE),
            serde_json::to_vec_pretty(&correlation)?,
        )?;
        let bundle = bundle::build_bundle(&session_id, &task, &events, Some(&correlation));
        std::fs::write(
            dir_owned.join(bundle::BUNDLE_FILE),
            serde_json::to_vec_pretty(&bundle)?,
        )?;
        std::fs::write(
            dir_owned.join(describe_md::DESCRIPTION_FILE),
            describe_md::render_description(&bundle),
        )?;
        anyhow::Ok(())
    })
    .await
    .map_err(|e| anyhow::anyhow!("timeline post-processing task failed: {e}"))?
}
