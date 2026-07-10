// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::{SharedProcess, Tunnel, TunnelProcess, kill_shared, new_shared_process};
use anyhow::{Result, bail};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use tokio::io::AsyncBufReadExt;

const NGROK_MONITOR_INTERVAL_SECS: u64 = 30;
const NGROK_PROBE_TIMEOUT_SECS: u64 = 5;
const NGROK_MAX_CONSECUTIVE_PROBE_FAILURES: u32 = 3;

pub struct NgrokTunnel {
    auth_token: String,
    domain: Option<String>,
    proc: SharedProcess,
    active: Arc<AtomicBool>,
    local_port: Arc<AtomicU16>,
    monitor: parking_lot::Mutex<Option<crate::runtime::task_manager::TaskHandle>>,
}

impl NgrokTunnel {
    pub fn new(auth_token: String, domain: Option<String>) -> Self {
        Self {
            auth_token,
            domain,
            proc: new_shared_process(),
            active: Arc::new(AtomicBool::new(false)),
            local_port: Arc::new(AtomicU16::new(0)),
            monitor: parking_lot::Mutex::new(None),
        }
    }

    fn spawn_monitor(&self) {
        let proc = Arc::clone(&self.proc);
        let active = Arc::clone(&self.active);
        let local_port = Arc::clone(&self.local_port);
        let auth_token = self.auth_token.clone();
        let domain = self.domain.clone();

        let handle = crate::runtime::task_manager::spawn_supervised(
            "tunnel.ngrok.monitor",
            async move {
                let mut consecutive_failures: u32 = 0;
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(
                        NGROK_MONITOR_INTERVAL_SECS,
                    ))
                    .await;
                    if !active.load(Ordering::SeqCst) {
                        break;
                    }

                    let state = {
                        let mut guard = proc.lock().await;
                        guard.as_mut().map(|tp| {
                            (
                                matches!(tp.child.try_wait(), Ok(None)),
                                tp.public_url.clone(),
                            )
                        })
                    };

                    let needs_restart = match state {
                        None => {
                            tracing::warn!("ngrok: tunnel process missing; restarting");
                            true
                        }
                        Some((false, _)) => {
                            tracing::warn!("ngrok: tunnel process exited; restarting");
                            true
                        }
                        Some((true, url)) => {
                            if probe_public_url(&url).await {
                                consecutive_failures = 0;
                                false
                            } else {
                                consecutive_failures = consecutive_failures.saturating_add(1);
                                tracing::warn!(
                                    consecutive_failures,
                                    url = %url,
                                    "ngrok: public URL probe failed"
                                );
                                consecutive_failures >= NGROK_MAX_CONSECUTIVE_PROBE_FAILURES
                            }
                        }
                    };

                    if !needs_restart {
                        continue;
                    }
                    consecutive_failures = 0;

                    {
                        let mut guard = proc.lock().await;
                        if let Some(mut tp) = guard.take() {
                            tp.child.kill().await.ok();
                            tp.child.wait().await.ok();
                        }
                    }

                    if !active.load(Ordering::SeqCst) {
                        break;
                    }

                    let port = local_port.load(Ordering::SeqCst);
                    if port == 0 {
                        tracing::warn!("ngrok: no local port recorded; cannot restart tunnel");
                        continue;
                    }

                    match launch_ngrok(&auth_token, domain.as_deref(), port).await {
                        Ok(tp) => {
                            let url = tp.public_url.clone();
                            *proc.lock().await = Some(tp);
                            tracing::info!(url = %url, "ngrok: tunnel restarted");
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "ngrok: tunnel restart failed; will retry on next check"
                            );
                        }
                    }
                }
            },
        );

        let mut guard = self.monitor.lock();
        if let Some(previous) = guard.take() {
            previous.abort();
        }
        *guard = Some(handle);
    }
}

