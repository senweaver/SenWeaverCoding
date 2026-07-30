// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::broadcast;

use super::schema::Config;

fn validate_full(config: &Config) -> Result<(), String> {
    config.validate().map_err(|err| format!("{err:#}"))
}

#[derive(Debug, Clone)]
pub struct ConfigChangedEvent {
    pub changed_fields: Vec<String>,
}

impl ConfigChangedEvent {

    pub fn affects(&self, prefixes: &[&str]) -> bool {
        self.changed_fields
            .iter()
            .any(|f| prefixes.iter().any(|p| f.starts_with(p)))
    }
}

pub struct SharedConfig {

    pub(crate) inner: Arc<ArcSwap<Config>>,
    pub(crate) notify: broadcast::Sender<ConfigChangedEvent>,
}

#[derive(Debug, Clone)]
pub struct LiveConfig {
    inner: Arc<SharedConfig>,
}

impl LiveConfig {

    #[inline]
    pub fn new(config: Config) -> Self {
        Self {
            inner: Arc::new(SharedConfig::new(config)),
        }
    }

    #[inline]
    pub fn from_shared(shared: Arc<SharedConfig>) -> Self {
        Self { inner: shared }
    }

    #[inline]
    pub fn load(&self) -> arc_swap::Guard<Arc<Config>> {
        self.inner.inner.load()
    }

    #[inline]
    pub fn load_ref(&self) -> Arc<Config> {
        self.inner.inner.load_full()
    }

    #[inline]
    pub fn store(&self, config: Config) {
        self.inner
            .as_ref()
            .store(config, vec!["runtime_hot_reload".into()]);
    }

    #[inline]
    pub fn store_validated(&self, config: Config) -> Result<(), String> {
        validate_full(&config)?;
        self.inner
            .as_ref()
            .apply(config, vec!["runtime_hot_reload".into()]);
        Ok(())
    }

    #[inline]
    pub fn swap(&self, config: Config) -> Result<Config, String> {
        validate_full(&config)?;
        let old = ArcSwap::swap(&self.inner.inner, Arc::new(config));
        let _ = self.inner.notify.send(ConfigChangedEvent {
            changed_fields: vec!["manual_swap".into()],
        });
        Ok(match Arc::try_unwrap(old) {
            Ok(config) => config,
            Err(shared) => (*shared).clone(),
        })
    }

    pub fn provider_changed_since(
        &self,
        cached_provider: &str,
        cached_api_key: &str,
        cached_api_url: &str,
    ) -> bool {
        let config = self.inner.inner.load_full();
        let new_provider = config.default_provider.as_deref().unwrap_or("openrouter");
        let new_api_key = config.api_key.as_deref().unwrap_or("");
        let new_api_url = config.api_url.as_deref().unwrap_or("");

        new_provider != cached_provider
            || new_api_key != cached_api_key
            || new_api_url != cached_api_url
    }

    #[inline]
    pub fn shared(&self) -> &Arc<SharedConfig> {
        &self.inner
    }
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self::new(Config::default())
    }
}

impl From<Config> for LiveConfig {
    fn from(config: Config) -> Self {
        Self::new(config)
    }
}

impl From<Arc<Config>> for LiveConfig {
    fn from(config: Arc<Config>) -> Self {
        Self::new((*config).clone())
    }
}

impl SharedConfig {

    pub fn new(config: Config) -> Self {
        let (notify, _) = broadcast::channel(16);
        Self {
            inner: Arc::new(ArcSwap::from_pointee(config)),
            notify,
        }
    }

    pub fn load(&self) -> Arc<Config> {
        self.inner.as_ref().load_full()
    }

    pub fn shared(&self) -> Arc<Self> {
        Arc::new(self.clone())
    }

    pub fn store(&self, new_config: Config, changed_fields: Vec<String>) {
        if let Err(error) = validate_full(&new_config) {
            tracing::warn!(
                target: "config.hot_reload",
                %error,
                changed_fields = ?changed_fields,
                "rejecting invalid live config update; previous snapshot remains active"
            );
            return;
        }
        self.apply(new_config, changed_fields);
    }

    fn apply(&self, new_config: Config, changed_fields: Vec<String>) {
        self.inner.as_ref().store(Arc::new(new_config));
        let _ = self.notify.send(ConfigChangedEvent { changed_fields });
    }

    pub fn mutate<F>(&self, f: F, changed_fields: Vec<String>)
    where
        F: FnOnce(&mut Config),
    {
        let mut new = (*self.load()).clone();
        f(&mut new);
        self.store(new, changed_fields);
    }

    pub fn swap(&self, new_config: Config) -> Result<Config, String> {
        validate_full(&new_config)?;
        let old = ArcSwap::swap(self.inner.as_ref(), Arc::new(new_config));
        let _ = self.notify.send(ConfigChangedEvent {
            changed_fields: vec!["manual_swap".into()],
        });
        Ok(match Arc::try_unwrap(old) {
            Ok(config) => config,
            Err(shared) => (*shared).clone(),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConfigChangedEvent> {
        self.notify.subscribe()
    }

    pub fn subscribe_filtered<F>(
        self: &std::sync::Arc<Self>,
        prefixes: Vec<String>,
        callback: F,
    ) -> crate::runtime::TaskHandle
    where
        F: FnMut(std::sync::Arc<Config>) + Send + 'static,
    {
        let store = self.clone();
        let callback = std::sync::Arc::new(std::sync::Mutex::new(callback));
        crate::runtime::spawn_supervised_restartable(
            "config.hot_reload.subscriber",
            3,
            move || {
                let store = store.clone();
                let prefixes = prefixes.clone();
                let callback = std::sync::Arc::clone(&callback);
                async move {
                    let mut rx = store.subscribe();
                    loop {
                        match rx.recv().await {
                            Ok(event) => {
                                let refs: Vec<&str> =
                                    prefixes.iter().map(String::as_str).collect();
                                if event.affects(&refs) {
                                    if let Ok(mut cb) = callback.lock() {
                                        cb(store.load());
                                    }
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(
                                    target: "config.hot_reload",
                                    skipped,
                                    "config change subscriber lagged; applying latest snapshot"
                                );
                                if let Ok(mut cb) = callback.lock() {
                                    cb(store.load());
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            },
        )
    }

}

impl Clone for SharedConfig {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            notify: self.notify.clone(),
        }
    }
}

impl std::fmt::Debug for SharedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedConfig").finish_non_exhaustive()
    }
}

