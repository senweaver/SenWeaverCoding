// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use crate::config::Config;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

const SERVICE_LABEL: &str = "com.senweavercoding.daemon";
const WINDOWS_TASK_NAME: &str = "SenWeaverCoding Daemon";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InitSystem {
    #[default]
    Auto,
    Systemd,
    Openrc,
}

impl FromStr for InitSystem {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "systemd" => Ok(Self::Systemd),
            "openrc" => Ok(Self::Openrc),
            other => bail!(
                "Unknown init system: '{}'. Supported: auto, systemd, openrc",
                other
            ),
        }
    }
}

impl InitSystem {
    #[cfg(target_os = "linux")]
    pub fn resolve(self) -> Result<Self> {
        match self {
            Self::Auto => detect_init_system(),
            concrete => Ok(concrete),
        }
    }

    #[cfg(not(target_os = "linux"))]
    pub fn resolve(self) -> Result<Self> {
        match self {
            Self::Auto => Ok(Self::Systemd),
            concrete => Ok(concrete),
        }
    }

    #[cfg(target_os = "linux")]
    pub fn from_str(s: &str) -> Result<Self> {
        <Self as FromStr>::from_str(s)
    }
}

#[cfg(target_os = "linux")]
fn detect_init_system() -> Result<InitSystem> {
    if Path::new("/run/systemd/system").exists() {
        return Ok(InitSystem::Systemd);
    }
    if Path::new("/run/openrc").exists() {
        if Path::new("/sbin/openrc-run").exists() || which::which("rc-service").is_ok() {
            return Ok(InitSystem::Openrc);
        }
    }
    bail!(
        "Could not detect init system. Supported: systemd, OpenRC. \
         Use --service-init to specify manually."
    );
}

fn windows_task_name() -> &'static str {
    WINDOWS_TASK_NAME
}

pub fn is_running() -> bool {
    if cfg!(target_os = "macos") {
        run_capture(crate::util::hidden_sync_command("launchctl").arg("list"))
            .map(|out| out.lines().any(|l| l.contains(SERVICE_LABEL)))
            .unwrap_or(false)
    } else if cfg!(target_os = "linux") {
        is_running_linux()
    } else if cfg!(target_os = "windows") {
        run_capture(crate::util::hidden_sync_command("schtasks").args([
            "/Query",
            "/TN",
            WINDOWS_TASK_NAME,
            "/FO",
            "LIST",
        ]))
        .map(|out| out.contains("Running"))
        .unwrap_or(false)
    } else {
        false
    }
}

fn is_running_linux() -> bool {
    if run_capture(crate::util::hidden_sync_command("systemctl").args(["--user", "is-active", "sen.service"]))
        .map(|out| out.trim() == "active")
        .unwrap_or(false)
    {
        return true;
    }
    run_capture(crate::util::hidden_sync_command("rc-service").args(["sen", "status"]))
        .map(|out| out.contains("started"))
        .unwrap_or(false)
}

pub fn handle_command(
    command: &crate::ServiceCommands,
    config: &Config,
    init_system: InitSystem,
) -> Result<()> {
    match command {
        crate::ServiceCommands::Install => install(config, init_system),
        crate::ServiceCommands::Start => start(config, init_system),
        crate::ServiceCommands::Stop => stop(config, init_system),
        crate::ServiceCommands::Restart => restart(config, init_system),
        crate::ServiceCommands::Status => status(config, init_system),
        crate::ServiceCommands::Uninstall => uninstall(config, init_system),
        crate::ServiceCommands::Logs { lines, follow } => {
            logs(config, init_system, *lines, *follow)
        }
    }
}

fn install(config: &Config, init_system: InitSystem) -> Result<()> {
    if cfg!(target_os = "macos") {
        install_macos(config)
    } else if cfg!(target_os = "linux") {
        let resolved = init_system.resolve()?;
        install_linux(config, resolved)
    } else if cfg!(target_os = "windows") {
        install_windows(config)
    } else {
        anyhow::bail!("Service management is supported on macOS and Linux only");
    }
}

fn start(config: &Config, init_system: InitSystem) -> Result<()> {
    if cfg!(target_os = "macos") {
        let exe = std::env::current_exe().ok();
        if let Some(ref exe_path) = exe {
            if let Some(var_dir) = detect_homebrew_var_dir(exe_path) {
                let _ = fs::create_dir_all(&var_dir);
            }
        }
        let plist = macos_service_file()?;
        run_checked(crate::util::hidden_sync_command("launchctl").arg("load").arg("-w").arg(&plist))?;
        run_checked(crate::util::hidden_sync_command("launchctl").arg("start").arg(SERVICE_LABEL))?;
        println!("Service started");
        Ok(())
    } else if cfg!(target_os = "linux") {
        let resolved = init_system.resolve()?;
        start_linux(resolved)
    } else if cfg!(target_os = "windows") {
        let _ = config;
        run_checked(crate::util::hidden_sync_command("schtasks").args(["/Run", "/TN", windows_task_name()]))?;
        println!("Service started");
        Ok(())
    } else {
        let _ = config;
        anyhow::bail!("Service management is supported on macOS and Linux only")
    }
}

