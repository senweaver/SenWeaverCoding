// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
pub mod docker;
pub mod native;
pub mod traits;
#[cfg(feature = "runtime-wasm")]
pub mod wasm;

pub use docker::DockerRuntime;
pub use native::NativeRuntime;
pub use traits::RuntimeAdapter;
#[cfg(feature = "runtime-wasm")]
pub use wasm::WasmRuntime;

use crate::config::RuntimeConfig;

/// Factory: create the right runtime from config
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
        other => anyhow::bail!("Unknown runtime kind '{other}'. Supported values: native, docker, wasm"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_native() {
        let cfg = RuntimeConfig {
            kind: "native".into(),
            ..RuntimeConfig::default()
        };
        let rt = create_runtime(&cfg).unwrap();
        assert_eq!(rt.name(), "native");
        assert!(rt.has_shell_access());
    }

    #[test]
    fn factory_docker() {
        let cfg = RuntimeConfig {
            kind: "docker".into(),
            ..RuntimeConfig::default()
        };
        let rt = create_runtime(&cfg).unwrap();
        assert_eq!(rt.name(), "docker");
        assert!(rt.has_shell_access());
    }

    #[test]
    fn factory_cloudflare_errors_without_env() {
        // SAFETY: test-only, single-threaded access to env var
        unsafe { std::env::remove_var("SEN_CF_EXPERIMENTAL") };
        let cfg = RuntimeConfig {
            kind: "cloudflare".into(),
            ..RuntimeConfig::default()
        };
        match create_runtime(&cfg) {
            Err(err) => assert!(err.to_string().contains("experimental")),
            Ok(_) => panic!("cloudflare runtime should error without env var"),
        }
    }

    #[test]
    fn factory_cloudflare_falls_back_with_env() {
        // SAFETY: test-only, single-threaded access to env var
        unsafe { std::env::set_var("SEN_CF_EXPERIMENTAL", "1") };
        let cfg = RuntimeConfig {
            kind: "cloudflare".into(),
            ..RuntimeConfig::default()
        };
        let rt = create_runtime(&cfg).expect("cloudflare with env var should succeed");
        assert_eq!(rt.name(), "native");
        // SAFETY: test-only cleanup
        unsafe { std::env::remove_var("SEN_CF_EXPERIMENTAL") };
    }

    #[test]
    fn factory_unknown_errors() {
        let cfg = RuntimeConfig {
            kind: "wasm-edge-unknown".into(),
            ..RuntimeConfig::default()
        };
        match create_runtime(&cfg) {
            Err(err) => assert!(err.to_string().contains("Unknown runtime kind")),
            Ok(_) => panic!("unknown runtime should error"),
        }
    }

    #[test]
    fn factory_empty_errors() {
        let cfg = RuntimeConfig {
            kind: String::new(),
            ..RuntimeConfig::default()
        };
        match create_runtime(&cfg) {
            Err(err) => assert!(err.to_string().contains("cannot be empty")),
            Ok(_) => panic!("empty runtime should error"),
        }
    }

    #[test]
    fn factory_wasm() {
        let cfg = RuntimeConfig {
            kind: "wasm".into(),
            ..RuntimeConfig::default()
        };
        let result = create_runtime(&cfg);
        if cfg!(feature = "runtime-wasm") {
            let rt = result.expect("wasm factory should succeed with feature");
            assert_eq!(rt.name(), "wasm");
            assert!(!rt.has_shell_access());
        } else {
            match result {
                Err(err) => assert!(err.to_string().contains("runtime-wasm")),
                Ok(_) => panic!("wasm without feature flag should error"),
            }
        }
    }

    #[test]
    fn factory_wasm_rejects_invalid_config() {
        let mut cfg = RuntimeConfig {
            kind: "wasm".into(),
            ..RuntimeConfig::default()
        };
        cfg.wasm.memory_limit_mb = 0;
        let result = create_runtime(&cfg);
        if cfg!(feature = "runtime-wasm") {
            match result {
                Err(err) => assert!(err.to_string().contains("must be > 0")),
                Ok(_) => panic!("invalid wasm config should error"),
            }
        }
    }
}
