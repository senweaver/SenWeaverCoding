// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub enum CliCategory {
    VersionControl,
    Language,
    PackageManager,
    Container,
    Build,
    Cloud,
    AiAgent,
    Productivity,
}

impl std::fmt::Display for CliCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VersionControl => write!(f, "Version Control"),
            Self::Language => write!(f, "Language"),
            Self::PackageManager => write!(f, "Package Manager"),
            Self::Container => write!(f, "Container"),
            Self::Build => write!(f, "Build"),
            Self::Cloud => write!(f, "Cloud"),
            Self::AiAgent => write!(f, "AI Agent"),
            Self::Productivity => write!(f, "Productivity"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiscoveredCli {
    pub name: String,
    pub path: PathBuf,
    pub version: Option<String>,
    pub category: CliCategory,
}

struct KnownCli {
    name: &'static str,
    version_args: &'static [&'static str],
    category: CliCategory,
}

const KNOWN_CLIS: &[KnownCli] = &[
    KnownCli {
        name: "git",
        version_args: &["--version"],
        category: CliCategory::VersionControl,
    },
    KnownCli {
        name: "python",
        version_args: &["--version"],
        category: CliCategory::Language,
    },
    KnownCli {
        name: "python3",
        version_args: &["--version"],
        category: CliCategory::Language,
    },
    KnownCli {
        name: "node",
        version_args: &["--version"],
        category: CliCategory::Language,
    },
    KnownCli {
        name: "npm",
        version_args: &["--version"],
        category: CliCategory::PackageManager,
    },
    KnownCli {
        name: "pip",
        version_args: &["--version"],
        category: CliCategory::PackageManager,
    },
    KnownCli {
        name: "pip3",
        version_args: &["--version"],
        category: CliCategory::PackageManager,
    },
    KnownCli {
        name: "docker",
        version_args: &["--version"],
        category: CliCategory::Container,
    },
    KnownCli {
        name: "cargo",
        version_args: &["--version"],
        category: CliCategory::Build,
    },
    KnownCli {
        name: "make",
        version_args: &["--version"],
        category: CliCategory::Build,
    },
    KnownCli {
        name: "kubectl",
        version_args: &["version", "--client", "--short"],
        category: CliCategory::Cloud,
    },
    KnownCli {
        name: "rustc",
        version_args: &["--version"],
        category: CliCategory::Language,
    },
    KnownCli {
        name: "claude",
        version_args: &["--version"],
        category: CliCategory::AiAgent,
    },
    KnownCli {
        name: "gemini",
        version_args: &["--version"],
        category: CliCategory::AiAgent,
    },
    KnownCli {
        name: "kilo",
        version_args: &["--version"],
        category: CliCategory::AiAgent,
    },
    KnownCli {
        name: "gws",
        version_args: &["--version"],
        category: CliCategory::Productivity,
    },
];

pub fn discover_cli_tools(additional: &[String], excluded: &[String]) -> Vec<DiscoveredCli> {
    let mut results = Vec::new();

    for known in KNOWN_CLIS {
        if excluded.iter().any(|e| e == known.name) {
            continue;
        }
        if let Some(cli) = probe_cli(known.name, known.version_args, known.category.clone()) {
            results.push(cli);
        }
    }

    for tool_name in additional {
        if excluded.iter().any(|e| e == tool_name) {
            continue;
        }

        if results.iter().any(|r| r.name == *tool_name) {
            continue;
        }
        if let Some(cli) = probe_cli(tool_name, &["--version"], CliCategory::Build) {
            results.push(cli);
        }
    }

    results
}

fn probe_cli(name: &str, version_args: &[&str], category: CliCategory) -> Option<DiscoveredCli> {

    let path = find_executable(name)?;

    let version = get_version(name, version_args);

    Some(DiscoveredCli {
        name: name.to_string(),
        path,
        version,
        category,
    })
}

const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn output_with_timeout(
    mut cmd: std::process::Command,
    timeout: std::time::Duration,
) -> Option<std::process::Output> {
    use std::io::Read;

    cmd.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());
    let mut child = cmd.spawn().ok()?;

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();
    Some(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn find_executable(name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let which_cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let which_cmd = "which";

    let mut cmd = crate::util::hidden_sync_command(which_cmd);
    cmd.arg(name);
    let output = output_with_timeout(cmd, PROBE_TIMEOUT)?;

    if !output.status.success() {
        return None;
    }

    let path_str = String::from_utf8_lossy(&output.stdout);
    let first_line = path_str.lines().next()?.trim();
    if first_line.is_empty() {
        return None;
    }
    Some(PathBuf::from(first_line))
}

fn get_version(name: &str, args: &[&str]) -> Option<String> {
    let mut cmd = crate::util::hidden_sync_command(name);
    cmd.args(args);
    let output = output_with_timeout(cmd, PROBE_TIMEOUT)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let version_text = if stdout.trim().is_empty() {
        stderr.trim().to_string()
    } else {
        stdout.trim().to_string()
    };

    let first_line = version_text.lines().next()?.trim().to_string();
    if first_line.is_empty() {
        None
    } else {
        Some(first_line)
    }
}
