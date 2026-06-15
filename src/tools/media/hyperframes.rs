// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{anyhow, Context};
use std::path::{Path, PathBuf};
use std::process::Stdio;

pub enum HyperframesOutput {
    Mp4(PathBuf),
    LiveHtml(PathBuf),
}

pub async fn render(
    composition_dir: &Path,
    output_path: &Path,
    duration_secs: u32,
) -> anyhow::Result<HyperframesOutput> {
    let index = composition_dir.join("index.html");
    if !index.exists() {
        return Err(anyhow!(
            "HyperFrames composition is missing index.html at {}",
            index.display()
        ));
    }

    match try_cli_render(composition_dir, output_path, duration_secs).await {
        Ok(true) if output_path.exists() => Ok(HyperframesOutput::Mp4(output_path.to_path_buf())),
        _ => Ok(HyperframesOutput::LiveHtml(index)),
    }
}

async fn try_cli_render(
    composition_dir: &Path,
    output_path: &Path,
    duration_secs: u32,
) -> anyhow::Result<bool> {
    let mut cmd = crate::util::hidden_async_command("npx");
    cmd.arg("--yes")
        .arg("@hyperframes/cli")
        .arg("render")
        .arg(composition_dir)
        .arg("--out")
        .arg(output_path)
        .arg("--duration")
        .arg(duration_secs.to_string())
        .kill_on_drop(true)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let status = tokio::time::timeout(std::time::Duration::from_secs(180), cmd.status())
        .await
        .context("hyperframes render timed out")?
        .context("failed to spawn hyperframes renderer")?;

    Ok(status.success())
}
