// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use anyhow::Context;
use reqwest as reqwest_proxy;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use crate::config::ProxyConfig;
use crate::config::schema::{is_disallowed_custom_header, normalize_proxy_url_option};

const FALLBACK_TIMEOUT_SECS: u64 = 120;
const FALLBACK_CONNECT_TIMEOUT_SECS: u64 = 30;
const DEFAULT_CONNECT_TIMEOUT_SECS: u64 = 15;
const DEFAULT_READ_IDLE_TIMEOUT_SECS: u64 = 300;

fn timed_fallback_client(
    timeout_secs: Option<u64>,
    connect_timeout_secs: Option<u64>,
    no_redirect: bool,
) -> reqwest::Client {
    let timeout = timeout_secs.unwrap_or(FALLBACK_TIMEOUT_SECS);
    let connect = connect_timeout_secs.unwrap_or(FALLBACK_CONNECT_TIMEOUT_SECS);
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .connect_timeout(std::time::Duration::from_secs(connect));
    if no_redirect {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}

trait AsyncReadWrite: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> AsyncReadWrite for T {}

pub struct BoxedIo(Box<dyn AsyncReadWrite>);

impl tokio::io::AsyncRead for BoxedIo {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.0).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for BoxedIo {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut *self.0).poll_write(cx, buf)
    }
    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.0).poll_flush(cx)
    }
    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut *self.0).poll_shutdown(cx)
    }
}

impl Unpin for BoxedIo {}

pub type ProxiedWsStream = tokio_tungstenite::WebSocketStream<BoxedIo>;

static GLOBAL_PROXY_RUNTIME: OnceLock<Arc<ProxyRuntime>> = OnceLock::new();

#[derive(Debug)]
pub struct ProxyRuntime {
    config: RwLock<ProxyConfig>,
    clients: RwLock<HashMap<String, reqwest::Client>>,
}

impl Default for ProxyRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxyRuntime {
    pub fn new() -> Self {
        Self {
            config: RwLock::new(ProxyConfig::default()),
            clients: RwLock::new(HashMap::new()),
        }
    }

    pub fn global() -> Arc<ProxyRuntime> {
        GLOBAL_PROXY_RUNTIME
            .get_or_init(|| Arc::new(ProxyRuntime::new()))
            .clone()
    }