fn start_linux(init_system: InitSystem) -> Result<()> {
    match init_system {
        InitSystem::Systemd => {
            run_checked(crate::util::hidden_sync_command("systemctl").args(["--user", "daemon-reload"]))?;
            run_checked(crate::util::hidden_sync_command("systemctl").args(["--user", "start", "sen.service"]))?;
        }
        InitSystem::Openrc => {
            run_checked(crate::util::hidden_sync_command("rc-service").args(["sen", "start"]))?;
        }
        InitSystem::Auto => anyhow::bail!("InitSystem::Auto must be resolved to a concrete init system before reaching service control logic"),
    }
    println!("Service started");
    Ok(())
}

fn stop(config: &Config, init_system: InitSystem) -> Result<()> {
    if cfg!(target_os = "macos") {
        let plist = macos_service_file()?;
        let _ = run_checked(crate::util::hidden_sync_command("launchctl").arg("stop").arg(SERVICE_LABEL));
        let _ = run_checked(
            crate::util::hidden_sync_command("launchctl")
                .arg("unload")
                .arg("-w")
                .arg(&plist),
        );
        println!("Service stopped");
        Ok(())
    } else if cfg!(target_os = "linux") {
        let resolved = init_system.resolve()?;
        stop_linux(resolved)
    } else if cfg!(target_os = "windows") {
        let _ = config;
        let task_name = windows_task_name();
        let _ = run_checked(crate::util::hidden_sync_command("schtasks").args(["/End", "/TN", task_name]));
        println!("Service stopped");
        Ok(())
    } else {
        let _ = config;
        anyhow::bail!("Service management is supported on macOS and Linux only")
    }
}

fn stop_linux(init_system: InitSystem) -> Result<()> {
    match init_system {
        InitSystem::Systemd => {
            let _ = run_checked(crate::util::hidden_sync_command("systemctl").args(["--user", "stop", "sen.service"]));
        }
        InitSystem::Openrc => {
            let _ = run_checked(crate::util::hidden_sync_command("rc-service").args(["sen", "stop"]));
        }
        InitSystem::Auto => anyhow::bail!("InitSystem::Auto must be resolved to a concrete init system before reaching service control logic"),
    }
    println!("Service stopped");
    Ok(())
}

fn restart(config: &Config, init_system: InitSystem) -> Result<()> {
    if cfg!(target_os = "macos") {
        stop(config, init_system)?;
        start(config, init_system)?;
        println!("Service restarted");
        return Ok(());
    }
    if cfg!(target_os = "linux") {
        let resolved = init_system.resolve()?;
        return restart_linux(resolved);
    }
    if cfg!(target_os = "windows") {
        stop(config, init_system)?;
        start(config, init_system)?;
        println!("Service restarted");
        return Ok(());
    }
    anyhow::bail!("Service management is supported on macOS and Linux only")
}

fn restart_linux(init_system: InitSystem) -> Result<()> {
    match init_system {
        InitSystem::Systemd => {
            run_checked(crate::util::hidden_sync_command("systemctl").args(["--user", "daemon-reload"]))?;
            run_checked(crate::util::hidden_sync_command("systemctl").args(["--user", "restart", "sen.service"]))?;
        }
        InitSystem::Openrc => {
            run_checked(crate::util::hidden_sync_command("rc-service").args(["sen", "restart"]))?;
        }
        InitSystem::Auto => anyhow::bail!("InitSystem::Auto must be resolved to a concrete init system before reaching service control logic"),
    }
    println!("Service restarted");
    Ok(())
}

