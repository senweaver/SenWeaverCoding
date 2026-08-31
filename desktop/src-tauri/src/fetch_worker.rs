// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use senweavercoding::tools::web::fetch::{
    FetchController, FetchedPage, install_fetch_controller,
};
use serde_json::Value;
use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl,
    webview::WebviewBuilder,
};
use tokio::sync::{Mutex as AsyncMutex, oneshot};

pub const FETCH_WORKER_LABEL: &str = "fetch_worker_main";

const BRIDGE_HOST: &str = "senbridge.localhost";
const BRIDGE_SCHEME: &str = "senbridge";
const ABOUT_BLANK: &str = "about:blank";

fn bridge_base_url() -> &'static str {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        "http://senbridge.localhost"
    } else {
        "senbridge://localhost"
    }
}

fn build_fetch_bridge_js() -> String {
    let header = format!(
        "window.__SEN_BRIDGE_BASE = {base:?};",
        base = bridge_base_url(),
    );
    format!("{header}\n{FETCH_BRIDGE_JS}")
}

const FETCH_BRIDGE_JS: &str = r#"
(() => {
  if (window.__senFetchBridge) return;

  const BRIDGE_BASE = window.__SEN_BRIDGE_BASE || 'http://senbridge.localhost';

  function send(kind, data) {
    try {
      const params = new URLSearchParams();
      params.set('kind', kind);
      params.set('data', JSON.stringify(data == null ? null : data));
      const url = `${BRIDGE_BASE}/fetch_event?${params.toString()}`;
      try {
        fetch(url, {
          method: 'GET',
          mode: 'no-cors',
          cache: 'no-store',
          credentials: 'omit',
          keepalive: true,
        }).catch(() => {});
      } catch (_) {
        try {
          const xhr = new XMLHttpRequest();
          xhr.open('GET', url, true);
          xhr.send();
        } catch (_) {}
      }
    } catch (_) {}
  }

  const RESULT_CHUNK_BYTES = 14000;
  function postResult(reqId, ok, value, error) {
    let envelopeJson;
    try {
      envelopeJson = JSON.stringify({ ok: !!ok, value: value == null ? null : value, error: error == null ? null : error });
    } catch (err) {
      envelopeJson = JSON.stringify({ ok: false, value: null, error: 'json serialise: ' + String(err && err.message || err) });
    }
    if (envelopeJson.length <= RESULT_CHUNK_BYTES) {
      send('result', { reqId, ok: !!ok, value: value == null ? null : value, error: error == null ? null : error });
      return;
    }
    const total = Math.ceil(envelopeJson.length / RESULT_CHUNK_BYTES);
    for (let i = 0; i < total; i += 1) {
      const start = i * RESULT_CHUNK_BYTES;
      const slice = envelopeJson.slice(start, start + RESULT_CHUNK_BYTES);
      send('result_chunk', { reqId, seq: i, total, payload: slice });
    }
  }

  function snapshot() {
    try {
      send('state', {
        url: window.location.href,
        title: document.title || '',
        ts: Date.now(),
      });
    } catch (_) {}
  }

  function extractMainText() {
    try {
      const root = document.body;
      if (!root) return '';
      const clone = root.cloneNode(true);
      const drop = clone.querySelectorAll(
        'script, style, noscript, template, link, meta, ' +
        'svg, canvas, iframe[hidden], [aria-hidden="true"]'
      );
      drop.forEach((el) => { try { el.remove(); } catch (_) {} });
      const raw = clone.innerText || clone.textContent || '';
      const lines = raw.split('\n').map((line) => line.replace(/\s+$/g, ''));
      const collapsed = [];
      let blankRun = 0;
      for (const line of lines) {
        if (line.trim() === '') {
          blankRun += 1;
          if (blankRun <= 1) collapsed.push('');
        } else {
          blankRun = 0;
          collapsed.push(line);
        }
      }
      return collapsed.join('\n').trim();
    } catch (_) {
      try {
        return document.body
          ? (document.body.innerText || document.body.textContent || '').trim()
          : '';
      } catch (_) {
        return '';
      }
    }
  }

  const handlers = {
    extract_text() {
      return {
        url: window.location.href,
        title: document.title || '',
        text: extractMainText(),
      };
    },
    wait_for_ready(args) {
      const target = String((args && (args.ready_state || args.readyState)) || 'complete').toLowerCase();
      const timeoutMs = Number((args && (args.timeout_ms || args.timeoutMs)) || 12000);
      return new Promise((resolve, reject) => {
        const start = Date.now();
        function check() {
          const cur = String(document.readyState || '').toLowerCase();
          const ok = target === 'complete'
            ? cur === 'complete'
            : (target === 'interactive'
                ? (cur === 'interactive' || cur === 'complete')
                : cur === target);
          if (ok) {
            resolve({ ready_state: cur, elapsed_ms: Date.now() - start });
            return true;
          }
          return false;
        }
        if (check()) return;
        const onState = () => { if (check()) cleanup(); };
        function cleanup() {
          document.removeEventListener('readystatechange', onState, true);
          clearTimeout(to);
        }
        document.addEventListener('readystatechange', onState, true);
        const to = setTimeout(() => {
          cleanup();
          reject(new Error('wait_for_ready timeout'));
        }, timeoutMs);
      });
    },
    ping() { return { ok: true, ts: Date.now() }; },
  };

  window.__senFetchBridge = {
    exec(payload) {
      const reqId = payload && payload.reqId;
      const kind = payload && payload.kind;
      const args = (payload && payload.args) || {};
      let value;
      try {
        const fn = handlers[kind];
        if (!fn) throw new Error('unknown bridge kind: ' + kind);
        value = fn(args);
      } catch (err) {
        postResult(reqId, false, null, String(err && err.message || err));
        return;
      }
      Promise.resolve(value).then(
        (v) => postResult(reqId, true, v, null),
        (err) => postResult(reqId, false, null, String(err && err.message || err)),
      );
    },
  };

  window.addEventListener('load', snapshot);
  window.addEventListener('popstate', snapshot);
  window.addEventListener('hashchange', snapshot);
  document.addEventListener('readystatechange', () => {
    if (document.readyState === 'interactive' || document.readyState === 'complete') {
      snapshot();
    }
  });
  setTimeout(snapshot, 50);
})();
"#;

