// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::broadcast;

use super::schema::Config;

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
        self.inner.as_ref().store(config, vec!["runtime_hot_reload".into()]);
    }

    #[inline]
    pub fn swap(&self, config: Config) -> Config {
        Arc::into_inner(ArcSwap::swap(&self.inner.inner, Arc::new(config)))
            .expect("ArcSwap must hold the only reference after swap")
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

    pub fn swap(&self, new_config: Config) -> Config {
        let old = ArcSwap::swap(self.inner.as_ref(), Arc::new(new_config));
        let _ = self.notify.send(ConfigChangedEvent {
            changed_fields: vec!["manual_swap".into()],
        });
        Arc::into_inner(old).expect("ArcSwap must hold the only reference after swap")
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ConfigChangedEvent> {
        self.notify.subscribe()
    }

    pub fn subscribe_filtered<F>(
        self: &std::sync::Arc<Self>,
        prefixes: Vec<String>,
        mut callback: F,
    ) -> crate::runtime::TaskHandle
    where
        F: FnMut(std::sync::Arc<Config>) + Send + 'static,
    {
        let mut rx = self.subscribe();
        let store = self.clone();
        crate::runtime::spawn_supervised("config.hot_reload.subscriber", async move {
            while let Ok(event) = rx.recv().await {
                let refs: Vec<&str> = prefixes.iter().map(String::as_str).collect();
                if event.affects(&refs) {
                    callback(store.load());
                }
            }
        })
    }

    pub fn store_validated(
        &self,
        new_config: Config,
        changed_fields: Vec<String>,
        validator: impl Fn(&Config) -> Result<(), String>,
    ) -> Result<(), String> {
        validator(&new_config)?;
        self.store(new_config, changed_fields);
        Ok(())
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

pub mod validators {
    use super::Config;

    pub fn validate_temperature(cfg: &Config) -> Result<(), String> {
        if !(0.0..=2.0).contains(&cfg.default_temperature) {
            return Err(format!(
                "default_temperature must be in [0.0, 2.0], got {}",
                cfg.default_temperature
            ));
        }
        Ok(())
    }

    pub fn validate_provider_coherence(cfg: &Config) -> Result<(), String> {
        if cfg.api_url.is_some() && cfg.default_provider.is_none() {
            return Err("api_url set without default_provider".into());
        }
        Ok(())
    }

    pub fn validate_all(cfg: &Config) -> Result<(), String> {
        validate_temperature(cfg)?;
        validate_provider_coherence(cfg)?;
        Ok(())
    }
}
