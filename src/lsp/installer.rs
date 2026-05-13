// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
//! Managed installer for the built-in LSP server templates.
//!
//! Recipe coverage matches the desktop UI:
//!
//! - `rust-analyzer` — fetched as a `gz`-compressed binary from the
//!   GitHub Releases of `rust-lang/rust-analyzer`.
//! - `typescript-language-server` — installed via `npm` into a private
//!   prefix; user must have `node` + `npm` on `PATH`.
//! - `pyright` — same npm strategy as typescript-language-server.
//!
//! Each recipe has a *PATH fallback*: if the user already has the
//! requested binary on `PATH` (e.g. because they installed it through
//! `cargo install`, system package manager, or `volta`), the installer
//! short-circuits the download step and points at the existing binary.
//! Progress is reported through the [`InstallProgress`] callback the
//! caller provides; the [`crate::lsp::manager::LspManager`] hooks this
//! into the gateway broadcast so the desktop UI gets streaming updates.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use directories::UserDirs;
use futures_util::StreamExt;
use serde::Deserialize;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::config::schema::LspInstallState;
use crate::lsp::events::InstallPhase;

pub type InstallProgress = Arc<dyn Fn(InstallPhase) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct InstallReport {
    pub server_id: String,
    pub version: String,

    pub binary_path: PathBuf,

    pub default_args: Vec<String>,
}

pub async fn install(server_id: &str, progress: InstallProgress) -> Result<InstallReport> {
    match server_id {
        "rust-analyzer" => install_rust_analyzer(progress).await,
        "typescript-language-server" => install_npm_server(
            "typescript-language-server",
            &["typescript-language-server", "typescript"],
            "typescript-language-server",
            &["--stdio"],
            progress,
        )
        .await,
        "pyright" => install_npm_server(
            "pyright",
            &["pyright"],
            "pyright-langserver",
            &["--stdio"],
            progress,
        )
        .await,
        "gopls" => install_gopls(progress).await,
        "bash-language-server" => install_npm_server(
            "bash-language-server",
            &["bash-language-server"],
            "bash-language-server",
            &["start"],
            progress,
        )
        .await,
        "yaml-language-server" => install_npm_server(
            "yaml-language-server",
            &["yaml-language-server"],
            "yaml-language-server",
            &["--stdio"],
            progress,
        )
        .await,
        "vscode-html-language-server" => install_npm_server(
            "vscode-html-language-server",
            &["vscode-langservers-extracted"],
            "vscode-html-language-server",
            &["--stdio"],
            progress,
        )
        .await,
        "vscode-css-language-server" => install_npm_server(
            "vscode-css-language-server",
            &["vscode-langservers-extracted"],
            "vscode-css-language-server",
            &["--stdio"],
            progress,
        )
        .await,
        "vscode-json-language-server" => install_npm_server(
            "vscode-json-language-server",
            &["vscode-langservers-extracted"],
            "vscode-json-language-server",
            &["--stdio"],
            progress,
        )
        .await,
        "clangd" => install_path_only(
            "clangd",
            "clangd",
            &[],
            "clangd is not bundled; install it via your platform package manager (apt, brew, winget, MSYS2, LLVM release) and re-try",
            progress,
        )
        .await,
        other => Err(anyhow!(
            "no managed install recipe for `{other}`; switch the entry to manual mode and provide a command"
        )),
    }
}

