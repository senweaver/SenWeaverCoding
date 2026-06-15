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
        self.snapshot().apply_to_reqwest_builder(builder, service_key)
    }

    pub fn build_client(&self, service_key: &str) -> reqwest::Client {
        crate::services::proxy::registry::register(service_key);
        let ck = cache_key(service_key, None, None);
        if let Some(c) = self.cached_client(&ck) {
            return c;
        }
        let c = self
            .apply_to_builder(reqwest::Client::builder(), service_key)
            .build()
            .unwrap_or_else(|e| {
                tracing::warn!(service_key, "Failed to build proxied client: {e}");
                timed_fallback_client(None, None, false)
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
        let ck = cache_key(service_key, Some(timeout_secs), Some(connect_timeout_secs));
        if let Some(c) = self.cached_client(&ck) {
            return c;
        }
        let b = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .connect_timeout(std::time::Duration::from_secs(connect_timeout_secs));
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

    pub fn build_client_no_redirect_with_timeouts(
        &self,
        service_key: &str,
        timeout_secs: u64,
        connect_timeout_secs: u64,
    ) -> reqwest::Client {
        crate::services::proxy::registry::register(service_key);
        let ck = format!(
            "{}|noredirect",
            cache_key(service_key, Some(timeout_secs), Some(connect_timeout_secs))
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
        if headers.is_empty() {
            return self.build_client_with_timeouts(
                service_key,
                timeout_secs,
                connect_timeout_secs,
            );
        }

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
        }
        if let Some(ct) = connect_timeout_secs {
            b = b.connect_timeout(std::time::Duration::from_secs(ct));
        }
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
        match proxy_url {
            None => {
                let (stream, resp) = tokio_tungstenite::connect_async(ws_url).await?;
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
            Some(p) => ws_connect_via_proxy(ws_url, &p).await,
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
        if !cfg.should_apply_to_service(service_key) {
            return None;
        }
        if let Ok(parsed) = reqwest::Url::parse(ws_url) {
            if let Some(host) = parsed.host_str() {
                let entries = cfg.normalized_no_proxy();
                if !entries.is_empty() {
                    let hl = host.to_ascii_lowercase();
                    if entries.iter().any(|e| {
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
        let pref = if is_secure {
            normalize_proxy_url_option(cfg.https_proxy.as_deref())
        } else {
            normalize_proxy_url_option(cfg.http_proxy.as_deref())
        };
        pref.or_else(|| normalize_proxy_url_option(cfg.all_proxy.as_deref()))
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