fn status(config: &Config, init_system: InitSystem) -> Result<()> {
    if cfg!(target_os = "macos") {
        let out = run_capture(crate::util::hidden_sync_command("launchctl").arg("list"))?;
        let running = out.lines().any(|line| line.contains(SERVICE_LABEL));
        println!(
            "Service: {}",
            if running {
                "running/loaded"
            } else {
                "not loaded"
            }
        );
        println!("Unit: {}", macos_service_file()?.display());
        return Ok(());
    }
    if cfg!(target_os = "linux") {
        let resolved = init_system.resolve()?;
        return status_linux(config, resolved);
    }
    if cfg!(target_os = "windows") {
        let _ = config;
        let task_name = windows_task_name();
        let out =
            run_capture(crate::util::hidden_sync_command("schtasks").args(["/Query", "/TN", task_name, "/FO", "LIST"]));
        match out {
            Ok(text) => {
                let running = text.contains("Running");
                println!(
                    "Service: {}",
                    if running { "running" } else { "not running" }
                );
                println!("Task: {}", task_name);
            }
            Err(_) => {
                println!("Service: not installed");
            }
        }
        return Ok(());
    }
    anyhow::bail!("Service management is supported on macOS and Linux only")
}

fn status_linux(config: &Config, init_system: InitSystem) -> Result<()> {
    match init_system {
        InitSystem::Systemd => {
            let out =
                run_capture(crate::util::hidden_sync_command("systemctl").args(["--user", "is-active", "sen.service"]))
                    .unwrap_or_else(|_| "unknown".into());
            println!("Service state: {}", out.trim());
            println!("Unit: {}", linux_service_file(config)?.display());
        }
        InitSystem::Openrc => {
            let out = run_capture(crate::util::hidden_sync_command("rc-service").args(["sen", "status"]))
                .unwrap_or_else(|_| "unknown".into());
            println!("Service state: {}", out.trim());
            println!("Unit: /etc/init.d/sen");
        }
        InitSystem::Auto => anyhow::bail!("InitSystem::Auto must be resolved to a concrete init system before reaching service control logic"),
    }
    Ok(())
}

fn logs(config: &Config, init_system: InitSystem, lines: usize, follow: bool) -> Result<()> {
    if cfg!(target_os = "macos") {
        return logs_macos(config, lines, follow);
    }
    if cfg!(target_os = "linux") {
        let resolved = init_system.resolve()?;
        return logs_linux(config, resolved, lines, follow);
    }
    if cfg!(target_os = "windows") {
        return logs_windows(config, lines, follow);
    }
    anyhow::bail!("Service log viewing is supported on macOS, Linux, and Windows only")
}

fn logs_macos(config: &Config, lines: usize, follow: bool) -> Result<()> {
    let exe = std::env::current_exe().ok();
    let homebrew_var_dir = exe.as_ref().and_then(|e| detect_homebrew_var_dir(e));
    let logs_dir = if let Some(ref var_dir) = homebrew_var_dir {
        var_dir.join("logs")
    } else {
        config
            .config_path
            .parent()
            .map_or_else(|| PathBuf::from("."), PathBuf::from)
            .join("logs")
    };

    let stderr_log = logs_dir.join("daemon.stderr.log");
    let stdout_log = logs_dir.join("daemon.stdout.log");

    let log_file = if stderr_log.exists() {
        stderr_log
    } else if stdout_log.exists() {
        stdout_log
    } else {
        bail!(
            "No log files found in {}. Is the service installed?",
            logs_dir.display()
        );
    };

    if follow {
        let status = crate::util::hidden_sync_command("tail")
            .args(["-n", &lines.to_string(), "-f"])
            .arg(&log_file)
            .status()
            .context("Failed to run tail")?;
        if !status.success() {
            bail!("tail exited with non-zero status");
        }
    } else {
        let status = crate::util::hidden_sync_command("tail")
            .args(["-n", &lines.to_string()])
            .arg(&log_file)
            .status()
            .context("Failed to run tail")?;
        if !status.success() {
            bail!("tail exited with non-zero status");
        }
    }
    Ok(())
}

fn logs_linux(config: &Config, init_system: InitSystem, lines: usize, follow: bool) -> Result<()> {
    match init_system {
        InitSystem::Systemd => {
            let mut args = vec![
                "--user".to_string(),
                "-u".to_string(),
                "sen.service".to_string(),
                "-n".to_string(),
                lines.to_string(),
                "--no-pager".to_string(),
            ];
            if follow {
                args.push("-f".to_string());
            }
            let status = crate::util::hidden_sync_command("journalctl")
                .args(&args)
                .status()
                .context("Failed to run journalctl")?;
            if !status.success() {
                bail!("journalctl exited with non-zero status");
            }
        }
        InitSystem::Openrc => {
            let log_file = Path::new("/var/log/sen/error.log");
            if !log_file.exists() {
                let access_log = Path::new("/var/log/sen/access.log");
                if !access_log.exists() {
                    bail!("No log files found at /var/log/sen/. Is the service installed?");
                }
                return tail_file(access_log, lines, follow);
            }
            tail_file(log_file, lines, follow)?;
        }
        InitSystem::Auto => anyhow::bail!("InitSystem::Auto must be resolved to a concrete init system before reaching service control logic"),
    }
    let _ = config;
    Ok(())
}