async fn install_gopls(progress: InstallProgress) -> Result<InstallReport> {
    progress(InstallPhase::Resolving {
        message: "checking PATH for gopls".into(),
    });

    if let Some(existing) = which_on_path("gopls") {
        let version = run_version_query(&existing, &["version"])
            .await
            .or_else(|| None);
        let report = InstallReport {
            server_id: "gopls".into(),
            version: version.unwrap_or_else(|| "system".into()),
            binary_path: existing.clone(),
            default_args: Vec::new(),
        };
        progress(InstallPhase::Done {
            version: report.version.clone(),
            path: report.binary_path.to_string_lossy().to_string(),
        });
        return Ok(report);
    }

    let go = which_on_path("go").ok_or_else(|| {
        anyhow!(
            "managed install for `gopls` requires the Go toolchain (`go` on PATH); \
             install Go from https://go.dev/dl/ or switch to manual mode and \
             point the entry at an existing gopls binary"
        )
    })?;

    progress(InstallPhase::Resolving {
        message: "preparing GOBIN for gopls".into(),
    });
    let install_dir = managed_dir()?.join("gopls");
    fs::create_dir_all(&install_dir)
        .await
        .with_context(|| format!("create {}", install_dir.display()))?;

    progress(InstallPhase::Downloading {
        percent: None,
        bytes_downloaded: 0,
        bytes_total: None,
    });

    let mut cmd = crate::util::hidden_async_command(&go);
    cmd.arg("install").arg("golang.org/x/tools/gopls@latest");
    cmd.env("GOBIN", &install_dir);
    cmd.kill_on_drop(true);

    let output = cmd
        .output()
        .await
        .with_context(|| "run `go install golang.org/x/tools/gopls@latest`".to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "`go install gopls` failed (status {}): {}",
            output.status,
            stderr.trim()
        ));
    }

    let bin_name = if cfg!(windows) { "gopls.exe" } else { "gopls" };
    let bin_path = install_dir.join(bin_name);
    if !fs::try_exists(&bin_path).await.unwrap_or(false) {
        return Err(anyhow!(
            "`go install gopls` completed but {} is missing",
            bin_path.display()
        ));
    }
    set_executable(&bin_path).await?;

    progress(InstallPhase::Verifying {
        message: format!("running `{} version`", bin_path.display()),
    });
    let version = run_version_query(&bin_path, &["version"])
        .await
        .unwrap_or_else(|| "managed".into());

    let report = InstallReport {
        server_id: "gopls".into(),
        version: version.clone(),
        binary_path: bin_path.clone(),
        default_args: Vec::new(),
    };

    progress(InstallPhase::Done {
        version,
        path: bin_path.to_string_lossy().to_string(),
    });

    Ok(report)
}

async fn install_path_only(
    server_id: &str,
    bin_name: &str,
    default_args: &[&str],
    miss_message: &str,
    progress: InstallProgress,
) -> Result<InstallReport> {
    progress(InstallPhase::Resolving {
        message: format!("checking PATH for {bin_name}"),
    });
    let Some(existing) = which_on_path(bin_name) else {
        return Err(anyhow!("{miss_message}"));
    };
    let version = run_version_query(&existing, &["--version"]).await;
    let report = InstallReport {
        server_id: server_id.into(),
        version: version.unwrap_or_else(|| "system".into()),
        binary_path: existing.clone(),
        default_args: default_args.iter().map(|s| (*s).to_string()).collect(),
    };
    progress(InstallPhase::Done {
        version: report.version.clone(),
        path: report.binary_path.to_string_lossy().to_string(),
    });
    Ok(report)
}

