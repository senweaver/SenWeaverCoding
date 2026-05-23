// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result, bail};
use std::path::Path;
use tracing::{info, warn};

const GITHUB_RELEASES_LATEST_URL: &str =
    "https://api.github.com/repos/senweaver/SenWeaverCoding/releases/latest";
const GITHUB_RELEASES_TAG_URL: &str =
    "https://api.github.com/repos/senweaver/SenWeaverCoding/releases/tags";

#[derive(Debug)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub download_url: Option<String>,
    pub is_newer: bool,
}

pub async fn check(target_version: Option<&str>) -> Result<UpdateInfo> {
    let current = env!("CARGO_PKG_VERSION").to_string();

    let client = reqwest::Client::builder()
        .user_agent(format!("sen/{current}"))
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let url = match target_version {
        Some(v) => {
            let tag = if v.starts_with('v') {
                v.to_string()
            } else {
                format!("v{v}")
            };
            format!("{GITHUB_RELEASES_TAG_URL}/{tag}")
        }
        None => GITHUB_RELEASES_LATEST_URL.to_string(),
    };

    let resp = client
        .get(&url)
        .send()
        .await
        .context("failed to reach GitHub releases API")?;

    if !resp.status().is_success() {
        bail!("GitHub API returned {}", resp.status());
    }

    let release: serde_json::Value = resp.json().await?;
    let tag = release["tag_name"]
        .as_str()
        .unwrap_or("unknown")
        .trim_start_matches('v')
        .to_string();

    let download_url = find_asset_url(&release);
    let is_newer = version_is_newer(&current, &tag);

    Ok(UpdateInfo {
        current_version: current,
        latest_version: tag,
        download_url,
        is_newer,
    })
}

pub async fn run(target_version: Option<&str>) -> Result<()> {

    info!("Step 1/6: Preflight checks...");
    let update_info = check(target_version).await?;

    if !update_info.is_newer {
        println!("Already up to date (v{}).", update_info.current_version);
        return Ok(());
    }

    println!(
        "Update available: v{} -> v{}",
        update_info.current_version, update_info.latest_version
    );

    let download_url = update_info
        .download_url
        .context("no suitable binary found for this platform")?;

    let current_exe =
        std::env::current_exe().context("cannot determine current executable path")?;

    info!("Step 2/6: Downloading...");
    let temp_dir = tempfile::tempdir().context("failed to create temp dir")?;
    let download_path = temp_dir.path().join("sen_new");
    download_binary(&download_url, &download_path).await?;

    info!("Step 3/6: Creating backup...");
    let backup_path = current_exe.with_extension("bak");
    tokio::fs::copy(&current_exe, &backup_path)
        .await
        .context("failed to backup current binary")?;

    info!("Step 4/6: Validating download...");
    validate_binary(&download_path).await?;

    info!("Step 5/6: Swapping binary...");
    if let Err(e) = swap_binary(&download_path, &current_exe).await {

        warn!("Swap failed, rolling back: {e}");
        if let Err(rollback_err) = rollback_binary(&backup_path, &current_exe).await {
            eprintln!("CRITICAL: Rollback also failed: {rollback_err}");
            eprintln!(
                "Manual recovery: cp {} {}",
                backup_path.display(),
                current_exe.display()
            );
        }
        bail!("Update failed during swap: {e}");
    }

    info!("Step 6/6: Smoke test...");
    match smoke_test(&current_exe).await {
        Ok(()) => {

            let _ = tokio::fs::remove_file(&backup_path).await;
            println!("Successfully updated to v{}!", update_info.latest_version);
            Ok(())
        }
        Err(e) => {
            warn!("Smoke test failed, rolling back: {e}");
            rollback_binary(&backup_path, &current_exe)
                .await
                .context("rollback after smoke test failure")?;
            bail!("Update rolled back — smoke test failed: {e}");
        }
    }
}

fn find_asset_url(release: &serde_json::Value) -> Option<String> {
    let target = current_target_triple();

    release["assets"]
        .as_array()?
        .iter()
        .find(|asset| {
            asset["name"]
                .as_str()
                .map(|name| name.contains(target))
                .unwrap_or(false)
        })
        .and_then(|asset| asset["browser_download_url"].as_str().map(String::from))
}

fn current_target_triple() -> &'static str {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-unknown-linux-gnu"
        } else {
            "x86_64-unknown-linux-gnu"
        }
    } else if cfg!(target_os = "windows") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-pc-windows-msvc"
        } else {
            "x86_64-pc-windows-msvc"
        }
    } else {
        "unknown"
    }
}

fn version_is_newer(current: &str, candidate: &str) -> bool {
    let parse = |v: &str| -> Vec<u32> { v.split('.').filter_map(|p| p.parse().ok()).collect() };
    let cur = parse(current);
    let cand = parse(candidate);
    cand > cur
}

