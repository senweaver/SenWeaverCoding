// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::path::PathBuf;
use std::sync::Arc;

use super::schema::Config;

#[cfg(feature = "fs-watch")]
fn hash_file(path: &std::path::Path) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

#[cfg(feature = "fs-watch")]
pub fn spawn_config_file_watcher(
    config_path: PathBuf,
    live: crate::config::live::LiveConfig,
    app_config: Arc<parking_lot::Mutex<Config>>,
) -> Option<crate::runtime::TaskHandle> {
    use notify::{RecommendedWatcher, RecursiveMode, Watcher};

    let parent = config_path.parent()?.to_path_buf();
    let file_name = config_path.file_name()?.to_os_string();
    if !parent.exists() {
        return None;
    }

    Some(crate::runtime::task_manager::spawn_supervised(
        "config.file_watch",
        async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();
            let watch_name = file_name.clone();
            let mut watcher = match RecommendedWatcher::new(
                move |res: notify::Result<notify::Event>| {
                    if let Ok(ev) = res {
                        let relevant = ev
                            .paths
                            .iter()
                            .any(|p| p.file_name() == Some(watch_name.as_os_str()));
                        if relevant {
                            let _ = tx.send(());
                        }
                    }
                },
                notify::Config::default()
                    .with_poll_interval(std::time::Duration::from_millis(500)),
            ) {
                Ok(w) => w,
                Err(e) => {
                    tracing::warn!(
                        target: "config.file_watch",
                        error = %e,
                        "config file watcher unavailable; external config.toml edits require restart"
                    );
                    return;
                }
            };
            if let Err(e) = watcher.watch(&parent, RecursiveMode::NonRecursive) {
                tracing::warn!(
                    target: "config.file_watch",
                    path = %parent.display(),
                    error = %e,
                    "cannot watch config directory; external config.toml edits require restart"
                );
                return;
            }
            tracing::debug!(
                target: "config.file_watch",
                path = %config_path.display(),
                "watching config file for external edits"
            );

            let mut last_hash = hash_file(&config_path);
            loop {
                if rx.recv().await.is_none() {
                    break;
                }
                loop {
                    match tokio::time::timeout(
                        std::time::Duration::from_millis(400),
                        rx.recv(),
                    )
                    .await
                    {
                        Ok(Some(())) => continue,
                        Ok(None) => return,
                        Err(_) => break,
                    }
                }
                let new_hash = hash_file(&config_path);
                if new_hash.is_none() || new_hash == last_hash {
                    continue;
                }
                last_hash = new_hash;
                match Config::load_or_init().await {
                    Ok(new_config) => match live.store_validated(new_config.clone()) {
                        Ok(()) => {
                            *app_config.lock() = new_config;
                            tracing::info!(
                                target: "config.file_watch",
                                path = %config_path.display(),
                                "config.toml reloaded after external edit"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(
                                target: "config.file_watch",
                                error = %e,
                                "edited config.toml failed validation; keeping previous config"
                            );
                        }
                    },
                    Err(e) => {
                        tracing::warn!(
                            target: "config.file_watch",
                            error = %e,
                            "failed to reload edited config.toml; keeping previous config"
                        );
                    }
                }
            }
            drop(watcher);
        },
    ))
}

#[cfg(not(feature = "fs-watch"))]
pub fn spawn_config_file_watcher(
    _config_path: PathBuf,
    _live: crate::config::live::LiveConfig,
    _app_config: Arc<parking_lot::Mutex<Config>>,
) -> Option<crate::runtime::TaskHandle> {
    None
}