async fn install_rust_analyzer(progress: InstallProgress) -> Result<InstallReport> {
    progress(InstallPhase::Resolving {
        message: "checking PATH for rust-analyzer".into(),
    });

    if let Some(existing) = which_on_path("rust-analyzer") {
        let version = run_version_query(&existing, &["--version"]).await;
        let report = InstallReport {
            server_id: "rust-analyzer".into(),
            version: version.unwrap_or_else(|| "system".into()),
            binary_path: existing.clone(),
            default_args: Vec::new(),
        };
        progress(InstallPhase::Done {
            version: report.version.clone(),
            path: report.binary_path.to_string_lossy().to_string(),
        });
        return Ok(report);
    }

    progress(InstallPhase::Resolving {
        message: "fetching latest rust-analyzer release info".into(),
    });
    let release = fetch_latest_release("rust-lang", "rust-analyzer").await?;

    let asset_name = rust_analyzer_asset_name()?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(&asset_name))
        .ok_or_else(|| {
            anyhow!(
                "no `{asset_name}` asset in rust-analyzer release `{tag}`",
                tag = release.tag_name,
            )
        })?;

    let install_dir = managed_dir()?
        .join("rust-analyzer")
        .join(&release.tag_name);
    fs::create_dir_all(&install_dir)
        .await
        .with_context(|| format!("create {}", install_dir.display()))?;

    let archive_path = install_dir.join(&asset.name);
    download_with_progress(&asset.browser_download_url, &archive_path, &progress).await?;

    progress(InstallPhase::Extracting {
        message: format!("extracting {}", asset.name),
    });
    let executable = if asset_name.ends_with(".gz") {
        let out_name = if cfg!(windows) {
            "rust-analyzer.exe"
        } else {
            "rust-analyzer"
        };
        let out_path = install_dir.join(out_name);
        gunzip_to(&archive_path, &out_path).await?;
        out_path
    } else if asset_name.ends_with(".zip") {
        let extracted = unzip_first_executable(&archive_path, &install_dir).await?;
        extracted
    } else {
        archive_path.clone()
    };

    set_executable(&executable).await?;

    progress(InstallPhase::Verifying {
        message: format!("running `{} --version`", executable.display()),
    });
    let version = run_version_query(&executable, &["--version"])
        .await
        .unwrap_or_else(|| release.tag_name.clone());

    let report = InstallReport {
        server_id: "rust-analyzer".into(),
        version: version.clone(),
        binary_path: executable.clone(),
        default_args: Vec::new(),
    };

    progress(InstallPhase::Done {
        version,
        path: executable.to_string_lossy().to_string(),
    });

    Ok(report)
}

fn rust_analyzer_asset_name() -> Result<String> {
    let target = if cfg!(target_os = "windows") {
        if cfg!(target_arch = "x86_64") {
            "rust-analyzer-x86_64-pc-windows-msvc.zip"
        } else if cfg!(target_arch = "aarch64") {
            "rust-analyzer-aarch64-pc-windows-msvc.zip"
        } else {
            return Err(anyhow!("unsupported windows architecture for rust-analyzer"));
        }
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "x86_64") {
            "rust-analyzer-x86_64-apple-darwin.gz"
        } else if cfg!(target_arch = "aarch64") {
            "rust-analyzer-aarch64-apple-darwin.gz"
        } else {
            return Err(anyhow!("unsupported macOS architecture for rust-analyzer"));
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "x86_64") {
            "rust-analyzer-x86_64-unknown-linux-gnu.gz"
        } else if cfg!(target_arch = "aarch64") {
            "rust-analyzer-aarch64-unknown-linux-gnu.gz"
        } else {
            return Err(anyhow!("unsupported linux architecture for rust-analyzer"));
        }
    } else {
        return Err(anyhow!("unsupported OS for rust-analyzer managed install"));
    };
    Ok(target.to_string())
}