#[derive(Default)]
struct FetchInternal {
    last_state_url: Option<String>,
}

#[derive(Default, Clone)]
pub struct FetchSharedState(Arc<Mutex<FetchInternal>>);

impl FetchSharedState {
    pub fn new() -> Self {
        Self::default()
    }

    fn record_state_url(&self, url: impl Into<String>) {
        self.0.lock().last_state_url = Some(url.into());
    }

    fn last_state_url(&self) -> Option<String> {
        self.0.lock().last_state_url.clone()
    }

    fn forget_state_url(&self) {
        self.0.lock().last_state_url = None;
    }
}

#[derive(Default)]
struct ChunkBuffer {
    parts: Vec<Option<String>>,
    received: usize,
}

impl ChunkBuffer {
    fn new(total: usize) -> Self {
        Self {
            parts: vec![None; total],
            received: 0,
        }
    }
    fn ingest(&mut self, seq: usize, payload: String) -> bool {
        if seq >= self.parts.len() {
            return false;
        }
        if self.parts[seq].is_none() {
            self.parts[seq] = Some(payload);
            self.received += 1;
        }
        self.received == self.parts.len()
    }
    fn assemble(self) -> String {
        let mut buf = String::with_capacity(
            self.parts
                .iter()
                .map(|p| p.as_ref().map(|s| s.len()).unwrap_or(0))
                .sum(),
        );
        for part in self.parts.into_iter().flatten() {
            buf.push_str(&part);
        }
        buf
    }
}

#[derive(Debug, Clone)]
struct FetchResponse {
    ok: bool,
    value: Value,
    error: Option<String>,
}

#[derive(Clone)]
pub struct TauriFetchController(Arc<TauriFetchControllerInner>);

const PENDING_REQUEST_MAX_AGE: Duration = Duration::from_secs(120);