async fn download_binary(url: &str, dest: &Path) -> Result<()> {
    let client = reqwest::Client::builder()
        .user_agent(format!("sen/{}", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let resp = client
        .get(url)
        .send()
        .await
        .context("download request failed")?;
    if !resp.status().is_success() {
        bail!("download returned {}", resp.status());
    }

    let bytes = resp.bytes().await.context("failed to read download body")?;

    if url.ends_with(".tar.gz") || url.ends_with(".tgz") {
        extract_tar_gz(&bytes, dest).context("failed to extract binary from tar.gz archive")?;
    } else {
        tokio::fs::write(dest, &bytes)
            .await
            .context("failed to write downloaded binary")?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        tokio::fs::set_permissions(dest, perms).await?;
    }

    Ok(())
}

fn extract_tar_gz(archive_bytes: &[u8], dest: &Path) -> Result<()> {
    use flate2::read::GzDecoder;
    use std::io::Read;
    use tar::Archive;

    let gz = GzDecoder::new(archive_bytes);
    let mut archive = Archive::new(gz);

    for entry in archive.entries().context("failed to read tar entries")? {
        let mut entry = entry.context("failed to read tar entry")?;
        let path = entry.path().context("failed to read entry path")?;

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if file_name == "sen" || file_name == "sen.exe" {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .context("failed to read binary from archive")?;
            std::fs::write(dest, &buf).context("failed to write extracted binary")?;
            return Ok(());
        }
    }

    bail!("archive does not contain a 'sen' binary")
}

async fn validate_binary(path: &Path) -> Result<()> {
    let meta = tokio::fs::metadata(path).await?;
    if meta.len() < 1_000_000 {
        bail!(
            "downloaded binary too small ({} bytes), likely corrupt",
            meta.len()
        );
    }

    check_binary_arch(path).await?;

    let output = crate::util::hidden_async_command(path)
        .arg("--version")
        .output()
        .await
        .context("cannot execute downloaded binary")?;

    if !output.status.success() {
        bail!("downloaded binary --version check failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("sen") {
        bail!("downloaded binary does not appear to be sen");
    }

    Ok(())
}

async fn check_binary_arch(path: &Path) -> Result<()> {
    let header = tokio::fs::read(path)
        .await
        .map(|bytes| bytes.into_iter().take(32).collect::<Vec<u8>>())
        .context("failed to read binary header")?;

    if header.len() < 20 {
        bail!("downloaded file too small to be a valid binary");
    }

    let binary_arch = detect_arch_from_header(&header);
    let host_arch = host_architecture();

    if let (Some(bin), Some(host)) = (binary_arch, host_arch) {
        if bin != host {
            bail!(
                "architecture mismatch: downloaded binary is {bin} but this host is {host} — \
                 the release asset may be mispackaged"
            );
        }
    }

    Ok(())
}

fn detect_arch_from_header(header: &[u8]) -> Option<&'static str> {

    if header.len() >= 20 && header[0..4] == [0x7f, b'E', b'L', b'F'] {

        let e_machine = u16::from_le_bytes([header[18], header[19]]);
        return Some(match e_machine {
            0x3E => "x86_64",
            0xB7 => "aarch64",
            0x03 => "x86",
            0x28 => "arm",
            0xF3 => "riscv",
            _ => "unknown-elf",
        });
    }

    if header.len() >= 8 && header[0..4] == [0xCF, 0xFA, 0xED, 0xFE] {
        let cputype = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
        return Some(match cputype {
            0x0100_0007 => "x86_64",
            0x0100_000C => "aarch64",
            _ => "unknown-macho",
        });
    }

    None
}

fn host_architecture() -> Option<&'static str> {
    if cfg!(target_arch = "x86_64") {
        Some("x86_64")
    } else if cfg!(target_arch = "aarch64") {
        Some("aarch64")
    } else if cfg!(target_arch = "x86") {
        Some("x86")
    } else if cfg!(target_arch = "arm") {
        Some("arm")
    } else {
        None
    }
}

async fn swap_binary(new: &Path, target: &Path) -> Result<()> {

    tokio::fs::remove_file(target)
        .await
        .context("failed to remove old binary")?;
    tokio::fs::copy(new, target)
        .await
        .context("failed to write new binary")?;
    Ok(())
}

async fn rollback_binary(backup: &Path, target: &Path) -> Result<()> {

    let _ = tokio::fs::remove_file(target).await;
    tokio::fs::copy(backup, target)
        .await
        .context("failed to restore backup binary")?;
    Ok(())
}

async fn smoke_test(binary: &Path) -> Result<()> {
    let output = crate::util::hidden_async_command(binary)
        .arg("--version")
        .output()
        .await
        .context("smoke test: cannot execute updated binary")?;

    if !output.status.success() {
        bail!("smoke test: updated binary returned non-zero exit code");
    }

    Ok(())
}