async fn install_npm_server(
    server_id: &str,
    npm_packages: &[&str],
    bin_name: &str,
    default_args: &[&str],
    progress: InstallProgress,
) -> Result<InstallReport> {
    progress(InstallPhase::Resolving {
        message: format!("checking PATH for {bin_name}"),
    });
    if let Some(existing) = which_on_path(bin_name) {
        let version = run_version_query(&existing, &["--version"]).await;
        let report = InstallReport {
            server_id: server_id.into(),
            version: version.unwrap_or_else(|| "system".into()),
            binary_path: existing,
            default_args: default_args.iter().map(|s| (*s).to_string()).collect(),
        };
        progress(InstallPhase::Done {
            version: report.version.clone(),
            path: report.binary_path.to_string_lossy().to_string(),
        });
        return Ok(report);
    }

    let npm = which_on_path("npm").ok_or_else(|| {
        anyhow!(
            "managed install for `{server_id}` requires `node` + `npm` on PATH; install Node.js or switch to manual mode"
        )
    })?;

    progress(InstallPhase::Resolving {
        message: format!("preparing prefix for {server_id}"),
    });
    let prefix = managed_dir()?.join(server_id);
    fs::create_dir_all(&prefix)
        .await
        .with_context(|| format!("create {}", prefix.display()))?;

    progress(InstallPhase::Downloading {
        percent: None,
        bytes_downloaded: 0,
        bytes_total: None,
    });

    let mut cmd = crate::util::hidden_async_command(&npm);
    cmd.arg("install")
        .arg("--prefix")
        .arg(&prefix)
        .arg("--no-audit")
        .arg("--no-fund")
        .arg("--silent");
    for pkg in npm_packages {
        cmd.arg(*pkg);
    }
    cmd.kill_on_drop(true);

    let output = cmd
        .output()
        .await
        .with_context(|| format!("run npm install --prefix {}", prefix.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "npm install failed (status {}): {}",
            output.status,
            stderr.trim()
        ));
    }

    progress(InstallPhase::Verifying {
        message: format!("locating {bin_name} in install prefix"),
    });
    let bin_path = locate_npm_bin(&prefix, bin_name).await.ok_or_else(|| {
        anyhow!(
            "npm install completed but `{bin_name}` not found under {}",
            prefix.display()
        )
    })?;
    set_executable(&bin_path).await?;

    let version = run_version_query(&bin_path, &["--version"])
        .await
        .unwrap_or_else(|| "managed".into());

    let report = InstallReport {
        server_id: server_id.into(),
        version: version.clone(),
        binary_path: bin_path.clone(),
        default_args: default_args.iter().map(|s| (*s).to_string()).collect(),
    };

    progress(InstallPhase::Done {
        version,
        path: bin_path.to_string_lossy().to_string(),
    });

    Ok(report)
}

async fn locate_npm_bin(prefix: &Path, bin_name: &str) -> Option<PathBuf> {
    let candidates = if cfg!(windows) {
        vec![
            prefix.join(format!("{bin_name}.cmd")),
            prefix.join(format!("{bin_name}.ps1")),
            prefix.join(bin_name),
            prefix.join("node_modules").join(".bin").join(format!("{bin_name}.cmd")),
            prefix.join("node_modules").join(".bin").join(bin_name),
        ]
    } else {
        vec![
            prefix.join("bin").join(bin_name),
            prefix.join("node_modules").join(".bin").join(bin_name),
        ]
    };
    for cand in candidates {
        if fs::try_exists(&cand).await.unwrap_or(false) {
            return Some(cand);
        }
    }
    None
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    #[allow(dead_code)]
    size: u64,
}

async fn fetch_latest_release(owner: &str, repo: &str) -> Result<GithubRelease> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let client = build_http_client()?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "senagentos-cli/lsp-installer")
        .send()
        .await
        .with_context(|| format!("fetch {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "GitHub release lookup failed: {} {}",
            resp.status(),
            url
        ));
    }
    let release: GithubRelease = resp
        .json()
        .await
        .context("decode GitHub release response")?;
    Ok(release)
}

async fn download_with_progress(
    url: &str,
    dest: &Path,
    progress: &InstallProgress,
) -> Result<()> {
    let client = build_http_client()?;
    let resp = client
        .get(url)
        .header("User-Agent", "senagentos-cli/lsp-installer")
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("download failed: {} {}", resp.status(), url));
    }
    let total = resp.content_length();
    let mut downloaded: u64 = 0;
    let mut last_pct: i32 = -1;
    let mut stream = resp.bytes_stream();
    let mut file = fs::File::create(dest)
        .await
        .with_context(|| format!("create {}", dest.display()))?;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.with_context(|| format!("download chunk for {url}"))?;
        file.write_all(&bytes)
            .await
            .with_context(|| format!("write {}", dest.display()))?;
        downloaded += bytes.len() as u64;
        let pct = total.map(|t| ((downloaded * 100) / t.max(1)) as i32);
        if let Some(p) = pct {
            if p != last_pct {
                last_pct = p;
                progress(InstallPhase::Downloading {
                    percent: Some(p.clamp(0, 100) as u8),
                    bytes_downloaded: downloaded,
                    bytes_total: total,
                });
            }
        } else {

            if downloaded.is_multiple_of(256 * 1024) {
                progress(InstallPhase::Downloading {
                    percent: None,
                    bytes_downloaded: downloaded,
                    bytes_total: None,
                });
            }
        }
    }
    file.flush().await.ok();
    Ok(())
}

