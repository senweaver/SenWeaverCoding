// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod docker;
pub mod native;
pub mod task_manager;
pub mod traits;
#[cfg(feature = "runtime-wasm")]
pub mod wasm;

pub use docker::DockerRuntime;
pub use native::NativeRuntime;
pub use task_manager::{TaskHandle, panic_message, spawn_supervised, spawn_supervised_restartable};
pub use traits::RuntimeAdapter;
#[cfg(feature = "runtime-wasm")]
pub use wasm::WasmRuntime;

use crate::config::RuntimeConfig;

pub fn create_runtime(config: &RuntimeConfig) -> anyhow::Result<Box<dyn RuntimeAdapter>> {
    match config.kind.as_str() {
        "native" => Ok(Box::new(NativeRuntime::new())),
        "docker" => Ok(Box::new(DockerRuntime::new(config.docker.clone()))),
        "wasm" => {
            #[cfg(feature = "runtime-wasm")]
            {
                let rt = wasm::WasmRuntime::new(config.wasm.clone());
                rt.validate_config()?;
                Ok(Box::new(rt))
            }
            #[cfg(not(feature = "runtime-wasm"))]
            {
                anyhow::bail!(
                    "runtime.kind='wasm' requires the `runtime-wasm` feature. \
                     Rebuild with `cargo build --features runtime-wasm` to enable WASM sandbox support."
                )
            }
        }
        "cloudflare" => {
            if std::env::var("SEN_CF_EXPERIMENTAL").is_ok() {
                tracing::warn!(
                    "Cloudflare Workers runtime is experimental. Falling back to native runtime."
                );
                Ok(Box::new(NativeRuntime::new()))
            } else {
                anyhow::bail!(
                    "runtime.kind='cloudflare' is experimental and requires SEN_CF_EXPERIMENTAL=1 env var. \
                     The Cloudflare Workers runtime deploys the agent as a Worker with D1 storage. \
                     Set SEN_CF_EXPERIMENTAL=1 to enable with native runtime fallback, or use runtime.kind='native'."
                )
            }
        }
        other if other.trim().is_empty() => {
            anyhow::bail!("runtime.kind cannot be empty. Supported values: native, docker, wasm")
        }
        other => {
            anyhow::bail!("Unknown runtime kind '{other}'. Supported values: native, docker, wasm")
        }
    }
}