    pub fn snapshot(&self) -> ProxyConfig {
        self.config
            .read()
            .ok()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn replace(&self, config: ProxyConfig) {
        if let Ok(mut g) = self.config.write() {
            *g = config;
        }
        crate::services::proxy::system::invalidate();
        self.clear_client_cache();
    }

    pub fn applies_to(&self, service_key: &str) -> bool {
        self.snapshot().should_apply_to_service(service_key)
    }

    pub fn apply_to_builder(
        &self,
        builder: reqwest::ClientBuilder,
        service_key: &str,
    ) -> reqwest::ClientBuilder {
        let cfg = self.snapshot();
        if cfg.has_explicit_proxy() {
            return cfg.apply_to_reqwest_builder(builder, service_key);
        }
        if cfg.system_detect {
            if let Some(sys) = crate::services::proxy::system::detect_cached() {
                return apply_system_proxy_to_builder(builder, &sys, &cfg);
            }
        }
        builder
    }

    fn proxy_fingerprint(&self, service_key: &str) -> String {
        let cfg = self.snapshot();
        if cfg.has_explicit_proxy() {
            if cfg.should_apply_to_service(service_key) {
                return format!(
                    "cfg|{}|{}|{}",
                    cfg.all_proxy.as_deref().unwrap_or("-"),
                    cfg.http_proxy.as_deref().unwrap_or("-"),
                    cfg.https_proxy.as_deref().unwrap_or("-"),
                );
            }
            return "cfg|off".to_string();
        }
        if cfg.system_detect {
            if let Some(sys) = crate::services::proxy::system::detect_cached() {
                return format!("sys|{}", sys.signature());
            }
        }
        "none".to_string()
    }

    pub fn build_client(&self, service_key: &str) -> reqwest::Client {
        crate::services::proxy::registry::register(service_key);
        let ck = format!(
            "{}|{}",
            cache_key(service_key, None, Some(DEFAULT_CONNECT_TIMEOUT_SECS)),
            self.proxy_fingerprint(service_key)
        );
        if let Some(c) = self.cached_client(&ck) {
            return c;
        }
        let b = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
            .read_timeout(std::time::Duration::from_secs(DEFAULT_READ_IDLE_TIMEOUT_SECS))
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .tcp_keepalive(std::time::Duration::from_secs(30));
        let c = self
            .apply_to_builder(b, service_key)
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(service_key, "Failed to build proxied client: {e}");
                timed_fallback_client(None, Some(DEFAULT_CONNECT_TIMEOUT_SECS), false)
            });
        self.set_cached_client(ck, c.clone());
        c
    }

    pub fn build_client_with_timeouts(
        &self,
        service_key: &str,
        timeout_secs: u64,
        connect_timeout_secs: u64,
    ) -> reqwest::Client {
        crate::services::proxy::registry::register(service_key);
        let ck = format!(
            "{}|{}",
            cache_key(service_key, Some(timeout_secs), Some(connect_timeout_secs)),
            self.proxy_fingerprint(service_key)
        );
        if let Some(c) = self.cached_client(&ck) {
            return c;
        }
        let b = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs))
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .tcp_keepalive(std::time::Duration::from_secs(30));
        let c = self
            .apply_to_builder(b, service_key)
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(service_key, "Failed to build proxied timeout client: {e}");
                timed_fallback_client(Some(timeout_secs), Some(connect_timeout_secs), false)
            });
        self.set_cached_client(ck, c.clone());
        c
    }

    pub fn build_search_client(
        &self,
        service_key: &str,
        timeout_secs: u64,
        user_agent: &str,
    ) -> reqwest::Client {
        crate::services::proxy::registry::register(service_key);
        let ck = format!(
            "{}|ua={}|{}",
            cache_key(service_key, Some(timeout_secs), None),
            user_agent,
            self.proxy_fingerprint(service_key)
        );
        if let Some(c) = self.cached_client(&ck) {
            return c;
        }
        let b = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .user_agent(user_agent);
        let c = self
            .apply_to_builder(b, service_key)
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(service_key, "Failed to build proxied search client: {e}");
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(timeout_secs))
                    .user_agent(user_agent)
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new())
            });
        self.set_cached_client(ck, c.clone());
        c
    }

    pub fn build_client_no_redirect_with_timeouts(
        &self,
        service_key: &str,
        timeout_secs: u64,
        connect_timeout_secs: u64,
    ) -> reqwest::Client {
        crate::services::proxy::registry::register(service_key);
        let ck = format!(
            "{}|noredirect|{}",
            cache_key(service_key, Some(timeout_secs), Some(connect_timeout_secs)),
            self.proxy_fingerprint(service_key)
        );
        if let Some(c) = self.cached_client(&ck) {
            return c;
        }
        let b = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs))
            .redirect(reqwest::redirect::Policy::none());
        let c = self
            .apply_to_builder(b, service_key)
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(
                    service_key,
                    "Failed to build proxied no-redirect client: {e}"
                );
                timed_fallback_client(Some(timeout_secs), Some(connect_timeout_secs), true)
            });
        self.set_cached_client(ck, c.clone());
        c
    }

    pub fn build_client_with_timeouts_and_headers(
        &self,
        service_key: &str,
        timeout_secs: u64,
        connect_timeout_secs: u64,
        headers: &HashMap<String, String>,
    ) -> reqwest::Client {
        crate::services::proxy::registry::register(service_key);

        let mut header_map = reqwest::header::HeaderMap::with_capacity(headers.len());
        for (key, value) in headers {
            let trimmed_key = key.trim();
            if trimmed_key.is_empty() {
                continue;
            }
            if is_disallowed_custom_header(trimmed_key) {
                tracing::warn!(
                    service_key,
                    header_name = trimmed_key,
                    "skipping reserved/disallowed custom HTTP header when building HTTP client"
                );
                continue;
            }
            match (
                reqwest::header::HeaderName::from_bytes(trimmed_key.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                (Ok(name), Ok(val)) => {
                    header_map.insert(name, val);
                }
                _ => {
                    tracing::warn!(
                        service_key,
                        header_name = trimmed_key,
                        "skipping invalid custom HTTP header name or value when building HTTP client"
                    );
                }
            }
        }

        let builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs))
            .default_headers(header_map);
        let builder = self.apply_to_builder(builder, service_key);
        builder.build().unwrap_or_else(|e| {
            tracing::warn!(
                service_key,
                "Failed to build proxied timeout client with custom headers: {e}"
            );
            timed_fallback_client(Some(timeout_secs), Some(connect_timeout_secs), false)
        })
    }

    pub fn build_stream_client(
        &self,
        service_key: &str,
        read_timeout_secs: u64,
        connect_timeout_secs: u64,
        headers: &reqwest::header::HeaderMap,
    ) -> reqwest::Client {
        crate::services::proxy::registry::register(service_key);
        let build = || {
            let mut builder = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs))
                .read_timeout(std::time::Duration::from_secs(read_timeout_secs))
                .pool_idle_timeout(std::time::Duration::from_secs(15));
            if !headers.is_empty() {
                builder = builder.default_headers(headers.clone());
            }
            builder
        };
        self.apply_to_builder(build(), service_key)
            .build()
            .or_else(|error| {
                tracing::warn!(
                    service_key,
                    "Failed to build proxied stream client: {error}; retrying without proxy"
                );
                build().build()
            })
            .unwrap_or_else(|error| {
                tracing::warn!(service_key, "Failed to build stream client: {error}");
                reqwest::Client::builder()
                    .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs))
                    .read_timeout(std::time::Duration::from_secs(read_timeout_secs))
                    .build()
                    .unwrap_or_else(|_| reqwest::Client::new())
            })
    }

    pub fn build_channel_client(
        &self,
        service_key: &str,
        proxy_url: Option<&str>,
    ) -> reqwest::Client {
        match normalize_proxy_url_option(proxy_url) {
            Some(u) => self.build_explicit_client(service_key, &u, None, None),
            None => self.build_client(service_key),
        }
    }

    pub fn build_channel_client_with_timeouts(
        &self,
        service_key: &str,
        proxy_url: Option<&str>,
        timeout_secs: u64,
        connect_timeout_secs: u64,
    ) -> reqwest::Client {
        match normalize_proxy_url_option(proxy_url) {
            Some(u) => self.build_explicit_client(
                service_key,
                &u,
                Some(timeout_secs),
                Some(connect_timeout_secs),
            ),
            None => self.build_client_with_timeouts(
                service_key,
                timeout_secs,
                connect_timeout_secs,
            ),
        }
    }

    pub fn apply_channel_to_builder(
        &self,
        builder: reqwest::ClientBuilder,
        service_key: &str,
        proxy_url: Option<&str>,
    ) -> reqwest::ClientBuilder {
        match normalize_proxy_url_option(proxy_url) {
            Some(u) => apply_explicit_proxy_to_builder(builder, service_key, &u),
            None => self.apply_to_builder(builder, service_key),
        }
    }

    pub fn build_explicit_client(
        &self,
        service_key: &str,
        proxy_url: &str,
        timeout_secs: Option<u64>,
        connect_timeout_secs: Option<u64>,
    ) -> reqwest::Client {
        crate::services::proxy::registry::register(service_key);
        let ck = format!(
            "explicit|{}|{}|timeout={}|connect_timeout={}",
            service_key.trim().to_ascii_lowercase(),
            proxy_url,
            timeout_secs
                .map(|v| v.to_string())
                .unwrap_or_else(|| "none".to_string()),
            connect_timeout_secs
                .map(|v| v.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
        if let Some(c) = self.cached_client(&ck) {
            return c;
        }
        let mut b = reqwest::Client::builder();
        if let Some(t) = timeout_secs {
            b = b.timeout(std::time::Duration::from_secs(t));
        } else {
            b = b.read_timeout(std::time::Duration::from_secs(
                DEFAULT_READ_IDLE_TIMEOUT_SECS,
            ));
        }
        b = b.connect_timeout(std::time::Duration::from_secs(
            connect_timeout_secs.unwrap_or(DEFAULT_CONNECT_TIMEOUT_SECS),
        ));
        b = apply_explicit_proxy_to_builder(b, service_key, proxy_url);
        let c = b.build().unwrap_or_else(|e| {
            tracing::warn!(
                service_key,
                proxy_url,
                "Failed to build channel proxy client: {e}"
            );
            timed_fallback_client(timeout_secs, connect_timeout_secs, false)
        });
        self.set_cached_client(ck, c.clone());
        c
    }

    pub async fn ws_connect(
        &self,
        ws_url: &str,
        service_key: &str,
        channel_proxy_url: Option<&str>,
    ) -> anyhow::Result<(
        ProxiedWsStream,
        tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>,
    )> {
        let proxy_url = self.resolve_ws_proxy_url(service_key, ws_url, channel_proxy_url);
        let connect_timeout = std::time::Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS);
        match proxy_url {
            None => {
                let (stream, resp) = tokio::time::timeout(
                    connect_timeout,
                    tokio_tungstenite::connect_async(ws_url),
                )
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "websocket connect to {ws_url} timed out after \
                         {DEFAULT_CONNECT_TIMEOUT_SECS}s"
                    )
                })??;
                let inner = stream.into_inner();
                let boxed = BoxedIo(Box::new(inner));
                let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
                    boxed,
                    tokio_tungstenite::tungstenite::protocol::Role::Client,
                    None,
                )
                .await;
                Ok((ws, resp))
            }
            Some(p) => tokio::time::timeout(connect_timeout, ws_connect_via_proxy(ws_url, &p))
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "websocket connect to {ws_url} via proxy timed out after \
                         {DEFAULT_CONNECT_TIMEOUT_SECS}s"
                    )
                })?,
        }
    }

    fn resolve_ws_proxy_url(
        &self,
        service_key: &str,
        ws_url: &str,
        channel_proxy_url: Option<&str>,
    ) -> Option<String> {
        if let Some(url) = normalize_proxy_url_option(channel_proxy_url) {
            return Some(url);
        }
        let cfg = self.snapshot();

        let (https, http, all, bypass) = if cfg.has_explicit_proxy() {
            if !cfg.should_apply_to_service(service_key) {
                return None;
            }
            (
                normalize_proxy_url_option(cfg.https_proxy.as_deref()),
                normalize_proxy_url_option(cfg.http_proxy.as_deref()),
                normalize_proxy_url_option(cfg.all_proxy.as_deref()),
                cfg.normalized_no_proxy(),
            )
        } else if cfg.system_detect {
            match crate::services::proxy::system::detect_cached() {
                Some(sys) => {
                    let mut bypass = cfg.normalized_no_proxy();
                    bypass.extend(sys.bypass.iter().cloned());
                    (sys.https, sys.http, sys.all, bypass)
                }
                None => return None,
            }
        } else {
            return None;
        };

        if !bypass.is_empty() {
            if let Ok(parsed) = reqwest::Url::parse(ws_url) {
                if let Some(host) = parsed.host_str() {
                    let hl = host.to_ascii_lowercase();
                    if bypass.iter().any(|e| {
                        let e = e.trim().to_ascii_lowercase();
                        e == "*"
                            || hl == e
                            || (e.starts_with('.') && (hl.ends_with(&e) || hl == e[1..]))
                            || hl.ends_with(&format!(".{e}"))
                    }) {
                        return None;
                    }
                }
            }
        }

        let is_secure = ws_url.starts_with("wss://") || ws_url.starts_with("wss:");
        let pref = if is_secure { https } else { http };
        pref.or(all)
    }

    fn cached_client(&self, cache_key: &str) -> Option<reqwest::Client> {
        self.clients
            .read()
            .ok()
            .and_then(|g| g.get(cache_key).cloned())
    }

    fn set_cached_client(&self, cache_key: String, client: reqwest::Client) {
        if let Ok(mut g) = self.clients.write() {
            g.insert(cache_key, client);
        }
    }

    fn clear_client_cache(&self) {
        if let Ok(mut g) = self.clients.write() {
            g.clear();
        }
    }
}