struct TauriFetchControllerInner {
    app: AppHandle,
    pending: Mutex<HashMap<u64, (std::time::Instant, oneshot::Sender<FetchResponse>)>>,
    chunks: Mutex<HashMap<u64, ChunkBuffer>>,
    nav_waiters: Mutex<Vec<oneshot::Sender<()>>>,
    drive_lock: AsyncMutex<()>,
    next_req_id: AtomicU64,
}

impl TauriFetchController {
    pub fn new(app: AppHandle) -> Self {
        Self(Arc::new(TauriFetchControllerInner {
            app,
            pending: Mutex::new(HashMap::new()),
            chunks: Mutex::new(HashMap::new()),
            nav_waiters: Mutex::new(Vec::new()),
            drive_lock: AsyncMutex::new(()),
            next_req_id: AtomicU64::new(1),
        }))
    }

    fn forget_request(&self, req_id: u64) {
        self.0.pending.lock().remove(&req_id);
        self.0.chunks.lock().remove(&req_id);
    }

    fn sweep_stale_requests(&self) {
        let mut pending = self.0.pending.lock();
        let before = pending.len();
        pending.retain(|_, (created_at, _)| created_at.elapsed() < PENDING_REQUEST_MAX_AGE);
        if pending.len() < before {
            let mut chunks = self.0.chunks.lock();
            chunks.retain(|req_id, _| pending.contains_key(req_id));
        }
    }

    pub fn deliver_result(
        &self,
        req_id: u64,
        ok: bool,
        value: Value,
        error: Option<String>,
    ) {
        self.0.chunks.lock().remove(&req_id);
        let sender = self.0.pending.lock().remove(&req_id);
        if let Some((_, tx)) = sender {
            let _ = tx.send(FetchResponse { ok, value, error });
        }
    }

    pub fn deliver_chunk(
        &self,
        req_id: u64,
        seq: usize,
        total: usize,
        payload: String,
    ) {
        let assembled = {
            let mut guard = self.0.chunks.lock();
            let buf = guard
                .entry(req_id)
                .or_insert_with(|| ChunkBuffer::new(total));
            if buf.parts.len() != total {
                *buf = ChunkBuffer::new(total);
            }
            if buf.ingest(seq, payload) {
                guard.remove(&req_id).map(|b| b.assemble())
            } else {
                None
            }
        };
        if let Some(json) = assembled {
            match serde_json::from_str::<Value>(&json) {
                Ok(env) => {
                    let ok = env.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
                    let value = env.get("value").cloned().unwrap_or(Value::Null);
                    let error = env
                        .get("error")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let sender = self.0.pending.lock().remove(&req_id);
                    if let Some((_, tx)) = sender {
                        let _ = tx.send(FetchResponse { ok, value, error });
                    }
                }
                Err(err) => {
                    let sender = self.0.pending.lock().remove(&req_id);
                    if let Some((_, tx)) = sender {
                        let _ = tx.send(FetchResponse {
                            ok: false,
                            value: Value::Null,
                            error: Some(format!(
                                "chunk reassembly parse error: {err}"
                            )),
                        });
                    }
                }
            }
        }
    }

    pub fn signal_nav_ready(&self) {
        let waiters: Vec<oneshot::Sender<()>> =
            std::mem::take(&mut *self.0.nav_waiters.lock());
        for tx in waiters {
            let _ = tx.send(());
        }
    }

