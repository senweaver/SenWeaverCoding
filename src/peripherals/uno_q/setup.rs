// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Context, Result};

const BRIDGE_APP_NAME: &str = "uno-q-bridge";

pub fn setup_uno_q_bridge(host: Option<&str>) -> Result<()> {
    let bridge_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("firmware")
        .join("uno-q-bridge");

    if let Some(h) = host {
        if bridge_dir.is_dir() {
            deploy_remote(h, &bridge_dir)?;
        } else if let Some(fallback) = home_bridge_dir().filter(|d| d.is_dir()) {
            deploy_remote(h, &fallback)?;
        } else {
            anyhow::bail!(
                "Bridge app not found at {} or ~/.senweavercoding/firmware/{BRIDGE_APP_NAME}. \
                 Place the app and retry.",
                bridge_dir.display()
            );
        }
    } else {
        deploy_local(if bridge_dir.exists() {
            Some(&bridge_dir)
        } else {
            None
        })?;
    }
    Ok(())
}

fn home_bridge_dir() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|dirs| {
        dirs.home_dir()
            .join(".senweavercoding")
            .join("firmware")
            .join(BRIDGE_APP_NAME)
    })
}

fn deploy_remote(host: &str, bridge_dir: &std::path::Path) -> Result<()> {
    let ssh_target = if host.contains('@') {
        host.to_string()
    } else {
        format!("arduino@{}", host)
    };

    println!("Copying Bridge app to {}...", host);
    let status = crate::util::hidden_sync_command("ssh")
        .args([&ssh_target, "mkdir", "-p", "~/ArduinoApps"])
        .status()
        .context("ssh mkdir failed")?;
    if !status.success() {
        anyhow::bail!("Failed to create ArduinoApps dir on Uno Q");
    }

    let status = crate::util::hidden_sync_command("scp")
        .args([
            "-r",
            &bridge_dir.to_string_lossy(),
            &format!("{}:~/ArduinoApps/", ssh_target),
        ])
        .status()
        .context("scp failed")?;
    if !status.success() {
        anyhow::bail!("Failed to copy Bridge app");
    }

    println!("Starting Bridge app on Uno Q...");
    let status = crate::util::hidden_sync_command("ssh")
        .args([
            &ssh_target,
            "arduino-app-cli",
            "app",
            "start",
            "~/ArduinoApps/uno-q-bridge",
        ])
        .status()
        .context("arduino-app-cli start failed")?;
    if !status.success() {
        anyhow::bail!("Failed to start Bridge app. Ensure arduino-app-cli is installed on Uno Q.");
    }

    println!("SenWeaverCoding Bridge app started. Add to config.toml:");
    println!("  [[peripherals.boards]]");
    println!("  board = \"arduino-uno-q\"");
    println!("  transport = \"bridge\"");
    Ok(())
}

fn deploy_local(bridge_dir: Option<&std::path::Path>) -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/arduino".into());
    let apps_dir = std::path::Path::new(&home).join("ArduinoApps");
    let dest_dir = apps_dir.join(BRIDGE_APP_NAME);

    std::fs::create_dir_all(&dest_dir).context("create dest dir")?;

    if let Some(src) = bridge_dir {
        println!("Copying Bridge app from repo...");
        copy_dir(src, &dest_dir)?;
    } else {
        install_bridge_assets(&dest_dir)?;
    }

    println!("Starting Bridge app...");
    let status = crate::util::hidden_sync_command("arduino-app-cli")
        .args(["app", "start", &dest_dir.to_string_lossy()])
        .status()
        .context("arduino-app-cli start failed")?;
    if !status.success() {
        anyhow::bail!("Failed to start Bridge app. Ensure arduino-app-cli is installed on Uno Q.");
    }

    println!("SenWeaverCoding Bridge app started.");
    Ok(())
}

fn install_bridge_assets(dest: &std::path::Path) -> Result<()> {
    if let Some(fallback) = home_bridge_dir().filter(|d| d.is_dir()) {
        println!("Copying Bridge app from {}...", fallback.display());
        return copy_dir(&fallback, dest);
    }
    anyhow::bail!(
        "Bridge app assets not found. Place the uno-q-bridge app at <repo>/firmware/uno-q-bridge/ \
         or ~/.senweavercoding/firmware/uno-q-bridge/ and retry."
    )
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) -> Result<()> {
    for entry in std::fs::read_dir(src)? {
        let e = entry?;
        let name = e.file_name();
        let src_path = src.join(&name);
        let dst_path = dst.join(&name);
        if e.file_type()?.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            copy_dir(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
