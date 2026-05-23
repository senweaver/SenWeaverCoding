// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

const PICO_UF2: &[u8] = include_bytes!("../../firmware/pico/sen-pico.uf2");

pub const PICO_MAIN_PY: &[u8] = include_bytes!("../../firmware/pico/main.py");

const UF2_MAGIC1: [u8; 4] = [0x55, 0x46, 0x32, 0x0A];

pub fn find_rpi_rp2_mount() -> Option<PathBuf> {

    let mac = PathBuf::from("/Volumes/RPI-RP2");
    if mac.exists() {
        return Some(mac);
    }

    for base in &["/media", "/run/media"] {
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("RPI-RP2");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

pub fn ensure_firmware_dir() -> Result<PathBuf> {
    use directories::BaseDirs;

    let base = BaseDirs::new().ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;

    let firmware_dir = base
        .home_dir()
        .join(".senweavercoding")
        .join("firmware")
        .join("pico");
    std::fs::create_dir_all(&firmware_dir)?;

    let uf2_path = firmware_dir.join("sen-pico.uf2");
    if !uf2_path.exists() {
        if PICO_UF2.len() < 8 || PICO_UF2[..4] != UF2_MAGIC1 {
            bail!(
                "Bundled UF2 is a placeholder — download the real MicroPython UF2 from \
                 https://micropython.org/download/RPI_PICO/ and place it at \
                 src/firmware/pico/sen-pico.uf2, then rebuild SenWeaverCoding."
            );
        }
        std::fs::write(&uf2_path, PICO_UF2)?;
        tracing::info!(path = %uf2_path.display(), "extracted bundled UF2");
    }

    let main_py_path = firmware_dir.join("main.py");
    if !main_py_path.exists() {
        std::fs::write(&main_py_path, PICO_MAIN_PY)?;
        tracing::info!(path = %main_py_path.display(), "extracted bundled main.py");
    }

    Ok(firmware_dir)
}

pub async fn flash_uf2(mount_point: &Path, firmware_dir: &Path) -> Result<()> {
    let uf2_src = firmware_dir.join("sen-pico.uf2");
    let uf2_dst = mount_point.join("firmware.uf2");
    let src_str = uf2_src.to_string_lossy().into_owned();
    let dst_str = uf2_dst.to_string_lossy().into_owned();

    tracing::info!(
        src = %src_str,
        dst = %dst_str,
        "flashing UF2"
    );

    let data = std::fs::read(&uf2_src)?;
    if data.len() < 8 || data[..4] != UF2_MAGIC1 {
        bail!(
            "UF2 at {} does not look like a valid UF2 file (magic mismatch). \
             Download from https://micropython.org/download/RPI_PICO/ and delete \
             the existing file so SenWeaverCoding can re-extract it.",
            uf2_src.display()
        );
    }

    {
        let src = uf2_src.clone();
        let dst = uf2_dst.clone();
        let result = tokio::task::spawn_blocking(move || std::fs::copy(&src, &dst))
            .await
            .map_err(|e| anyhow::anyhow!("copy task panicked: {e}"));

        match result {
            Ok(Ok(_)) => {
                tracing::info!("UF2 copy complete (std::fs::copy) — Pico will reboot");
                return Ok(());
            }
            Ok(Err(e)) => tracing::warn!("std::fs::copy failed ({}), trying cp", e),
            Err(e) => tracing::warn!("std::fs::copy task failed ({}), trying cp", e),
        }
    }

    {

        const CP_TIMEOUT_SECS: u64 = 10;

        let out = tokio::time::timeout(
            std::time::Duration::from_secs(CP_TIMEOUT_SECS),
            crate::util::hidden_async_command("cp")
                .arg(&src_str)
                .arg(&dst_str)
                .output(),
        )
        .await;

        match out {
            Err(_elapsed) => {
                tracing::warn!("cp timed out after {}s, trying sudo cp", CP_TIMEOUT_SECS);
            }
            Ok(Ok(o)) if o.status.success() => {
                tracing::info!("UF2 copy complete (cp) — Pico will reboot");
                return Ok(());
            }
            Ok(Ok(o)) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                tracing::warn!("cp failed ({}), trying sudo cp", stderr.trim());
            }
            Ok(Err(e)) => tracing::warn!("cp spawn failed ({}), trying sudo cp", e),
        }
    }

    {
        const SUDO_CP_TIMEOUT_SECS: u64 = 10;

        let out = tokio::time::timeout(
            std::time::Duration::from_secs(SUDO_CP_TIMEOUT_SECS),
            crate::util::hidden_async_command("sudo")
                .args(["-n", "cp", &src_str, &dst_str])
                .output(),
        )
        .await;

        match out {
            Err(_elapsed) => {
                tracing::warn!("sudo cp timed out after {}s", SUDO_CP_TIMEOUT_SECS);
            }
            Ok(Ok(o)) if o.status.success() => {
                tracing::info!("UF2 copy complete (sudo cp) — Pico will reboot");
                return Ok(());
            }
            Ok(Ok(o)) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                tracing::warn!("sudo cp failed: {}", stderr.trim());
            }
            Ok(Err(e)) => tracing::warn!("sudo cp spawn failed: {}", e),
        }
    }

    bail!(
        "All copy methods failed. Run this command manually, then restart SenWeaverCoding:\n\
         \n  sudo cp {src_str} {dst_str}\n"
    )
}

pub async fn wait_for_serial_port(
    timeout: std::time::Duration,
    interval: std::time::Duration,
) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let patterns = &["/dev/cu.usbmodem*"];
    #[cfg(target_os = "linux")]
    let patterns = &["/dev/ttyACM*"];
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let patterns: &[&str] = &[];

    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        for pattern in *patterns {
            if let Ok(mut hits) = glob::glob(pattern) {
                if let Some(Ok(path)) = hits.next() {
                    return Some(path);
                }
            }
        }

        if tokio::time::Instant::now() >= deadline {
            return None;
        }

        tokio::time::sleep(interval).await;
    }
}

pub async fn deploy_main_py(port: &Path, firmware_dir: &Path) -> Result<()> {
    let main_py_src = firmware_dir.join("main.py");
    let src_str = main_py_src.to_string_lossy().into_owned();
    let port_str = port.to_string_lossy().into_owned();

    if !main_py_src.exists() {
        bail!(
            "main.py not found at {} — run ensure_firmware_dir() first",
            main_py_src.display()
        );
    }

    tracing::info!(
        src = %src_str,
        port = %port_str,
        "deploying main.py via mpremote"
    );

    let out = crate::util::hidden_async_command("mpremote")
        .args([
            "connect", &port_str, "cp", &src_str, ":main.py", "+", "reset",
        ])
        .output()
        .await;

    match out {
        Ok(o) if o.status.success() => {
            tracing::info!("main.py deployed and Pico reset via mpremote");
            Ok(())
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            bail!(
                "mpremote failed (exit {}): {}.\n\
                 Run manually:\n  mpremote connect {port_str} cp {src_str} :main.py + reset",
                o.status,
                stderr.trim()
            )
        }
        Err(e) => {
            bail!(
                "mpremote not found or could not start ({e}).\n\
                 Install it with: pip install mpremote\n\
                 Then run: mpremote connect {port_str} cp {src_str} :main.py + reset"
            )
        }
    }
}