    fn register_nav_waiter(&self) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.0.nav_waiters.lock().push(tx);
        rx
    }

    async fn await_nav_ready(
        &self,
        expected_url: &str,
        timeout: Duration,
    ) -> Result<()> {
        let state = self
            .0
            .app
            .try_state::<FetchSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("fetch state not initialised"))?;

        if let Some(seen) = state.last_state_url() {
            if urls_logically_match(&seen, expected_url) {
                return Ok(());
            }
        }

        let rx = self.register_nav_waiter();

        if let Some(seen) = state.last_state_url() {
            if urls_logically_match(&seen, expected_url) {
                self.signal_nav_ready();
                return Ok(());
            }
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => Err(anyhow::anyhow!("nav ready channel closed")),
            Err(_) => Err(anyhow::anyhow!(
                "timed out waiting for fetch_worker navigation"
            )),
        }
    }

    async fn exec_internal(
        &self,
        kind: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<FetchResponse> {
        let _ = self
            .0
            .app
            .get_webview(FETCH_WORKER_LABEL)
            .ok_or_else(|| anyhow::anyhow!("fetch_worker webview not available"))?;
        self.sweep_stale_requests();
        let req_id = self.0.next_req_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.0
            .pending
            .lock()
            .insert(req_id, (std::time::Instant::now(), tx));
        let payload = serde_json::json!({
            "reqId": req_id,
            "kind": kind,
            "args": args,
        });
        let payload_js = serde_json::to_string(&payload)
            .with_context(|| "serialise fetch_worker exec payload")?;
        let source = format!(
            "window.__senFetchBridge && window.__senFetchBridge.exec({});",
            payload_js
        );
        if let Err(err) = run_fetch_on_main_thread(&self.0.app, "fetch_worker eval", move |app| {
            let webview = app
                .get_webview(FETCH_WORKER_LABEL)
                .ok_or_else(|| anyhow::anyhow!("fetch_worker webview not available"))?;
            webview
                .eval(&source)
                .map_err(|e| anyhow::anyhow!("{e}"))
        }) {
            self.forget_request(req_id);
            return Err(anyhow::anyhow!("fetch_worker eval failed: {err}"));
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => {
                self.forget_request(req_id);
                Err(anyhow::anyhow!(
                    "fetch_worker bridge channel closed before reply"
                ))
            }
            Err(_) => {
                self.forget_request(req_id);
                Err(anyhow::anyhow!(
                    "fetch_worker bridge timed out for kind={kind}"
                ))
            }
        }
    }
}

impl TauriFetchController {
    fn park_worker_to_blank(&self) {
        let outcome = run_fetch_on_main_thread(
            &self.0.app,
            "fetch_worker park",
            move |app| {
                let Some(webview) = app.get_webview(FETCH_WORKER_LABEL) else {
                    return Ok(());
                };
                if let Ok(current) = webview.url() {
                    if current.as_str() == ABOUT_BLANK {
                        return Ok(());
                    }
                }
                let parsed = Url::parse(ABOUT_BLANK)
                    .map_err(|e| anyhow::anyhow!("parse about:blank: {e}"))?;
                webview
                    .navigate(parsed)
                    .map_err(|e| anyhow::anyhow!("fetch_worker park navigate failed: {e}"))
            },
        );
        if let Err(err) = outcome {
            tracing::warn!("[fetch_worker] park to about:blank failed: {err}");
        }
    }

    async fn fetch_inner(&self, url: &str, timeout: Duration) -> Result<FetchedPage> {
        ensure_fetch_webview(&self.0.app)?;

        let state = self
            .0
            .app
            .try_state::<FetchSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("fetch state not initialised"))?;
        state.forget_state_url();

        let parsed = Url::parse(url)
            .map_err(|e| anyhow::anyhow!("invalid url '{url}': {e}"))?;
        let parsed_for_nav = parsed.clone();
        run_fetch_on_main_thread(&self.0.app, "fetch_worker navigate", move |app| {
            let webview = app
                .get_webview(FETCH_WORKER_LABEL)
                .ok_or_else(|| anyhow::anyhow!("fetch_worker webview missing"))?;
            webview
                .navigate(parsed_for_nav)
                .map_err(|e| anyhow::anyhow!("fetch_worker navigate failed: {e}"))
        })?;

        let nav_timeout = timeout.min(Duration::from_secs(8));
        if let Err(err) = self
            .await_nav_ready(parsed.as_str(), nav_timeout)
            .await
        {
            tracing::warn!("[fetch_worker] nav_ready: {err}");
        }

        let _ = self
            .exec_internal(
                "wait_for_ready",
                serde_json::json!({
                    "ready_state": "interactive",
                    "timeout_ms": 4_000,
                }),
                Duration::from_millis(5_000),
            )
            .await;