async fn gunzip_to(src: &Path, dest: &Path) -> Result<()> {
    let bytes = fs::read(src)
        .await
        .with_context(|| format!("read {}", src.display()))?;
    let dest_owned = dest.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        use flate2::read::GzDecoder;
        use std::io::{BufWriter, Read, Write};
        let mut decoder = GzDecoder::new(bytes.as_slice());
        let file = std::fs::File::create(&dest_owned)
            .with_context(|| format!("create {}", dest_owned.display()))?;
        let mut writer = BufWriter::new(file);
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = decoder.read(&mut buf)?;
            if n == 0 {
                break;
            }
            writer.write_all(&buf[..n])?;
        }
        writer.flush()?;
        Ok(())
    })
    .await
    .context("gunzip task panicked")??;
    Ok(())
}

async fn unzip_first_executable(archive: &Path, install_dir: &Path) -> Result<PathBuf> {
    let archive_owned = archive.to_path_buf();
    let dir_owned = install_dir.to_path_buf();
    let result = tokio::task::spawn_blocking(move || -> Result<PathBuf> {
        let file = std::fs::File::open(&archive_owned)
            .with_context(|| format!("open {}", archive_owned.display()))?;
        let mut zip = zip::ZipArchive::new(file).context("decode zip archive")?;
        let mut found: Option<PathBuf> = None;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            if entry.is_dir() {
                continue;
            }
            let name = entry.name().to_string();

            let lower = name.to_ascii_lowercase();
            let is_target = lower.ends_with("rust-analyzer.exe") || lower.ends_with("rust-analyzer");
            let dest = dir_owned.join(
                std::path::Path::new(&name)
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new(&name)),
            );
            let mut out = std::fs::File::create(&dest)
                .with_context(|| format!("create {}", dest.display()))?;
            std::io::copy(&mut entry, &mut out)?;
            if is_target {
                found = Some(dest);
            }
        }
        found.ok_or_else(|| anyhow!("no rust-analyzer entry in archive"))
    })
    .await
    .context("unzip task panicked")??;
    Ok(result)
}

#[cfg(unix)]
async fn set_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)
        .await
        .with_context(|| format!("stat {}", path.display()))?
        .permissions();
    perms.set_mode(perms.mode() | 0o755);
    fs::set_permissions(path, perms)
        .await
        .with_context(|| format!("chmod {}", path.display()))?;
    Ok(())
}

#[cfg(not(unix))]
async fn set_executable(_path: &Path) -> Result<()> {
    Ok(())
}

async fn run_version_query(bin: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = crate::util::hidden_async_command(bin);
    for a in args {
        cmd.arg(*a);
    }
    cmd.kill_on_drop(true);
    let output = cmd.output().await.ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines().next().map(|s| s.trim().to_string())
}

fn which_on_path(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("build reqwest client")
}

pub fn managed_dir() -> Result<PathBuf> {
    let home = UserDirs::new()
        .map(|u| u.home_dir().to_path_buf())
        .ok_or_else(|| anyhow!("cannot resolve home directory for LSP install"))?;
    Ok(home.join(".senweavercoding").join("lsp"))
}

impl From<&InstallReport> for LspInstallState {
    fn from(report: &InstallReport) -> Self {
        LspInstallState::Installed {
            version: report.version.clone(),
            path: report.binary_path.to_string_lossy().to_string(),
        }
    }
}