async fn launch_ngrok(
    auth_token: &str,
    domain: Option<&str>,
    local_port: u16,
) -> Result<TunnelProcess> {
    crate::util::hidden_async_command("ngrok")
        .args(["config", "add-authtoken", auth_token])
        .output()
        .await?;

    let mut args = vec!["http".to_string(), local_port.to_string()];
    if let Some(domain) = domain {
        args.push("--domain".into());
        args.push(domain.to_string());
    }

    args.push("--log".into());
    args.push("stdout".into());
    args.push("--log-format".into());
    args.push("logfmt".into());

    let mut child = crate::util::hidden_async_command("ngrok")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to capture ngrok stdout"))?;
    if let Some(stderr) = child.stderr.take() {
        crate::runtime::spawn_supervised("tunnel.ngrok.stderr_drain", async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "tunnel.ngrok", "{line}");
            }
        });
    }

    let mut reader = tokio::io::BufReader::new(stdout).lines();
    let mut public_url = String::new();

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
    while tokio::time::Instant::now() < deadline {
        let line =
            tokio::time::timeout(tokio::time::Duration::from_secs(3), reader.next_line()).await;

        match line {
            Ok(Ok(Some(l))) => {
                tracing::debug!("ngrok: {l}");

                if let Some(idx) = l.find("url=https://") {
                    let url_start = idx + 4;
                    let url_part = &l[url_start..];
                    let end = url_part
                        .find(|c: char| c.is_whitespace())
                        .unwrap_or(url_part.len());
                    public_url = url_part[..end].to_string();
                    break;
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(e)) => bail!("Error reading ngrok output: {e}"),
            Err(_) => {}
        }
    }

    if public_url.is_empty() {
        child.kill().await.ok();
        bail!("ngrok did not produce a public URL within 15s. Is the auth token valid?");
    }

    crate::runtime::spawn_supervised("tunnel.ngrok.stdout_drain", async move {
        while let Ok(Some(line)) = reader.next_line().await {
            tracing::debug!(target: "tunnel.ngrok", "{line}");
        }
    });

    Ok(TunnelProcess { child, public_url })
}

async fn probe_public_url(url: &str) -> bool {
    let client = crate::services::require_services()
        .proxy_runtime()
        .build_client("tunnel.ngrok");
    match client
        .get(url)
        .timeout(std::time::Duration::from_secs(NGROK_PROBE_TIMEOUT_SECS))
        .send()
        .await
    {
        Ok(resp) => {
            let status = resp.status();
            status.is_success()
                || status.is_redirection()
                || status.as_u16() == 401
                || status.as_u16() == 404
        }
        Err(e) => {
            tracing::debug!(error = %e, url = %url, "ngrok: probe request failed");
            false
        }
    }
}

#[async_trait::async_trait]
impl Tunnel for NgrokTunnel {
    fn name(&self) -> &str {
        "ngrok"
    }

    async fn start(&self, _local_host: &str, local_port: u16) -> Result<String> {
        let tunnel_process = launch_ngrok(&self.auth_token, self.domain.as_deref(), local_port).await?;
        let public_url = tunnel_process.public_url.clone();

        self.local_port.store(local_port, Ordering::SeqCst);
        {
            let mut guard = self.proc.lock().await;
            *guard = Some(tunnel_process);
        }
        self.active.store(true, Ordering::SeqCst);
        self.spawn_monitor();

        Ok(public_url)
    }

    async fn stop(&self) -> Result<()> {
        self.active.store(false, Ordering::SeqCst);
        if let Some(monitor) = self.monitor.lock().take() {
            monitor.abort();
        }
        kill_shared(&self.proc).await
    }

    async fn health_check(&self) -> bool {
        let url = {
            let mut guard = self.proc.lock().await;
            match guard.as_mut() {
                Some(tp) => {
                    if !matches!(tp.child.try_wait(), Ok(None)) {
                        return false;
                    }
                    tp.public_url.clone()
                }
                None => return false,
            }
        };
        probe_public_url(&url).await
    }

    fn public_url(&self) -> Option<String> {
        self.proc
            .try_lock()
            .ok()
            .and_then(|g| g.as_ref().map(|tp| tp.public_url.clone()))
    }
}