fn cache_key(
    service_key: &str,
    timeout_secs: Option<u64>,
    connect_timeout_secs: Option<u64>,
) -> String {
    let t = timeout_secs
        .map(|v| v.to_string())
        .unwrap_or_else(|| "none".to_string());
    let ct = connect_timeout_secs
        .map(|v| v.to_string())
        .unwrap_or_else(|| "none".to_string());
    format!(
        "{}|timeout={}|connect_timeout={}",
        service_key.trim().to_ascii_lowercase(),
        t,
        ct
    )
}

fn apply_system_proxy_to_builder(
    mut builder: reqwest::ClientBuilder,
    detected: &crate::services::proxy::system::DetectedSystemProxy,
    cfg: &ProxyConfig,
) -> reqwest::ClientBuilder {
    let mut bypass = cfg.normalized_no_proxy();
    bypass.extend(detected.bypass.iter().cloned());
    let no_proxy = if bypass.is_empty() {
        None
    } else {
        reqwest::NoProxy::from_string(&bypass.join(","))
    };

    type ProxyCtor = fn(&str) -> Result<reqwest_proxy::Proxy, reqwest::Error>;
    let entries: [(Option<&String>, ProxyCtor); 3] = [
        (detected.all.as_ref(), |u| reqwest_proxy::Proxy::all(u)),
        (detected.http.as_ref(), |u| reqwest_proxy::Proxy::http(u)),
        (detected.https.as_ref(), |u| reqwest_proxy::Proxy::https(u)),
    ];
    for (url_opt, make) in entries {
        if let Some(url) = url_opt {
            match make(url) {
                Ok(p) => {
                    builder = builder.proxy(p.no_proxy(no_proxy.clone()));
                }
                Err(e) => {
                    tracing::warn!(proxy_url = %url, "Ignoring invalid system proxy URL: {e}");
                }
            }
        }
    }
    builder
}

