// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
mod cloudflare;
mod custom;
mod ngrok;
mod none;
mod openvpn;
mod pinggy;
mod tailscale;

pub use cloudflare::CloudflareTunnel;
pub use custom::CustomTunnel;
pub use ngrok::NgrokTunnel;
pub use none::NoneTunnel;
pub use openvpn::OpenVpnTunnel;
pub use pinggy::PinggyTunnel;
pub use tailscale::TailscaleTunnel;

use crate::config::schema::{TailscaleTunnelConfig, TunnelConfig};
use anyhow::{Result, bail};
use std::sync::Arc;
use tokio::sync::Mutex;

#[async_trait::async_trait]
pub trait Tunnel: Send + Sync {

    fn name(&self) -> &str;

    async fn start(&self, local_host: &str, local_port: u16) -> Result<String>;

    async fn stop(&self) -> Result<()>;

    async fn health_check(&self) -> bool;

    fn public_url(&self) -> Option<String>;
}

pub(crate) struct TunnelProcess {
    pub child: tokio::process::Child,
    pub public_url: String,
}

pub(crate) type SharedProcess = Arc<Mutex<Option<TunnelProcess>>>;

pub(crate) fn new_shared_process() -> SharedProcess {
    Arc::new(Mutex::new(None))
}

pub(crate) async fn kill_shared(proc: &SharedProcess) -> Result<()> {
    let mut guard = proc.lock().await;
    if let Some(ref mut tp) = *guard {
        tp.child.kill().await.ok();
        tp.child.wait().await.ok();
    }
    *guard = None;
    Ok(())
}

pub fn create_tunnel(config: &TunnelConfig) -> Result<Option<Box<dyn Tunnel>>> {
    match config.provider.as_str() {
        "none" | "" => Ok(None),

        "cloudflare" => {
            let cf = config.cloudflare.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "tunnel.provider = \"cloudflare\" but [tunnel.cloudflare] section is missing"
                )
            })?;
            Ok(Some(Box::new(CloudflareTunnel::new(cf.token.clone()))))
        }

        "tailscale" => {
            let ts = config.tailscale.as_ref().unwrap_or(&TailscaleTunnelConfig {
                funnel: false,
                hostname: None,
            });
            Ok(Some(Box::new(TailscaleTunnel::new(
                ts.funnel,
                ts.hostname.clone(),
            ))))
        }

        "ngrok" => {
            let ng = config.ngrok.as_ref().ok_or_else(|| {
                anyhow::anyhow!("tunnel.provider = \"ngrok\" but [tunnel.ngrok] section is missing")
            })?;
            Ok(Some(Box::new(NgrokTunnel::new(
                ng.auth_token.clone(),
                ng.domain.clone(),
            ))))
        }

        "openvpn" => {
            let ov = config.openvpn.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "tunnel.provider = \"openvpn\" but [tunnel.openvpn] section is missing"
                )
            })?;
            Ok(Some(Box::new(OpenVpnTunnel::new(
                ov.config_file.clone(),
                ov.auth_file.clone(),
                ov.advertise_address.clone(),
                ov.connect_timeout_secs,
                ov.extra_args.clone(),
            ))))
        }

        "custom" => {
            let cu = config.custom.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "tunnel.provider = \"custom\" but [tunnel.custom] section is missing"
                )
            })?;
            Ok(Some(Box::new(CustomTunnel::new(
                cu.start_command.clone(),
                cu.health_url.clone(),
                cu.url_pattern.clone(),
            ))))
        }

        "pinggy" => {
            let pg = config.pinggy.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "tunnel.provider = \"pinggy\" but [tunnel.pinggy] section is missing"
                )
            })?;
            Ok(Some(Box::new(PinggyTunnel::new(
                pg.token.clone(),
                pg.region.clone(),
            ))))
        }

        other => bail!(
            "Unknown tunnel provider: \"{other}\". Valid: none, cloudflare, tailscale, ngrok, openvpn, pinggy, custom"
        ),
    }
}