        let resp = self
            .exec_internal(
                "extract_text",
                Value::Null,
                Duration::from_millis(8_000),
            )
            .await?;
        if !resp.ok {
            return Err(anyhow::anyhow!(
                "fetch_worker extract_text failed: {}",
                resp.error.unwrap_or_default()
            ));
        }
        let mut page = page_from_extract_value(&resp.value);
        if page.text.chars().count() < 200 {
            tokio::time::sleep(Duration::from_millis(1_200)).await;
            if let Ok(retry) = self
                .exec_internal(
                    "extract_text",
                    Value::Null,
                    Duration::from_millis(8_000),
                )
                .await
            {
                if retry.ok {
                    let second = page_from_extract_value(&retry.value);
                    if second.text.chars().count() > page.text.chars().count() {
                        page = second;
                    }
                }
            }
        }
        Ok(page)
    }
}

fn page_from_extract_value(value: &Value) -> FetchedPage {
    FetchedPage {
        url: value
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        title: value
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        text: value
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

#[async_trait]
impl FetchController for TauriFetchController {
    async fn fetch(&self, url: &str, timeout: Duration) -> Result<FetchedPage> {
        let _drive = self.0.drive_lock.lock().await;
        let result = self.fetch_inner(url, timeout).await;
        self.park_worker_to_blank();
        result
    }
}

fn urls_logically_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let normalize = |s: &str| -> String {
        let no_fragment = s.split('#').next().unwrap_or(s);
        no_fragment.trim_end_matches('/').to_string()
    };
    normalize(a) == normalize(b)
}

static FETCH_MAIN_THREAD_ID: std::sync::OnceLock<std::thread::ThreadId> =
    std::sync::OnceLock::new();

fn record_fetch_main_thread() {
    let _ = FETCH_MAIN_THREAD_ID.set(std::thread::current().id());
}

fn fetch_thread_is_main() -> bool {
    FETCH_MAIN_THREAD_ID
        .get()
        .is_some_and(|id| *id == std::thread::current().id())
}

fn blocking_recv_without_starving_runtime<T>(
    rx: &std::sync::mpsc::Receiver<T>,
    timeout: Duration,
) -> Result<T, std::sync::mpsc::RecvTimeoutError> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle)
            if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread =>
        {
            tokio::task::block_in_place(|| rx.recv_timeout(timeout))
        }
        _ => rx.recv_timeout(timeout),
    }
}

fn run_fetch_on_main_thread<F>(app: &AppHandle, label: &'static str, f: F) -> Result<()>
where
    F: FnOnce(&AppHandle) -> Result<()> + Send + 'static,
{
    if fetch_thread_is_main() {
        return f(app);
    }
    let app_for_main = app.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&app_for_main));
    })
    .map_err(|e| anyhow::anyhow!("schedule {label} on the main thread failed: {e}"))?;
    blocking_recv_without_starving_runtime(&rx, Duration::from_secs(20))
        .map_err(|e| anyhow::anyhow!("await {label} result failed: {e}"))?
}

fn build_and_attach_fetch_webview(app: &AppHandle) -> Result<()> {
    if app.get_webview(FETCH_WORKER_LABEL).is_some() {
        return Ok(());
    }
    let main = app
        .get_window("main")
        .ok_or_else(|| anyhow::anyhow!("main window not yet available"))?;
    let bridge = build_fetch_bridge_js();
    let initial = Url::parse(ABOUT_BLANK)
        .map_err(|e| anyhow::anyhow!("parse about:blank: {e}"))?;
    let builder = WebviewBuilder::new(
        FETCH_WORKER_LABEL,
        WebviewUrl::External(initial),
    )
    .initialization_script(bridge)
    .additional_browser_args(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
         --autoplay-policy=document-user-activation-required \
         --disable-background-timer-throttling \
         --disable-renderer-backgrounding \
         --disable-backgrounding-occluded-windows",
    )
    .accept_first_mouse(false)
    .on_navigation(|target: &Url| {
        if target.scheme() == BRIDGE_SCHEME {
            return false;
        }
        if target.host_str() == Some(BRIDGE_HOST) {
            return false;
        }
        true
    })
    .on_new_window(|_url, _features| {
        tauri::webview::NewWindowResponse::Deny
    });

    main.add_child(
        builder,
        LogicalPosition::new(-32_000.0, -32_000.0),
        LogicalSize::new(1280.0, 800.0),
    )
    .map(|_| ())
    .map_err(|e| anyhow::anyhow!("add_child(fetch_worker) failed: {e}"))
}