fn logs_windows(config: &Config, lines: usize, follow: bool) -> Result<()> {
    let logs_dir = config
        .config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("logs");

    let stderr_log = logs_dir.join("daemon.stderr.log");
    let stdout_log = logs_dir.join("daemon.stdout.log");

    let log_file = if stderr_log.exists() {
        stderr_log
    } else if stdout_log.exists() {
        stdout_log
    } else {
        bail!(
            "No log files found in {}. Is the service installed?",
            logs_dir.display()
        );
    };

    let escaped_path = log_file.display().to_string().replace('\'', "''");
    let script = if follow {
        format!("Get-Content -Path '{escaped_path}' -Tail {lines} -Wait")
    } else {
        format!("Get-Content -Path '{escaped_path}' -Tail {lines}")
    };
    let encoded_command = {
        use base64::Engine as _;
        let utf16le: Vec<u8> = script
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        base64::engine::general_purpose::STANDARD.encode(utf16le)
    };
    let status = crate::util::hidden_sync_command("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded_command,
        ])
        .status()
        .context("Failed to run PowerShell Get-Content")?;
    if !status.success() {
        bail!("PowerShell Get-Content exited with non-zero status");
    }
    Ok(())
}

fn tail_file(path: &Path, lines: usize, follow: bool) -> Result<()> {
    let mut args = vec!["-n".to_string(), lines.to_string()];
    if follow {
        args.push("-f".to_string());
    }
    let status = crate::util::hidden_sync_command("tail")
        .args(&args)
        .arg(path)
        .status()
        .context("Failed to run tail")?;
    if !status.success() {
        bail!("tail exited with non-zero status");
    }
    Ok(())
}

fn uninstall(config: &Config, init_system: InitSystem) -> Result<()> {
    stop(config, init_system)?;

    if cfg!(target_os = "macos") {
        let file = macos_service_file()?;
        if file.exists() {
            fs::remove_file(&file)
                .with_context(|| format!("Failed to remove {}", file.display()))?;
        }
        println!("Service uninstalled ({})", file.display());
        return Ok(());
    }
    if cfg!(target_os = "linux") {
        let resolved = init_system.resolve()?;
        return uninstall_linux(config, resolved);
    }
    if cfg!(target_os = "windows") {
        let task_name = windows_task_name();
        let _ = run_checked(crate::util::hidden_sync_command("schtasks").args(["/Delete", "/TN", task_name, "/F"]));
        let wrapper = config
            .config_path
            .parent()
            .map_or_else(|| PathBuf::from("."), PathBuf::from)
            .join("logs")
            .join("sen-daemon.cmd");
        if wrapper.exists() {
            fs::remove_file(&wrapper).ok();
        }
        println!("Service uninstalled");
        return Ok(());
    }
    anyhow::bail!("Service management is supported on macOS and Linux only")
}

fn uninstall_linux(config: &Config, init_system: InitSystem) -> Result<()> {
    match init_system {
        InitSystem::Systemd => {
            let file = linux_service_file(config)?;
            if file.exists() {
                fs::remove_file(&file)
                    .with_context(|| format!("Failed to remove {}", file.display()))?;
            }
            let _ = run_checked(crate::util::hidden_sync_command("systemctl").args(["--user", "daemon-reload"]));
            println!("Service uninstalled ({})", file.display());
        }
        InitSystem::Openrc => {
            let init_script = Path::new("/etc/init.d/sen");
            if init_script.exists() {
                if let Err(err) =
                    run_checked(crate::util::hidden_sync_command("rc-update").args(["del", "sen", "default"]))
                {
                    eprintln!("Warning: Could not remove sen from OpenRC default runlevel: {err}");
                }
                fs::remove_file(init_script)
                    .with_context(|| format!("Failed to remove {}", init_script.display()))?;
            }
            println!("Service uninstalled (/etc/init.d/sen)");
        }
        InitSystem::Auto => anyhow::bail!("InitSystem::Auto must be resolved to a concrete init system before reaching service control logic"),
    }
    Ok(())
}

