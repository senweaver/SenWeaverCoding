// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
use super::{SharedProcess, Tunnel, TunnelProcess, kill_shared, new_shared_process};
use anyhow::{Result, bail};
use tokio::io::AsyncBufReadExt;
pub struct PinggyTunnel {
    token: Option<String>,
    region: Option<String>,
    proc: SharedProcess,
}

impl PinggyTunnel {
    pub fn new(token: Option<String>, region: Option<String>) -> Self {
        Self {
            token,
            region,
            proc: new_shared_process(),
        }
    }
}

#[async_trait::async_trait]
impl Tunnel for PinggyTunnel {
    fn name(&self) -> &str {
        "pinggy"
    }

    async fn start(&self, local_host: &str, local_port: u16) -> Result<String> {

        let base = match self.token.as_deref() {
            Some(t) if !t.is_empty() => "pro.pinggy.io",
            _ => "free.pinggy.io",
        };
        let server_host = match self.region.as_deref() {
            Some(r) if !r.is_empty() => format!("{}.{base}", r.to_ascii_lowercase()),
            _ => base.into(),
        };

        let destination = match self.token.as_deref() {
            Some(t) if !t.is_empty() => format!("{t}@{server_host}"),
            _ => server_host,
        };

        let forward_spec = format!("0:{local_host}:{local_port}");

        let mut child = crate::util::hidden_async_command("ssh")
            .args([
                "-T",
                "-p",
                "443",
                "-R",
                &forward_spec,
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "ServerAliveInterval=30",
                &destination,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture pinggy stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture pinggy stderr"))?;

        let mut stdout_lines = tokio::io::BufReader::new(stdout).lines();
        let mut stderr_lines = tokio::io::BufReader::new(stderr).lines();
        let mut public_url = String::new();

        enum StreamLine {
            Stdout(std::io::Result<Option<String>>),
            Stderr(std::io::Result<Option<String>>),
        }

        let mut stdout_done = false;
        let mut stderr_done = false;

        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(15);
        while tokio::time::Instant::now() < deadline && !(stdout_done && stderr_done) {
            let stream_line = tokio::time::timeout(tokio::time::Duration::from_secs(3), async {
                tokio::select! {
                    biased;
                    l = stdout_lines.next_line(), if !stdout_done => StreamLine::Stdout(l),
                    l = stderr_lines.next_line(), if !stderr_done => StreamLine::Stderr(l),
                }
            })
            .await;

            match stream_line {
                Ok(StreamLine::Stdout(Ok(Some(l))) | StreamLine::Stderr(Ok(Some(l)))) => {
                    tracing::debug!("pinggy: {l}");

                    if let Some(idx) = l.find("https://") {
                        let url_part = &l[idx..];
                        let end = url_part
                            .find(|c: char| c.is_whitespace())
                            .unwrap_or(url_part.len());
                        let candidate = &url_part[..end];
                        if candidate.contains(".pinggy.link") {
                            public_url = candidate.to_string();
                            break;
                        }
                    }
                }
                Ok(StreamLine::Stdout(Ok(None))) => stdout_done = true,
                Ok(StreamLine::Stderr(Ok(None))) => stderr_done = true,
                Ok(StreamLine::Stdout(Err(e)) | StreamLine::Stderr(Err(e))) => {
                    bail!("Error reading pinggy output: {e}")
                }
                Err(_) => {}
            }
        }

        if public_url.is_empty() {
            child.kill().await.ok();
            child.wait().await.ok();
            bail!(
                "pinggy did not produce a public URL within 15s. Is SSH available and the token valid?"
            );
        }

        let mut guard = self.proc.lock().await;
        *guard = Some(TunnelProcess {
            child,
            public_url: public_url.clone(),
        });

        Ok(public_url)
    }

    async fn stop(&self) -> Result<()> {
        kill_shared(&self.proc).await
    }

    async fn health_check(&self) -> bool {
        let mut guard = self.proc.lock().await;
        match guard.as_mut() {
            Some(tp) => match tp.child.try_wait() {
                Ok(None) => true,
                Ok(Some(_)) | Err(_) => false,
            },
            None => false,
        }
    }

    fn public_url(&self) -> Option<String> {
        self.proc
            .try_lock()
            .ok()
            .and_then(|g| g.as_ref().map(|tp| tp.public_url.clone()))
    }
}