fn ensure_fetch_webview(app: &AppHandle) -> Result<tauri::Webview> {
    if let Some(wv) = app.get_webview(FETCH_WORKER_LABEL) {
        return Ok(wv);
    }
    run_fetch_on_main_thread(app, "add_child(fetch_worker)", build_and_attach_fetch_webview)?;
    app.get_webview(FETCH_WORKER_LABEL)
        .ok_or_else(|| anyhow::anyhow!("fetch_worker webview missing after add_child"))
}

fn dispatch_fetch_event(app: &AppHandle, kind: &str, data_raw: Option<&str>) {
    if kind.is_empty() {
        return;
    }
    let parsed_data: Value = data_raw
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null);

    if kind == "result" {
        if let Some(controller) = app.try_state::<TauriFetchController>() {
            if let Some(req_id) = parsed_data.get("reqId").and_then(|v| v.as_u64()) {
                let ok = parsed_data
                    .get("ok")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let value = parsed_data
                    .get("value")
                    .cloned()
                    .unwrap_or(Value::Null);
                let error = parsed_data
                    .get("error")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                controller.deliver_result(req_id, ok, value, error);
                return;
            }
        }
    }

    if kind == "result_chunk" {
        if let Some(controller) = app.try_state::<TauriFetchController>() {
            let req_id = parsed_data.get("reqId").and_then(|v| v.as_u64());
            let seq = parsed_data.get("seq").and_then(|v| v.as_u64());
            let total = parsed_data.get("total").and_then(|v| v.as_u64());
            let payload = parsed_data
                .get("payload")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            if let (Some(req_id), Some(seq), Some(total), Some(payload)) =
                (req_id, seq, total, payload)
            {
                controller.deliver_chunk(
                    req_id,
                    seq as usize,
                    total as usize,
                    payload,
                );
                return;
            }
        }
    }

    if kind == "state" {
        if let Some(state) = app.try_state::<FetchSharedState>() {
            if let Some(url) = parsed_data.get("url").and_then(|v| v.as_str()) {
                state.record_state_url(url);
            }
        }
        if let Some(controller) = app.try_state::<TauriFetchController>() {
            controller.signal_nav_ready();
        }
    }
}

pub fn handle_protocol_path(
    app: &AppHandle,
    segment: &str,
    query: Option<&str>,
) -> bool {
    if !segment.eq_ignore_ascii_case("fetch_event") {
        return false;
    }
    let mut kind = String::new();
    let mut data_raw: Option<String> = None;
    if let Some(query) = query {
        let stitched = format!("http://x/?{query}");
        if let Ok(parsed) = Url::parse(&stitched) {
            for (k, v) in parsed.query_pairs() {
                if k == "kind" {
                    kind = v.into_owned();
                } else if k == "data" {
                    data_raw = Some(v.into_owned());
                }
            }
        }
    }
    if !kind.is_empty() {
        dispatch_fetch_event(app, &kind, data_raw.as_deref());
    }
    true
}

pub fn install_into(app: &AppHandle) {
    record_fetch_main_thread();
    let controller = TauriFetchController::new(app.clone());
    app.manage(controller.clone());
    app.manage(FetchSharedState::new());
    install_fetch_controller(Arc::new(controller));

    let app_for_init = app.clone();
    if let Err(err) = app.run_on_main_thread(move || {
        if let Err(err) = ensure_fetch_webview(&app_for_init) {
            tracing::warn!("[fetch_worker] pre-create webview failed: {err}");
        }
    }) {
        tracing::warn!(
            "[fetch_worker] schedule pre-create on main thread failed: {err}"
        );
    }
}