fn detect_homebrew_var_dir(exe: &Path) -> Option<PathBuf> {
    let path_str = exe.to_string_lossy();
    let prefix = if path_str.contains("/Cellar/") {
        let mut ancestor = exe.to_path_buf();
        while let Some(parent) = ancestor.parent() {
            ancestor = parent.to_path_buf();
            if ancestor.file_name().map_or(false, |n| n == "Cellar") {
                return ancestor.parent().map(|p| p.join("var").join("sen"));
            }
        }
        return None;
    } else if let Some(bin_parent) = exe.parent() {
        if let Some(prefix) = bin_parent.parent() {
            if prefix.join("Cellar").is_dir() {
                Some(prefix.to_path_buf())
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    prefix.map(|p| p.join("var").join("sen"))
}

fn install_macos(config: &Config) -> Result<()> {
    let file = macos_service_file()?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }

    let exe = std::env::current_exe().context("Failed to resolve current executable")?;

    let homebrew_var_dir = detect_homebrew_var_dir(&exe);
    if let Some(ref var_dir) = homebrew_var_dir {
        fs::create_dir_all(var_dir).with_context(|| {
            format!(
                "Failed to create Homebrew var directory: {}",
                var_dir.display()
            )
        })?;
    }

    let logs_dir = if let Some(ref var_dir) = homebrew_var_dir {
        var_dir.join("logs")
    } else {
        config
            .config_path
            .parent()
            .map_or_else(|| PathBuf::from("."), PathBuf::from)
            .join("logs")
    };
    fs::create_dir_all(&logs_dir)?;

    let stdout = logs_dir.join("daemon.stdout.log");
    let stderr = logs_dir.join("daemon.stderr.log");

    let env_section = if let Some(ref var_dir) = homebrew_var_dir {
        format!(
            r"  <key>EnvironmentVariables</key>
  <dict>
    <key>SEN_CONFIG_DIR</key>
    <string>{config_dir}</string>
  </dict>
  <key>WorkingDirectory</key>
  <string>{working_dir}</string>
",
            config_dir = xml_escape(&var_dir.display().to_string()),
            working_dir = xml_escape(&var_dir.display().to_string()),
        )
    } else {
        String::new()
    };

    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>daemon</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
{env_section}  <key>StandardOutPath</key>
  <string>{stdout}</string>
  <key>StandardErrorPath</key>
  <string>{stderr}</string>
</dict>
</plist>
"#,
        label = SERVICE_LABEL,
        exe = xml_escape(&exe.display().to_string()),
        env_section = env_section,
        stdout = xml_escape(&stdout.display().to_string()),
        stderr = xml_escape(&stderr.display().to_string())
    );

    fs::write(&file, plist)?;
    println!("Installed launchd service: {}", file.display());
    if let Some(ref var_dir) = homebrew_var_dir {
        println!("   Homebrew var: {}", var_dir.display());
    }
    println!("   Start with: sen service start");
    Ok(())
}

fn install_linux(config: &Config, init_system: InitSystem) -> Result<()> {
    match init_system {
        InitSystem::Systemd => install_linux_systemd(config),
        InitSystem::Openrc => install_linux_openrc(config),
        InitSystem::Auto => anyhow::bail!("InitSystem::Auto must be resolved to a concrete init system before reaching service control logic"),
    }
}

fn install_linux_systemd(config: &Config) -> Result<()> {
    let file = linux_service_file(config)?;
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }

    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let unit = format!(
        "[Unit]\n\
         Description=SenWeaverCoding daemon\n\
         After=network.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exe} daemon\n\
         Restart=always\n\
         RestartSec=3\n\
         Environment=HOME=%h\n\
         PassEnvironment=DISPLAY XDG_RUNTIME_DIR\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        exe = exe.display()
    );

    fs::write(&file, unit)?;
    let _ = run_checked(crate::util::hidden_sync_command("systemctl").args(["--user", "daemon-reload"]));
    let _ = run_checked(crate::util::hidden_sync_command("systemctl").args(["--user", "enable", "sen.service"]));
    println!("Installed systemd user service: {}", file.display());
    println!("   Start with: sen service start");
    Ok(())
}

#[cfg(unix)]
fn is_root() -> bool {

    #[allow(unsafe_code)]
    unsafe {
        libc::getuid() == 0
    }
}

#[cfg(not(unix))]
fn is_root() -> bool {
    false
}