fn apply_explicit_proxy_to_builder(
    mut builder: reqwest::ClientBuilder,
    service_key: &str,
    proxy_url: &str,
) -> reqwest::ClientBuilder {
    match reqwest_proxy::Proxy::all(proxy_url) {
        Ok(p) => {
            builder = builder.proxy(p);
        }
        Err(e) => {
            tracing::warn!(
                proxy_url,
                service_key,
                "Ignoring invalid channel proxy_url: {e}"
            );
        }
    }
    builder
}

async fn ws_connect_via_proxy(
    ws_url: &str,
    proxy_url: &str,
) -> anyhow::Result<(
    ProxiedWsStream,
    tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>,
)> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt as _};
    use tokio::net::TcpStream;
    let target =
        reqwest::Url::parse(ws_url).with_context(|| format!("Invalid WebSocket URL: {ws_url}"))?;
    let th = target
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("WebSocket URL has no host: {ws_url}"))?
        .to_string();
    let tp = target
        .port_or_known_default()
        .unwrap_or(if target.scheme() == "wss" { 443 } else { 80 });
    let proxy = reqwest::Url::parse(proxy_url)
        .with_context(|| format!("Invalid proxy URL: {proxy_url}"))?;
    let stream: BoxedIo = match proxy.scheme() {
        "socks5" | "socks5h" | "socks" => {
            let pa = format!(
                "{}:{}",
                proxy.host_str().unwrap_or("127.0.0.1"),
                proxy.port_or_known_default().unwrap_or(1080)
            );
            let ta = format!("{th}:{tp}");
            let s5 = if proxy.username().is_empty() {
                tokio_socks::tcp::Socks5Stream::connect(pa.as_str(), ta.as_str())
                    .await
                    .with_context(|| format!("SOCKS5 connect to {ta} via {pa}"))?
            } else {
                tokio_socks::tcp::Socks5Stream::connect_with_password(
                    pa.as_str(),
                    ta.as_str(),
                    proxy.username(),
                    proxy.password().unwrap_or(""),
                )
                .await
                .with_context(|| format!("SOCKS5 auth connect to {ta} via {pa}"))?
            };
            BoxedIo(Box::new(s5.into_inner()))
        }
        "http" | "https" => {
            let ph = proxy.host_str().unwrap_or("127.0.0.1");
            let pp = proxy.port_or_known_default().unwrap_or(8080);
            let mut tcp = TcpStream::connect(&format!("{ph}:{pp}"))
                .await
                .with_context(|| format!("TCP connect to HTTP proxy {ph}:{pp}"))?;
            tcp.write_all(
                format!("CONNECT {th}:{tp} HTTP/1.1\r\nHost: {th}:{tp}\r\n\r\n").as_bytes(),
            )
            .await?;
            let mut buf = vec![0u8; 4096];
            let mut tot = 0usize;
            loop {
                let n = tcp.read(&mut buf[tot..]).await?;
                if n == 0 {
                    anyhow::bail!("HTTP CONNECT proxy closed connection before response");
                }
                tot += n;
                if let Some(pos) = find_header_end(&buf[..tot]) {
                    let sl = std::str::from_utf8(&buf[..pos])
                        .unwrap_or("")
                        .lines()
                        .next()
                        .unwrap_or("");
                    if !sl.contains("200") {
                        anyhow::bail!("HTTP CONNECT proxy returned non-200: {sl}");
                    }
                    break;
                }
                if tot >= buf.len() {
                    anyhow::bail!("HTTP CONNECT proxy response too large");
                }
            }
            BoxedIo(Box::new(tcp))
        }
        scheme => {
            anyhow::bail!("Unsupported proxy scheme '{scheme}' for WebSocket connections");
        }
    };
    let stream: BoxedIo = if target.scheme() == "wss" {
        let mut rs = rustls::RootCertStore::empty();
        rs.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let native = rustls_native_certs::load_native_certs();
        for cert in native.certs {
            let _ = rs.add(cert);
        }
        let tc = std::sync::Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(rs)
                .with_no_client_auth(),
        );
        let conn = tokio_rustls::TlsConnector::from(tc);
        let sn = rustls_pki_types::ServerName::try_from(th.clone())
            .with_context(|| format!("Invalid TLS server name: {th}"))?;
        BoxedIo(Box::new(
            conn.connect(sn, stream)
                .await
                .with_context(|| format!("TLS handshake with {th}"))?,
        ))
    } else {
        stream
    };
    let req = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(ws_url)
        .header("Host", format!("{th}:{tp}"))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header(
            "Sec-WebSocket-Key",
            tokio_tungstenite::tungstenite::handshake::client::generate_key(),
        )
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .with_context(|| "Failed to build WebSocket upgrade request")?;
    let (ws_stream, response) = tokio_tungstenite::client_async(req, stream)
        .await
        .with_context(|| format!("WebSocket handshake failed for {ws_url}"))?;
    Ok((ws_stream, response))
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}