fn check_sen_user() -> Result<()> {
    let output = crate::util::hidden_sync_command("getent").args(["passwd", "sen"]).output();
    let is_alpine = Path::new("/etc/alpine-release").exists();

    let (del_cmd, add_cmd) = if is_alpine {
        (
            "deluser sen && delgroup sen",
            "addgroup -S sen && adduser -S -s /sbin/nologin -H -D -G sen sen",
        )
    } else {
        ("userdel sen", "useradd -r -s /sbin/nologin sen")
    };

    match output {
        Ok(output) if output.status.success() => {
            let passwd_entry = String::from_utf8_lossy(&output.stdout);
            let parts: Vec<&str> = passwd_entry.split(':').collect();
            if parts.len() >= 7 {
                let uid = parts[2];
                let shell = parts[6];

                if uid.parse::<u32>().unwrap_or(999) >= 1000 {
                    bail!(
                        "User 'sen' exists but has unexpected UID {} (expected system UID < 1000).\n\
                         Recreate with: sudo {} && sudo {}",
                        uid,
                        del_cmd,
                        add_cmd
                    );
                }

                if !shell.contains("nologin") && !shell.contains("false") {
                    bail!(
                        "User 'sen' exists but has unexpected shell '{}'.\n\
                         Expected nologin/false for security. Fix with: sudo {} && sudo {}",
                        shell,
                        del_cmd,
                        add_cmd
                    );
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn ensure_sen_user() -> Result<()> {
    let output = crate::util::hidden_sync_command("getent").args(["passwd", "sen"]).output();
    if let Ok(output) = output {
        if output.status.success() {
            return check_sen_user();
        }
    }

    let is_alpine = Path::new("/etc/alpine-release").exists();

    if is_alpine {
        let group_output = crate::util::hidden_sync_command("getent").args(["group", "sen"]).output();
        let group_exists = group_output.map(|o| o.status.success()).unwrap_or(false);

        if !group_exists {
            let output = crate::util::hidden_sync_command("addgroup")
                .args(["-S", "sen"])
                .output()
                .context("Failed to create sen group")?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("Failed to create sen group: {}", stderr.trim());
            }
            println!("Created system group: sen");
        }

        let output = crate::util::hidden_sync_command("adduser")
            .args(["-S", "-s", "/sbin/nologin", "-H", "-D", "-G", "sen", "sen"])
            .output()
            .context("Failed to create sen user")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create sen user: {}", stderr.trim());
        }
    } else {
        let output = crate::util::hidden_sync_command("useradd")
            .args(["-r", "-s", "/sbin/nologin", "sen"])
            .output()
            .context("Failed to create sen user")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create sen user: {}", stderr.trim());
        }
    }

    println!("Created system user: sen");
    Ok(())
}

#[cfg(unix)]
fn chown_to_sen(path: &Path) -> Result<()> {
    let output = crate::util::hidden_sync_command("chown")
        .args(["sen:sen", &path.to_string_lossy()])
        .output()
        .context("Failed to run chown")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Failed to change ownership of {} to sen:sen: {}",
            path.display(),
            stderr.trim(),
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn chown_to_sen(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(unix)]
fn chown_recursive_to_sen(path: &Path) -> Result<()> {
    let output = crate::util::hidden_sync_command("chown")
        .args(["-R", "sen:sen", &path.to_string_lossy()])
        .output()
        .context("Failed to run recursive chown")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Failed to recursively change ownership of {} to sen:sen: {}",
            path.display(),
            stderr.trim(),
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn chown_recursive_to_sen(_path: &Path) -> Result<()> {
    Ok(())
}

fn copy_dir_recursive(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)
        .with_context(|| format!("Failed to create directory {}", target.display()))?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("Failed to read directory {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("Failed to inspect {}", source_path.display()))?;

        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if file_type.is_file() {
            if target_path.exists() {
                continue;
            }
            fs::copy(&source_path, &target_path).with_context(|| {
                format!(
                    "Failed to copy file {} -> {}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn resolve_invoking_user_config_dir() -> Option<PathBuf> {
    let sudo_user = std::env::var("SUDO_USER")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && value != "root");

    if let Some(user) = sudo_user {
        if let Ok(output) = crate::util::hidden_sync_command("getent").args(["passwd", &user]).output() {
            if output.status.success() {
                let entry = String::from_utf8_lossy(&output.stdout);
                let fields: Vec<&str> = entry.trim().split(':').collect();
                if fields.len() >= 6 {
                    return Some(PathBuf::from(fields[5]).join(".senweavercoding"));
                }
            }
        }
    }

    std::env::var("HOME")
        .ok()
        .map(PathBuf::from)
        .map(|home| home.join(".senweavercoding"))
}

fn migrate_openrc_runtime_state_if_needed(config_dir: &Path) -> Result<()> {
    let target_config = config_dir.join("config.toml");
    if target_config.exists() {
        println!(
            "Reusing existing OpenRC config at {}",
            target_config.display()
        );
        return Ok(());
    }

    let Some(source_dir) = resolve_invoking_user_config_dir() else {
        return Ok(());
    };

    let source_config = source_dir.join("config.toml");
    if !source_config.exists() {
        return Ok(());
    }

    copy_dir_recursive(&source_dir, config_dir)?;
    println!(
        "Migrated runtime state from {} to {}",
        source_dir.display(),
        config_dir.display()
    );
    Ok(())
}

#[cfg(unix)]
fn shell_single_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\"'\"'"))
}

#[cfg(unix)]
fn build_openrc_writability_probe_command(path: &Path, has_runuser: bool) -> (String, Vec<String>) {
    let probe = format!("test -w {}", shell_single_quote(&path.to_string_lossy()));
    if has_runuser {
        (
            "runuser".to_string(),
            vec![
                "-u".to_string(),
                "sen".to_string(),
                "--".to_string(),
                "sh".to_string(),
                "-c".to_string(),
                probe,
            ],
        )
    } else {
        (
            "su".to_string(),
            vec![
                "-s".to_string(),
                "/bin/sh".to_string(),
                "-c".to_string(),
                probe,
                "sen".to_string(),
            ],
        )
    }
}

#[cfg(unix)]
fn ensure_openrc_runtime_path_writable(path: &Path) -> Result<()> {
    let has_runuser = which::which("runuser").is_ok();
    let (program, args) = build_openrc_writability_probe_command(path, has_runuser);
    let output = crate::util::hidden_sync_command(&program)
        .args(args.iter().map(String::as_str))
        .output()
        .with_context(|| {
            format!(
                "Failed to verify OpenRC runtime write access for {}",
                path.display()
            )
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let details = if stderr.trim().is_empty() {
            "write-access probe failed"
        } else {
            stderr.trim()
        };
        bail!(
            "OpenRC runtime user 'sen' cannot write {} ({details}). \
             Re-run `sudo sen service install` and ensure ownership is sen:sen.",
            path.display(),
        );
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_openrc_runtime_dirs_writable(
    config_dir: &Path,
    workspace_dir: &Path,
    log_dir: &Path,
) -> Result<()> {
    for path in [config_dir, workspace_dir, log_dir] {
        ensure_openrc_runtime_path_writable(path)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_openrc_runtime_dirs_writable(
    _config_dir: &Path,
    _workspace_dir: &Path,
    _log_dir: &Path,
) -> Result<()> {
    Ok(())
}

fn warn_if_binary_in_home(exe_path: &Path) {
    let path_str = exe_path.to_string_lossy();
    if path_str.contains("/home/") || path_str.contains(".cargo/bin") {
        eprintln!(
            "Warning: Binary path '{}' appears to be in a user home directory.\n\
             For system-wide OpenRC service, consider installing to /usr/local/bin:\n\
             sudo cp '{}' /usr/local/bin/sen",
            exe_path.display(),
            exe_path.display()
        );
    }
}

fn generate_openrc_script(exe_path: &Path, config_dir: &Path) -> String {
    format!(
        r#"#!/sbin/openrc-run

name="sen"
description="SenWeaverCoding daemon"

command="{exe}"
command_args="--config-dir {config_dir} daemon"
command_background="yes"
command_user="sen:sen"
pidfile="/run/${{RC_SVCNAME}}.pid"
umask 027
output_log="/var/log/sen/access.log"
error_log="/var/log/sen/error.log"

export HOME="/var/lib/sen"

depend() {{
    need net
    after firewall
}}

start_pre() {{
    checkpath --directory --owner sen:sen --mode 0750 /var/lib/sen
}}
"#,
        exe = exe_path.display(),
        config_dir = config_dir.display(),
    )
}

fn resolve_openrc_executable() -> Result<PathBuf> {
    let preferred = Path::new("/usr/local/bin/sen");
    if preferred.exists() {
        return Ok(preferred.to_path_buf());
    }
    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    Ok(exe)
}

fn install_linux_openrc(config: &Config) -> Result<()> {
    if !is_root() {
        bail!(
            "OpenRC service installation requires root privileges.\n\
             Please run with sudo: sudo sen service install"
        );
    }

    ensure_sen_user()?;

    let exe = resolve_openrc_executable()?;
    warn_if_binary_in_home(&exe);

    let config_dir = Path::new("/etc/sen");
    let workspace_dir = config_dir.join("workspace");
    let log_dir = Path::new("/var/log/sen");

    if !config_dir.exists() {
        fs::create_dir_all(config_dir)
            .with_context(|| format!("Failed to create {}", config_dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(config_dir, fs::Permissions::from_mode(0o755)).with_context(
                || format!("Failed to set permissions on {}", config_dir.display()),
            )?;
        }
        println!("Created directory: {}", config_dir.display());
    }

    migrate_openrc_runtime_state_if_needed(config_dir)?;

    if !workspace_dir.exists() {
        fs::create_dir_all(&workspace_dir)
            .with_context(|| format!("Failed to create {}", workspace_dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&workspace_dir, fs::Permissions::from_mode(0o750)).with_context(
                || format!("Failed to set permissions on {}", workspace_dir.display()),
            )?;
        }
        chown_to_sen(&workspace_dir)?;
        println!(
            "Created directory: {} (owned by sen:sen)",
            workspace_dir.display()
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&workspace_dir, fs::Permissions::from_mode(0o750))
            .with_context(|| format!("Failed to set permissions on {}", workspace_dir.display()))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(config_dir, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Failed to set permissions on {}", config_dir.display()))?;
        let config_path = config_dir.join("config.toml");
        if config_path.exists() {
            fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)).with_context(
                || format!("Failed to set permissions on {}", config_path.display()),
            )?;
        }
        let secret_key_path = config_dir.join(".secret_key");
        if secret_key_path.exists() {
            fs::set_permissions(&secret_key_path, fs::Permissions::from_mode(0o600)).with_context(
                || format!("Failed to set permissions on {}", secret_key_path.display()),
            )?;
        }
    }

    chown_recursive_to_sen(config_dir)?;

    let created_log_dir = !log_dir.exists();
    if created_log_dir {
        fs::create_dir_all(log_dir)
            .with_context(|| format!("Failed to create {}", log_dir.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(log_dir, fs::Permissions::from_mode(0o750))
                .with_context(|| format!("Failed to set permissions on {}", log_dir.display()))?;
        }
    }

    chown_to_sen(log_dir)?;

    ensure_openrc_runtime_dirs_writable(config_dir, &workspace_dir, log_dir)?;

    if created_log_dir {
        println!(
            "Created directory: {} (owned by sen:sen)",
            log_dir.display()
        );
    }

    let init_script = generate_openrc_script(&exe, config_dir);
    let init_path = Path::new("/etc/init.d/sen");
    fs::write(init_path, init_script)
        .with_context(|| format!("Failed to write {}", init_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(init_path, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("Failed to set permissions on {}", init_path.display()))?;
    }

    run_checked(crate::util::hidden_sync_command("rc-update").args(["add", "sen", "default"]))?;
    println!("Installed OpenRC service: /etc/init.d/sen");
    println!("   Config path: /etc/sen/config.toml");
    println!("   Start with: sudo sen service start");
    let _ = config;
    Ok(())
}

fn install_windows(config: &Config) -> Result<()> {
    let exe = std::env::current_exe().context("Failed to resolve current executable")?;
    let logs_dir = config
        .config_path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
        .join("logs");
    fs::create_dir_all(&logs_dir)?;

    let wrapper = logs_dir.join("sen-daemon.cmd");
    let stdout_log = logs_dir.join("daemon.stdout.log");
    let stderr_log = logs_dir.join("daemon.stderr.log");

    let wrapper_content = format!(
        "@echo off\r\n\"{}\" daemon >>\"{}\" 2>>\"{}\"",
        exe.display(),
        stdout_log.display(),
        stderr_log.display()
    );
    fs::write(&wrapper, &wrapper_content)?;

    let task_name = windows_task_name();

    let _ = crate::util::hidden_sync_command("schtasks")
        .args(["/Delete", "/TN", task_name, "/F"])
        .output();

    run_checked(crate::util::hidden_sync_command("schtasks").args([
        "/Create",
        "/TN",
        task_name,
        "/SC",
        "ONLOGON",
        "/TR",
        &format!("\"{}\"", wrapper.display()),
        "/RL",
        "HIGHEST",
        "/F",
    ]))?;

    println!("Installed Windows scheduled task: {}", task_name);
    println!("   Wrapper: {}", wrapper.display());
    println!("   Logs: {}", logs_dir.display());
    println!("   Start with: sen service start");
    Ok(())
}

fn macos_service_file() -> Result<PathBuf> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("Could not find home directory")?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{SERVICE_LABEL}.plist")))
}

fn linux_service_file(config: &Config) -> Result<PathBuf> {
    let home = directories::UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .context("Could not find home directory")?;
    let _ = config;
    Ok(home
        .join(".config")
        .join("systemd")
        .join("user")
        .join("sen.service"))
}

fn run_checked(command: &mut Command) -> Result<()> {
    let output = command.output().context("Failed to spawn command")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Command failed: {}", stderr.trim());
    }
    Ok(())
}

fn run_capture(command: &mut Command) -> Result<String> {
    let output = command.output().context("Failed to spawn command")?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    if text.trim().is_empty() {
        text = String::from_utf8_lossy(&output.stderr).to_string();
    }
    Ok(text)
}

fn xml_escape(raw: &str) -> String {
    raw.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
