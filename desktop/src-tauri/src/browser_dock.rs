// SPDX-License-Identifier: MIT
//
//! Embedded Browser dock — a child Tauri webview that the React shell
//! parents to a placeholder div directly above the chat composer, *and*
//! the surface the agent's `browser` tool drives in real time.
//!
//! Architecture
//! ============
//!
//! Tauri 2's multi-webview API ([`Webview`] inside an existing
//! [`WebviewWindow`]) lets us add a second wry/WebView2/WebKitGTK surface
//! to the same OS window as the React shell. The React side keeps a
//! [`ResizeObserver`] over a placeholder `<div>`; on every layout change
//! it sends the new physical rect to [`browser_dock_set_rect`] which calls
//! `Webview::set_position` + `set_size`.
//!
//! IPC
//! ===
//!
//! Cross-origin web pages can't access `window.__TAURI_INTERNALS__`, so
//! the injected [`BRIDGE_JS`] communicates back to Rust via the
//! `senbridge://` custom scheme. Every event navigates a hidden anchor
//! to `senbridge://event?kind=…&data=…`; [`WebviewBuilder::on_navigation`]
//! intercepts the URL, parses the fragment and dispatches an
//! `browser_dock_event` Tauri event up to the main webview where the
//! `browserPanelStore` consumes it.
//!
//! Agent driver
//! ============
//!
//! [`TauriDockController`] implements
//! [`senweavercoding::tools::browser::DockController`] and is
//! registered through `install_dock_controller` inside the Tauri
//! `setup` hook so the agent's `BrowserTool` can drive the visible
//! dock directly through its `tauri_dock` backend (preferred under
//! `auto`).
//!
//! Each [`DockController::exec`] call:
//! 1. Awaits a per-controller `tokio::Mutex` so concurrent subagents
//!    serialise on the singleton dock.
//! 2. Generates a fresh `reqId`, registers a `oneshot::Sender` in the
//!    pending map, then evals
//!    `window.__senDockBridge.exec({reqId, kind, args})` in the dock
//!    webview.
//! 3. The injected JS performs the action against the page's main
//!    world (no dynamic `eval` — every kind is a static handler) and
//!    posts back a `result` event containing the same `reqId`. The
//!    `senbridge://event?kind=result&...` arm of [`dispatch_bridge_event`]
//!    routes that envelope to the oneshot.
//! 4. Failures, timeouts and navigations all drain the pending
//!    senders so the agent never deadlocks waiting for a page that
//!    moved on.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use senweavercoding::tools::browser::{
    clear_test_target_tab, current_test_target_tab, set_test_target_tab, DockController,
    DockRequest, DockResponse, DockTabInfo,
};
use serde::Deserialize;
use serde_json::Value;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl, Window,
    webview::WebviewBuilder,
};
use tokio::sync::{Mutex as AsyncMutex, oneshot};

const ABOUT_BLANK: &str = "about:blank";

const BRIDGE_SCHEME: &str = "senbridge";

const BRIDGE_HOST: &str = "senbridge.localhost";

fn bridge_base_url() -> &'static str {
    if cfg!(any(target_os = "windows", target_os = "android")) {
        "http://senbridge.localhost"
    } else {
        "senbridge://localhost"
    }
}

fn build_bridge_js() -> String {
    let header = format!(
        "window.__SEN_BRIDGE_BASE = {base:?};",
        base = bridge_base_url(),
    );
    format!("{header}\n{BRIDGE_JS}")
}

const BRIDGE_JS: &str = r#"
(() => {
  if (window.__senDockBridge) return;

  const BRIDGE_BASE = window.__SEN_BRIDGE_BASE || 'http://senbridge.localhost';

  function send(kind, data) {
    try {
      const params = new URLSearchParams();
      params.set('kind', kind);
      params.set('data', JSON.stringify(data ?? null));
      const url = `${BRIDGE_BASE}/event?${params.toString()}`;
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

  const ringMax = 256;
  const consoleRing = [];
  const wrapConsole = (level) => {
    const orig = console[level] && console[level].bind(console);
    if (!orig) return;
    console[level] = (...args) => {
      try {
        const message = args.map((a) => {
          if (typeof a === 'string') return a;
          try { return JSON.stringify(a); } catch (_) { return String(a); }
        }).join(' ');
        consoleRing.push({ level, message, ts: Date.now() });
        while (consoleRing.length > ringMax) consoleRing.shift();
        send('console', { level, message, ts: Date.now() });
      } catch (_) {}
      return orig(...args);
    };
  };
  ['log', 'info', 'warn', 'error', 'debug'].forEach(wrapConsole);

  window.addEventListener('error', (ev) => {
    try {
      const entry = { level: 'error', message: `[uncaught] ${ev.message} (${ev.filename}:${ev.lineno})`, ts: Date.now() };
      consoleRing.push(entry);
      while (consoleRing.length > ringMax) consoleRing.shift();
      send('console', entry);
    } catch (_) {}
  });
  window.addEventListener('unhandledrejection', (ev) => {
    try {
      const reason = ev.reason && (ev.reason.stack || ev.reason.message || String(ev.reason));
      const entry = { level: 'error', message: `[unhandledrejection] ${reason}`, ts: Date.now() };
      consoleRing.push(entry);
      while (consoleRing.length > ringMax) consoleRing.shift();
      send('console', entry);
    } catch (_) {}
  });

  let netInflight = 0;
  let netLastActive = Date.now();
  const netErrorsMax = 64;
  const netErrors = [];
  function recordNetError(entry) {
    try {
      const safe = {
        ts: Number(entry && entry.ts) || Date.now(),
        method: String((entry && entry.method) || 'GET').toUpperCase(),
        url: String((entry && entry.url) || ''),
        status: Number((entry && entry.status) || 0),
        duration_ms: Number((entry && entry.duration_ms) || 0),
        page_url: window.location && window.location.href ? window.location.href : '',
      };
      if (!safe.url) return;
      netErrors.push(safe);
      while (netErrors.length > netErrorsMax) netErrors.shift();
      send('network_error', safe);
    } catch (_) {}
  }
  function normaliseFetchUrl(input) {
    try {
      if (typeof input === 'string') return input;
      if (input && typeof input.url === 'string') return input.url;
    } catch (_) {}
    return '';
  }
  function normaliseFetchMethod(input, init) {
    try {
      if (init && typeof init.method === 'string') return init.method;
      if (input && typeof input.method === 'string') return input.method;
    } catch (_) {}
    return 'GET';
  }
  function markNetStart() {
    netInflight += 1;
    netLastActive = Date.now();
  }
  function markNetEnd() {
    if (netInflight > 0) netInflight -= 1;
    netLastActive = Date.now();
  }
  try {
    const origFetch = window.fetch ? window.fetch.bind(window) : null;
    if (origFetch) {
      window.fetch = function patchedFetch(input, init) {
        markNetStart();
        const startedAt = Date.now();
        const reqUrl = normaliseFetchUrl(input);
        const reqMethod = normaliseFetchMethod(input, init);
        let promise;
        try { promise = origFetch(input, init); } catch (err) { markNetEnd(); throw err; }
        return promise.then((res) => {
          markNetEnd();
          try {
            if (res && typeof res.status === 'number' && res.status >= 400) {
              recordNetError({
                ts: Date.now(),
                method: reqMethod,
                url: (res && res.url) || reqUrl,
                status: res.status,
                duration_ms: Date.now() - startedAt,
              });
            }
          } catch (_) {}
          return res;
        }, (err) => {
          markNetEnd();
          try {
            recordNetError({
              ts: Date.now(),
              method: reqMethod,
              url: reqUrl,
              status: 0,
              duration_ms: Date.now() - startedAt,
            });
          } catch (_) {}
          throw err;
        });
      };
    }
  } catch (_) {}
  try {
    const XHR = window.XMLHttpRequest;
    if (XHR && XHR.prototype) {
      const origOpen = XHR.prototype.open;
      const origSend = XHR.prototype.send;
      XHR.prototype.open = function patchedOpen(method, url) {
        try {
          this.__senTracked = true;
          this.__senMethod = (typeof method === 'string' ? method : 'GET');
          this.__senUrl = (typeof url === 'string' ? url : '');
        } catch (_) {}
        return origOpen.apply(this, arguments);
      };
      XHR.prototype.send = function patchedSend(...args) {
        if (this.__senTracked) {
          markNetStart();
          const startedAt = Date.now();
          const xhrRef = this;
          const finish = () => {
            markNetEnd();
            try {
              const status = Number(xhrRef.status) || 0;
              if (status >= 400 || status === 0) {
                recordNetError({
                  ts: Date.now(),
                  method: xhrRef.__senMethod || 'GET',
                  url: xhrRef.responseURL || xhrRef.__senUrl || '',
                  status,
                  duration_ms: Date.now() - startedAt,
                });
              }
            } catch (_) {}
          };
          this.addEventListener('loadend', finish, { once: true });
          this.addEventListener('abort', finish, { once: true });
          this.addEventListener('error', finish, { once: true });
        }
        return origSend.apply(this, args);
      };
    }
  } catch (_) {}
  try {
    const NativeWS = window.WebSocket;
    if (NativeWS) {
      const Patched = function PatchedWebSocket(url, protocols) {
        const ws = protocols == null ? new NativeWS(url) : new NativeWS(url, protocols);
        markNetStart();
        let closed = false;
        const finish = () => { if (!closed) { closed = true; markNetEnd(); } };
        try {
          ws.addEventListener('close', finish);
          ws.addEventListener('error', finish);
        } catch (_) {}
        return ws;
      };
      Patched.prototype = NativeWS.prototype;
      try { window.WebSocket = Patched; } catch (_) {}
    }
  } catch (_) {}

  function isOpenableHttpUrl(absolute) {
    if (typeof absolute !== 'string') return false;
    const lower = absolute.toLowerCase();
    return lower.startsWith('http:') || lower.startsWith('https:')
      || lower.startsWith('ftp:') || lower.startsWith('file:');
  }
  function resolveAbsoluteUrl(href) {
    if (href == null) return '';
    const raw = String(href).trim();
    if (!raw) return '';
    try { return new URL(raw, window.location.href).toString(); } catch (_) { return ''; }
  }
  function findAnchorAncestor(start) {
    let el = start;
    while (el && el.nodeType === 1) {
      if (el.tagName === 'A' && (el.getAttribute('href') || el.href)) return el;
      el = el.parentElement;
    }
    return null;
  }
  function shouldOpenInNewTab(anchor, ev) {
    if (!anchor) return false;
    const target = (anchor.getAttribute('target') || '').toLowerCase();
    if (target === '_blank' || target === 'blank' || target === 'new') return true;
    if (ev && (ev.ctrlKey || ev.metaKey || ev.shiftKey)) return true;
    if (ev && (ev.button === 1 || ev.type === 'auxclick')) return true;
    return false;
  }
  function interceptAnchorClick(ev) {
    try {
      if (pickMode) return;
      if (!ev || ev.defaultPrevented) return;
      if (ev.button !== undefined && ev.button !== 0 && ev.button !== 1) return;
      const anchor = findAnchorAncestor(ev.target);
      if (!anchor) return;
      if (!shouldOpenInNewTab(anchor, ev)) return;
      const href = anchor.getAttribute('href') || anchor.href || '';
      const absolute = resolveAbsoluteUrl(href);
      if (!isOpenableHttpUrl(absolute)) return;
      ev.preventDefault();
      ev.stopPropagation();
      if (typeof ev.stopImmediatePropagation === 'function') ev.stopImmediatePropagation();
      send('openNewTab', { url: absolute, source: 'anchor' });
    } catch (_) {}
  }
  document.addEventListener('click', interceptAnchorClick, true);
  document.addEventListener('auxclick', interceptAnchorClick, true);

  try {
    const nativeOpen = (typeof window.open === 'function') ? window.open.bind(window) : null;
    const stubWindow = (absolute) => ({
      closed: false,
      location: { href: absolute || 'about:blank' },
      focus() {},
      blur() {},
      close() { this.closed = true; },
      postMessage() {},
      opener: null,
      document: null,
      addEventListener() {},
      removeEventListener() {},
    });
    const shimmedOpen = function (url, target, features) {
      try {
        const absolute = resolveAbsoluteUrl(url);
        if (isOpenableHttpUrl(absolute)) {
          send('openNewTab', { url: absolute, source: 'window.open' });
          return stubWindow(absolute);
        }
      } catch (_) {}
      if (nativeOpen) {
        try { return nativeOpen(url, target, features); } catch (_) {}
      }
      return stubWindow('about:blank');
    };
    try { window.open = shimmedOpen; } catch (_) {}
    try {
      Object.defineProperty(window, 'open', {
        configurable: true,
        writable: true,
        value: shimmedOpen,
      });
    } catch (_) {}
  } catch (_) {}

  let pickMode = false;
  let lastHover = null;
  const STYLE_ID = '__sen_dock_pick_style';
  function ensurePickStyle() {
    if (document.getElementById(STYLE_ID)) return;
    const s = document.createElement('style');
    s.id = STYLE_ID;
    s.textContent = '.__sen_dock_outline { outline: 2px solid #2563eb !important; outline-offset: 2px !important; cursor: pointer !important; }';
    (document.head || document.documentElement).appendChild(s);
  }
  function clearOutline() {
    if (lastHover && lastHover.classList) lastHover.classList.remove('__sen_dock_outline');
    lastHover = null;
  }
  function selectorOf(el) {
    if (!el || el.nodeType !== 1) return '';
    if (el.id) return '#' + CSS.escape(el.id);
    const path = [];
    let cur = el;
    while (cur && cur.nodeType === 1 && path.length < 6 && cur !== document.body) {
      let part = cur.tagName.toLowerCase();
      if (typeof cur.className === 'string' && cur.className.trim()) {
        const classes = cur.className.trim().split(/\s+/).filter((c) => c && !c.startsWith('__sen_dock')).slice(0, 2);
        if (classes.length) part += '.' + classes.map((c) => CSS.escape(c)).join('.');
      }
      const parent = cur.parentNode;
      if (parent && parent.children) {
        const sib = Array.from(parent.children).filter((c) => c.tagName === cur.tagName);
        if (sib.length > 1) part += `:nth-of-type(${sib.indexOf(cur) + 1})`;
      }
      path.unshift(part);
      cur = cur.parentNode;
    }
    return path.join(' > ');
  }
  function computedStyleOf(el) {
    try {
      const cs = getComputedStyle(el);
      const props = ['display', 'position', 'box-sizing', 'width', 'height', 'margin', 'padding',
        'color', 'background-color', 'font-family', 'font-size', 'font-weight', 'line-height',
        'border', 'border-radius', 'box-shadow', 'opacity', 'z-index', 'flex', 'gap',
        'grid-template-columns', 'grid-template-rows', 'transform', 'transition'];
      const out = {};
      props.forEach((p) => { out[p] = cs.getPropertyValue(p); });
      const rect = el.getBoundingClientRect();
      out['__rect__'] = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
      return out;
    } catch (_) { return {}; }
  }
  function onHover(e) {
    if (!pickMode) return;
    clearOutline();
    if (e.target && e.target.classList) {
      lastHover = e.target;
      lastHover.classList.add('__sen_dock_outline');
    }
  }
  function onClick(e) {
    if (!pickMode) return;
    e.preventDefault();
    e.stopPropagation();
    const el = e.target;
    const sel = selectorOf(el);
    const props = computedStyleOf(el);
    send('pick', { selector: sel, text: (el.innerText || el.textContent || '').slice(0, 240), props });
    pickMode = false;
    clearOutline();
    document.removeEventListener('mouseover', onHover, true);
    document.removeEventListener('click', onClick, true);
  }

  function snapshot() {
    send('state', {
      url: window.location.href,
      title: document.title || '',
      canBack: history.length > 1,
      ts: Date.now(),
    });
  }

  // -------- Agent driver: static handlers (no dynamic eval) --------
  // Each handler receives `(args)` and returns a value (or thenable);
  // throwing rejects the round-trip with the error text.
  //
  // Result transport
  // ----------------
  // Results travel back as `senbridge://event?kind=result&data=…` URLs
  // intercepted by `WebviewBuilder::on_navigation`.  Because URL length
  // limits vary across WebView2 / WebKitGTK / WKWebView, payloads
  // larger than ~16KB are split into `result_chunk` frames keyed by
  // `(reqId, seq, total)`; the Rust controller reassembles them and
  // signals the awaiting oneshot only when the final frame arrives.
  //
  // The threshold is generous on purpose: small results stay on the
  // single-frame fast-path, only `snapshot`-style payloads pay the
  // chunking cost.
  const RESULT_CHUNK_BYTES = 14000;
  function postResult(reqId, ok, value, error) {
    let envelopeJson;
    try {
      envelopeJson = JSON.stringify({ ok: !!ok, value: value ?? null, error: error ?? null });
    } catch (err) {
      envelopeJson = JSON.stringify({ ok: false, value: null, error: 'json serialise: ' + String(err && err.message || err) });
    }
    if (envelopeJson.length <= RESULT_CHUNK_BYTES) {
      send('result', { reqId, ok: !!ok, value: value ?? null, error: error ?? null });
      return;
    }
    const total = Math.ceil(envelopeJson.length / RESULT_CHUNK_BYTES);
    for (let i = 0; i < total; i += 1) {
      const start = i * RESULT_CHUNK_BYTES;
      const slice = envelopeJson.slice(start, start + RESULT_CHUNK_BYTES);
      send('result_chunk', { reqId, seq: i, total, payload: slice });
    }
  }
  function escapeAttr(value) {
    return String(value).replace(/\\/g, '\\\\').replace(/"/g, '\\"');
  }
  function resolveSelector(selector) {
    if (selector == null) return null;
    const raw = String(selector).trim();
    if (!raw) return null;
    if (raw.charAt(0) === '@') {
      try {
        return document.querySelector('[data-zc-ref="' + escapeAttr(raw) + '"]');
      } catch (_) { return null; }
    }
    const lower = raw.toLowerCase();
    if (lower.indexOf('text=') === 0) {
      const needle = raw.slice(5).trim();
      if (!needle) return null;
      const all = document.body ? document.body.querySelectorAll('*') : [];
      for (const el of all) {
        if (!(el instanceof Element)) continue;
        const inner = (el.innerText || el.textContent || '').replace(/\s+/g, ' ').trim();
        if (inner === needle) return el;
      }
      for (const el of all) {
        if (!(el instanceof Element)) continue;
        const inner = (el.innerText || el.textContent || '').replace(/\s+/g, ' ').trim();
        if (inner.indexOf(needle) >= 0) return el;
      }
      return null;
    }
    if (lower.indexOf('label=') === 0) {
      const needle = raw.slice(6).trim();
      if (!needle) return null;
      const labels = Array.from(document.querySelectorAll('label'));
      for (const label of labels) {
        const txt = (label.innerText || label.textContent || '').replace(/\s+/g, ' ').trim();
        if (txt.indexOf(needle) < 0) continue;
        const forId = label.getAttribute('for');
        if (forId) {
          const target = document.getElementById(forId);
          if (target) return target;
        }
        const inner = label.querySelector('input,textarea,select,button');
        if (inner) return inner;
      }
      const aria = document.querySelector('[aria-label="' + escapeAttr(needle) + '"]');
      if (aria) return aria;
      return null;
    }
    try { return document.querySelector(raw); } catch (_) { return null; }
  }
  function findOne(selector) {
    if (!selector) throw new Error('selector is required');
    const el = resolveSelector(selector);
    if (!el) throw new Error(`element not found: ${selector}`);
    return el;
  }
  function rectOf(el) {
    try { const r = el.getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; }
    catch (_) { return null; }
  }
  function dispatchSyntheticInput(el, value) {
    const proto = (el instanceof HTMLTextAreaElement) ? HTMLTextAreaElement.prototype
                : (el instanceof HTMLSelectElement) ? HTMLSelectElement.prototype
                : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, 'value');
    if (setter && typeof setter.set === 'function') {
      setter.set.call(el, value);
    } else {
      el.value = value;
    }
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  }
  function dispatchKey(el, key) {
    ['keydown', 'keypress', 'keyup'].forEach((kind) => {
      try { el.dispatchEvent(new KeyboardEvent(kind, { key, bubbles: true, cancelable: true })); } catch (_) {}
    });
  }
  function isVisibleEl(el) {
    if (!(el instanceof Element)) return false;
    const cs = getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  }
  function snapshotTree(opts) {
    const interactiveOnly = !!(opts && opts.interactive_only);
    const compact = !!(opts && opts.compact);
    const maxDepth = (opts && typeof opts.depth === 'number') ? opts.depth : 24;
    let refSeq = 0;
    function visit(el, depth) {
      if (!el || depth > maxDepth) return null;
      if (!(el instanceof Element)) return null;
      const tag = el.tagName.toLowerCase();
      const interactive = ['a', 'button', 'input', 'select', 'textarea', 'option', 'label', 'summary'].indexOf(tag) >= 0
        || el.getAttribute('role') === 'button'
        || el.getAttribute('contenteditable') === 'true'
        || (typeof el.onclick === 'function');
      if (interactiveOnly && !interactive && depth > 0) {
        const acc = [];
        for (const c of el.children) { const v = visit(c, depth + 1); if (v) acc.push(v); }
        return acc.length === 1 ? acc[0] : (acc.length ? { tag: 'group', children: acc } : null);
      }
      const ref = '@e' + (++refSeq);
      el.setAttribute('data-zc-ref', ref);
      const node = {
        ref,
        tag,
        text: compact ? undefined : (el.innerText || '').slice(0, 200),
        attrs: compact ? undefined : (() => {
          const out = {};
          for (const a of el.attributes) {
            if (a.name === 'data-zc-ref') continue;
            out[a.name] = (a.value || '').slice(0, 200);
          }
          return out;
        })(),
        rect: compact ? undefined : rectOf(el),
        interactive: interactive || undefined,
      };
      if (depth < maxDepth && el.children && el.children.length) {
        const children = [];
        for (const c of el.children) { const v = visit(c, depth + 1); if (v) children.push(v); }
        if (children.length) node.children = children;
      }
      return node;
    }
    return {
      url: window.location.href,
      title: document.title || '',
      tree: visit(document.body, 0),
    };
  }

  const handlers = {
    navigate(args) {
      const url = args && args.url;
      if (!url) throw new Error('url is required');
      window.location.assign(url);
      return { navigated: true, url };
    },
    get_html(args) {
      const sel = args && args.selector;
      if (sel) {
        const el = findOne(sel);
        return { selector: sel, html: (el.outerHTML || '').slice(0, 60000) };
      }
      return { html: (document.documentElement && document.documentElement.outerHTML || '').slice(0, 60000) };
    },
    get_attribute(args) {
      const el = findOne(args && args.selector);
      const name = (args && (args.name || args.attribute)) || '';
      if (!name) throw new Error('attribute name is required');
      let value;
      if (name === 'value') {
        if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
          value = el.value;
        } else {
          value = el.getAttribute('value');
        }
      } else if (name === 'checked') {
        value = el instanceof HTMLInputElement ? String(el.checked) : el.getAttribute('checked');
      } else {
        value = el.getAttribute(name);
      }
      return { selector: args.selector, name, attribute: name, value };
    },
    count(args) {
      const sel = args && args.selector;
      if (!sel) throw new Error('selector is required');
      try {
        const all = document.querySelectorAll(sel);
        return { selector: sel, count: all.length };
      } catch (err) {
        return { selector: sel, count: 0, error: String(err && err.message || err) };
      }
    },
    console_logs(args) {
      const level = args && args.level;
      const since = (args && Number(args.since_ms)) || 0;
      const limit = (args && Number(args.limit)) || 0;
      let entries = consoleRing.slice();
      if (level) entries = entries.filter((e) => e.level === level);
      if (since > 0) entries = entries.filter((e) => Number(e.ts) >= since);
      if (limit > 0 && entries.length > limit) entries = entries.slice(entries.length - limit);
      if (args && args.clear_after) {
        consoleRing.length = 0;
      }
      return { entries, count: entries.length, buffered: consoleRing.length };
    },
    network_errors(args) {
      const since = (args && Number(args.since_ms)) || 0;
      const limit = (args && Number(args.limit)) || 0;
      let entries = netErrors.slice();
      if (since > 0) entries = entries.filter((e) => Number(e.ts) >= since);
      if (limit > 0 && entries.length > limit) entries = entries.slice(entries.length - limit);
      return { entries, count: entries.length, buffered: netErrors.length, page_url: window.location ? window.location.href : '' };
    },
    collect_links(args) {
      const sameOrigin = !!(args && args.same_origin);
      const limit = (args && Number(args.limit)) || 0;
      const seen = new Set();
      const out = [];
      let origin = '';
      try { origin = window.location && window.location.origin ? window.location.origin : ''; } catch (_) {}
      const els = document.querySelectorAll('a[href], [role="link"], form[action]');
      for (let i = 0; i < els.length; i += 1) {
        const el = els[i];
        let rawUrl = '';
        try {
          if (typeof el.href === 'string' && el.href) {
            rawUrl = el.href;
          } else if (el.getAttribute) {
            rawUrl = el.getAttribute('action') || el.getAttribute('href') || '';
          }
        } catch (_) { rawUrl = ''; }
        if (!rawUrl) continue;
        let absolute = '';
        try { absolute = new URL(rawUrl, window.location.href).toString(); } catch (_) { absolute = ''; }
        if (!absolute) continue;
        if (sameOrigin && origin) {
          try {
            const candidateOrigin = new URL(absolute).origin;
            if (candidateOrigin !== origin) continue;
          } catch (_) { continue; }
        }
        if (seen.has(absolute)) continue;
        seen.add(absolute);
        let text = '';
        try {
          if (el instanceof HTMLInputElement || el instanceof HTMLButtonElement) {
            text = String(el.value || el.innerText || '');
          } else {
            text = String(el.innerText || el.textContent || '');
          }
        } catch (_) { text = ''; }
        text = text.trim().replace(/\s+/g, ' ').slice(0, 120);
        let tag = '';
        try { tag = (el.tagName || '').toLowerCase(); } catch (_) { tag = ''; }
        out.push({ url: absolute, text, type: tag || 'link' });
        if (limit > 0 && out.length >= limit) break;
      }
      return { links: out, count: out.length, same_origin: sameOrigin, origin };
    },
    network_snapshot() {
      return { inflight: netInflight, idle_ms: Date.now() - netLastActive };
    },
    network_idle(args) {
      const idleMs = (args && Number(args.idle_ms)) || 500;
      const timeoutMs = (args && Number(args.timeout_ms)) || 15000;
      const start = Date.now();
      return new Promise((resolve, reject) => {
        const tick = () => {
          if (netInflight === 0 && (Date.now() - netLastActive) >= idleMs) {
            resolve({ idle_ms: Date.now() - netLastActive, elapsed_ms: Date.now() - start });
            return;
          }
          if (Date.now() - start >= timeoutMs) {
            reject(new Error('network_idle timeout'));
            return;
          }
          setTimeout(tick, 100);
        };
        tick();
      });
    },
    clear_storage(args) {
      const scope = (args && String(args.scope || 'all')).toLowerCase();
      const out = { scope, cookies: false, storage: false, cache: false, indexeddb: false };
      try {
        if (scope === 'all' || scope === 'local') {
          try { localStorage.clear(); } catch (_) {}
        }
        if (scope === 'all' || scope === 'session') {
          try { sessionStorage.clear(); } catch (_) {}
        }
        out.storage = true;
      } catch (_) {}
      if (scope === 'all' || scope === 'cookies') {
        try {
          document.cookie.split(';').forEach((c) => {
            const eq = c.indexOf('=');
            const name = eq > -1 ? c.slice(0, eq).trim() : c.trim();
            if (!name) return;
            document.cookie = `${name}=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/`;
          });
          out.cookies = true;
        } catch (_) {}
      }
      if (scope === 'all' || scope === 'cache') {
        try {
          if (window.caches && caches.keys) {
            return caches.keys()
              .then((names) => Promise.all(names.map((n) => caches.delete(n))))
              .then(() => { out.cache = true; return out; })
              .catch(() => out);
          }
        } catch (_) {}
      }
      if (scope === 'all' || scope === 'indexeddb') {
        try {
          if (indexedDB && indexedDB.databases) {
            return indexedDB.databases().then((dbs) => {
              dbs.forEach((db) => { try { if (db.name) indexedDB.deleteDatabase(db.name); } catch (_) {} });
              out.indexeddb = true;
              return out;
            }).catch(() => out);
          }
        } catch (_) {}
      }
      return out;
    },
    history_back() {
      try { window.history.back(); } catch (_) {}
      return { back: true };
    },
    history_forward() {
      try { window.history.forward(); } catch (_) {}
      return { forward: true };
    },
    history_reload() {
      try { window.location.reload(); } catch (_) {}
      return { reloaded: true };
    },
    select_option(args) {
      const el = findOne(args && args.selector);
      const value = args && args.value;
      const label = args && args.label;
      if (!(el instanceof HTMLSelectElement)) {
        throw new Error('select_option requires a <select> element');
      }
      let chosen = null;
      if (value != null) {
        chosen = Array.from(el.options).find((opt) => opt.value === String(value)) || null;
      } else if (label != null) {
        chosen = Array.from(el.options).find((opt) => (opt.text || '').trim() === String(label)) || null;
      }
      if (!chosen) throw new Error('option not found');
      el.value = chosen.value;
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
      return { selected: chosen.value, label: chosen.text };
    },
    click(args) {
      const el = findOne(args && args.selector);
      try { el.scrollIntoView({ block: 'center', inline: 'center' }); } catch (_) {}
      const r = el.getBoundingClientRect();
      const opts = {
        bubbles: true,
        cancelable: true,
        clientX: r.left + r.width / 2,
        clientY: r.top + r.height / 2,
        button: 0,
      };
      try { el.dispatchEvent(new PointerEvent('pointerdown', { ...opts, pointerType: 'mouse', isPrimary: true })); } catch (_) {}
      try { el.dispatchEvent(new MouseEvent('mousedown', opts)); } catch (_) {}
      try { el.dispatchEvent(new PointerEvent('pointerup', { ...opts, pointerType: 'mouse', isPrimary: true })); } catch (_) {}
      try { el.dispatchEvent(new MouseEvent('mouseup', opts)); } catch (_) {}
      try { el.click(); } catch (_) {}
      return { clicked: args.selector, rect: rectOf(el) };
    },
    set_value(args) {
      const el = findOne(args && args.selector);
      try { el.focus(); } catch (_) {}
      dispatchSyntheticInput(el, args.value ?? '');
      return { filled: args.selector };
    },
    type_text(args) {
      const el = findOne(args && args.selector);
      try { el.focus(); } catch (_) {}
      const next = (el.value ?? '') + (args.text ?? '');
      dispatchSyntheticInput(el, next);
      return { typed: args.selector, length: (args.text ?? '').length };
    },
    press_key(args) {
      const el = document.activeElement || document.body;
      dispatchKey(el, args && args.key ? args.key : '');
      return { pressed: args && args.key };
    },
    hover(args) {
      const el = findOne(args && args.selector);
      try {
        const r = el.getBoundingClientRect();
        const opts = { bubbles: true, cancelable: true, clientX: r.left + r.width/2, clientY: r.top + r.height/2 };
        el.dispatchEvent(new MouseEvent('mouseover', opts));
        el.dispatchEvent(new MouseEvent('mouseenter', opts));
        el.dispatchEvent(new MouseEvent('mousemove', opts));
      } catch (_) {}
      return { hovered: args.selector };
    },
    scroll(args) {
      const dir = (args && args.direction) || 'down';
      const px = (args && args.pixels) || 400;
      let dx = 0, dy = 0;
      if (dir === 'down') dy = px;
      else if (dir === 'up') dy = -px;
      else if (dir === 'right') dx = px;
      else if (dir === 'left') dx = -px;
      window.scrollBy({ top: dy, left: dx, behavior: 'instant' in window ? 'instant' : 'auto' });
      return { x: window.scrollX, y: window.scrollY };
    },
    is_visible(args) {
      const el = resolveSelector(args && args.selector);
      return { selector: args && args.selector, visible: isVisibleEl(el) };
    },
    get_text(args) {
      const el = findOne(args && args.selector);
      return { selector: args.selector, text: (el.innerText || el.textContent || '').slice(0, 4000) };
    },
    get_title() { return { title: document.title || '' }; },
    get_url() { return { url: window.location.href }; },
    snapshot(args) { return snapshotTree(args); },
    wait_for(args) {
      const sel = args && args.selector;
      const text = args && args.text;
      let readyState = args && (args.ready_state || args.readyState);
      const until = args && args.until;
      if (until === 'load') readyState = 'complete';
      else if (until === 'dom_content_loaded') readyState = 'interactive';
      const timeoutMs = (args && (args.timeout_ms || args.ms)) || 15000;
      if (until === 'network_idle') {
        const idleMs = (args && Number(args.idle_ms)) || 500;
        const start = Date.now();
        return new Promise((resolve, reject) => {
          const tick = () => {
            if (netInflight === 0 && (Date.now() - netLastActive) >= idleMs) {
              resolve({ until: 'network_idle', idle_ms: Date.now() - netLastActive, elapsed_ms: Date.now() - start });
              return;
            }
            if (Date.now() - start >= timeoutMs) { reject(new Error('wait_for network_idle timeout')); return; }
            setTimeout(tick, 100);
          };
          tick();
        });
      }
      const onlyMs = !sel && !text && !readyState && (args && (args.ms != null));
      if (onlyMs) {
        const ms = Number(args.ms) || 0;
        return new Promise((resolve) => {
          setTimeout(() => resolve({ slept_ms: ms }), ms);
        });
      }
      return new Promise((resolve, reject) => {
        const start = Date.now();
        function check() {
          if (sel) {
            const el = resolveSelector(sel);
            if (el && isVisibleEl(el)) { resolve({ found: true, selector: sel, elapsed_ms: Date.now() - start }); return true; }
          }
          if (text) {
            const body = document.body && document.body.innerText ? document.body.innerText : '';
            if (body.indexOf(String(text)) >= 0) { resolve({ found: true, text, elapsed_ms: Date.now() - start }); return true; }
          }
          if (readyState) {
            const target = String(readyState).toLowerCase();
            const cur = String(document.readyState || '').toLowerCase();
            const ok = target === 'complete'
              ? cur === 'complete'
              : target === 'interactive'
                ? (cur === 'interactive' || cur === 'complete')
                : cur === target;
            if (ok) { resolve({ ready_state: cur, elapsed_ms: Date.now() - start }); return true; }
          }
          return false;
        }
        if (check()) return;
        const onState = () => { if (check()) { obs.disconnect(); clearTimeout(to); document.removeEventListener('readystatechange', onState, true); } };
        const obs = new MutationObserver(() => { if (check()) { obs.disconnect(); clearTimeout(to); document.removeEventListener('readystatechange', onState, true); } });
        obs.observe(document.documentElement, { childList: true, subtree: true, attributes: true, characterData: true });
        document.addEventListener('readystatechange', onState, true);
        const to = setTimeout(() => { obs.disconnect(); document.removeEventListener('readystatechange', onState, true); reject(new Error('wait_for timeout')); }, timeoutMs);
      });
    },
    find(args) {
      const by = (args && args.by) || '';
      const value = (args && args.value) || '';
      const action = (args && args.action) || 'click';
      let target = null;
      if (by === 'role') target = document.querySelector(`[role="${CSS.escape(value)}"]`);
      else if (by === 'testid') target = document.querySelector(`[data-testid="${CSS.escape(value)}"]`);
      else if (by === 'placeholder') target = document.querySelector(`[placeholder="${CSS.escape(value)}"]`);
      else if (by === 'label') {
        const labels = Array.from(document.querySelectorAll('label')).filter((l) => (l.innerText || '').indexOf(value) >= 0);
        const label = labels[0];
        target = label ? (label.htmlFor ? document.getElementById(label.htmlFor) : label.querySelector('input,textarea,select,button')) : null;
      } else if (by === 'text') {
        const all = Array.from(document.querySelectorAll('a,button,[role="button"],[role="link"]'));
        target = all.find((el) => (el.innerText || '').trim() === value)
              || all.find((el) => (el.innerText || '').indexOf(value) >= 0);
      }
      if (!target) throw new Error(`find by ${by}=${value} not matched`);
      if (action === 'click') { target.click(); return { found: true, action: 'click' }; }
      if (action === 'fill') { dispatchSyntheticInput(target, args.fill_value ?? ''); return { found: true, action: 'fill' }; }
      if (action === 'hover') { target.dispatchEvent(new MouseEvent('mouseover', { bubbles: true })); return { found: true, action: 'hover' }; }
      if (action === 'text') { return { found: true, action: 'text', text: (target.innerText || '').slice(0, 4000) }; }
      if (action === 'check') {
        const before = !!target.checked;
        if (!before) {
          try { target.click(); } catch (_) {}
        }
        const after = !!target.checked;
        return { found: true, action: 'check', checked_before: before, checked_after: after };
      }
      throw new Error(`unsupported find action: ${action}`);
    },
    dock_close() { return { closed: true }; },
    screenshot_dom() {
      // Best-effort DOM render fallback.  Without a html-to-canvas
      // shim available in cross-origin pages, this returns the
      // serialised body text + viewport rect so the agent at least
      // knows what was on screen.  The OS-level capture path
      // (xcap on the Rust side) is the primary route.
      return {
        text: (document.body && document.body.innerText ? document.body.innerText.slice(0, 4000) : ''),
        title: document.title || '',
        url: window.location.href,
        viewport: { width: window.innerWidth, height: window.innerHeight },
      };
    },
  };

  window.__senDockBridge = {
    setPick(enabled) {
      pickMode = !!enabled;
      if (pickMode) {
        ensurePickStyle();
        document.addEventListener('mouseover', onHover, true);
        document.addEventListener('click', onClick, true);
      } else {
        clearOutline();
        document.removeEventListener('mouseover', onHover, true);
        document.removeEventListener('click', onClick, true);
      }
    },
    inspect(selector) {
      try {
        const el = resolveSelector(selector);
        if (!el) { send('inspect', { selector, error: 'not found' }); return; }
        send('inspect', { selector, props: computedStyleOf(el) });
      } catch (err) {
        send('inspect', { selector, error: String(err && err.message || err) });
      }
    },
    snapshot,
    zoom(factor) {
      try {
        const f = Number(factor);
        if (!Number.isFinite(f) || f <= 0) return;
        document.body.style.zoom = String(f);
        send('zoom', { factor: f });
      } catch (_) {}
    },
    async clearStorage(opts) {
      const out = { history: !!(opts && opts.history), cookies: false, storage: false, cache: false };
      try { localStorage.clear(); sessionStorage.clear(); out.storage = true; } catch (_) {}
      try {
        document.cookie.split(';').forEach((c) => {
          const eq = c.indexOf('=');
          const name = eq > -1 ? c.slice(0, eq).trim() : c.trim();
          if (!name) return;
          document.cookie = `${name}=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/`;
        });
        out.cookies = true;
      } catch (_) {}
      try {
        if (window.caches && caches.keys) {
          const names = await caches.keys();
          await Promise.all(names.map((n) => caches.delete(n)));
          out.cache = true;
        }
      } catch (_) {}
      send('cleared', out);
    },
    consoleBuffer() { return consoleRing.slice(); },
    /** Agent-driver entry point.  Looks up `kind` in the static
     *  handler map and posts the result back via `senbridge://event`
     *  keyed by `reqId`.  Never throws — failures become
     *  `{ ok: false, error }` envelopes. */
    exec(payload) {
      const reqId = payload && payload.reqId;
      const kind = payload && payload.kind;
      const args = (payload && payload.args) || {};
      let value;
      try {
        const fn = handlers[kind];
        if (!fn) throw new Error(`unknown bridge kind: ${kind}`);
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
  window.addEventListener('pushstate', snapshot);
  setTimeout(snapshot, 50);

  let lastSaneW = 0;
  let lastSaneH = 0;
  function ensureViewportSane() {
    try {
      const docEl = document.documentElement;
      const innerW = window.innerWidth || 0;
      const innerH = window.innerHeight || 0;
      const clientW = (docEl && docEl.clientWidth) || 0;
      const clientH = (docEl && docEl.clientHeight) || 0;
      const minDim = 100;
      const tooSmall = innerW < minDim || innerH < minDim || clientW < minDim || clientH < minDim;
      const mismatch = Math.abs(innerW - clientW) > 2 || Math.abs(innerH - clientH) > 2;
      const grew = innerW > lastSaneW + 2 || innerH > lastSaneH + 2;
      if (tooSmall || mismatch || grew) {
        window.dispatchEvent(new Event('resize'));
        if (docEl) { void docEl.offsetHeight; void docEl.offsetWidth; }
        if (document.body) { void document.body.offsetHeight; void document.body.offsetWidth; }
      }
      lastSaneW = innerW;
      lastSaneH = innerH;
    } catch (_) {}
  }
  window.addEventListener('load', ensureViewportSane);
  window.addEventListener('DOMContentLoaded', ensureViewportSane);
  window.addEventListener('popstate', ensureViewportSane);
  window.addEventListener('hashchange', ensureViewportSane);
  window.addEventListener('pageshow', ensureViewportSane);
  window.addEventListener('focus', ensureViewportSane);
  document.addEventListener('readystatechange', ensureViewportSane);
  [50, 150, 300, 600, 1200, 2400, 4800].forEach((ms) => setTimeout(ensureViewportSane, ms));
})();
"#;

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct DockRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl DockRect {
    fn position_logical(self) -> LogicalPosition<f64> {
        LogicalPosition::new(self.x.max(0.0), self.y.max(0.0))
    }
    fn size_logical(self) -> LogicalSize<f64> {
        LogicalSize::new(self.w.max(1.0), self.h.max(1.0))
    }
}

pub type TabId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabOwner {
    User,
    Agent,
}

impl Default for TabOwner {
    fn default() -> Self {
        Self::User
    }
}

#[derive(Default, Debug, Clone)]
struct TabRecord {
    last_url: Option<String>,
    last_title: Option<String>,
    owner: TabOwner,
}

#[derive(Default)]
struct TabsState {
    last_rect: Option<DockRect>,
    tabs: HashMap<TabId, TabRecord>,
    order: Vec<TabId>,
    active: Option<TabId>,
    next_id: TabId,
    parked: bool,
    dock_visible: bool,
    last_state_url: HashMap<TabId, String>,
    agent_tab_id: Option<TabId>,
}

#[derive(Default, Clone)]
pub struct DockSharedState(Arc<Mutex<TabsState>>);

#[derive(Debug, Clone, serde::Serialize)]
pub struct TabSummary {
    pub id: TabId,
    pub url: Option<String>,
    pub title: Option<String>,
    pub active: bool,
    pub owner: TabOwner,
}

impl DockSharedState {
    pub fn new() -> Self {
        Self::default()
    }

    fn rect(&self) -> Option<DockRect> {
        self.0.lock().last_rect
    }
    fn set_rect(&self, rect: DockRect) {
        self.0.lock().last_rect = Some(rect);
    }

    fn reset(&self) {
        *self.0.lock() = TabsState::default();
    }

    fn parked(&self) -> bool {
        self.0.lock().parked
    }

    fn set_parked(&self, parked: bool) {
        self.0.lock().parked = parked;
    }

    fn dock_visible(&self) -> bool {
        self.0.lock().dock_visible
    }

    fn set_dock_visible(&self, visible: bool) {
        self.0.lock().dock_visible = visible;
    }

    fn record_state_url(&self, id: TabId, url: impl Into<String>) {
        self.0.lock().last_state_url.insert(id, url.into());
    }

    fn last_state_url(&self, id: TabId) -> Option<String> {
        self.0.lock().last_state_url.get(&id).cloned()
    }

    fn forget_state_url(&self, id: TabId) {
        self.0.lock().last_state_url.remove(&id);
    }

    fn alloc_id(&self) -> TabId {
        let mut g = self.0.lock();
        g.next_id = g.next_id.checked_add(1).unwrap_or(1);
        g.next_id
    }

    fn register_tab(&self, id: TabId, url: Option<String>, owner: TabOwner) {
        let mut g = self.0.lock();
        g.tabs.insert(
            id,
            TabRecord {
                last_url: url,
                last_title: None,
                owner,
            },
        );
        if !g.order.contains(&id) {
            g.order.push(id);
        }
        if g.active.is_none() {
            g.active = Some(id);
        }
        if matches!(owner, TabOwner::Agent) && g.agent_tab_id.is_none() {
            g.agent_tab_id = Some(id);
        }
    }

    fn acquire_or_create_user_tab(&self, url: Option<String>) -> (TabId, bool) {
        let mut g = self.0.lock();
        if let Some(active) = g.active {
            if let Some(rec) = g.tabs.get(&active) {
                if matches!(rec.owner, TabOwner::User) {
                    return (active, false);
                }
            }
        }
        let existing_user = g
            .order
            .iter()
            .rev()
            .find(|tid| {
                g.tabs
                    .get(tid)
                    .is_some_and(|rec| matches!(rec.owner, TabOwner::User))
            })
            .copied();
        if let Some(uid) = existing_user {
            g.active = Some(uid);
            return (uid, false);
        }
        g.next_id = g.next_id.checked_add(1).unwrap_or(1);
        let id = g.next_id;
        g.tabs.insert(
            id,
            TabRecord {
                last_url: url,
                last_title: None,
                owner: TabOwner::User,
            },
        );
        if !g.order.contains(&id) {
            g.order.push(id);
        }
        g.active = Some(id);
        (id, true)
    }

    fn acquire_or_create_agent_tab(&self, url: Option<String>) -> (TabId, bool) {
        let mut g = self.0.lock();
        if let Some(existing) = g.agent_tab_id {
            if g.tabs.contains_key(&existing) {
                if let Some(target) = url {
                    if let Some(rec) = g.tabs.get_mut(&existing) {
                        rec.last_url = Some(target);
                    }
                }
                return (existing, false);
            }
            g.agent_tab_id = None;
        }
        g.next_id = g.next_id.checked_add(1).unwrap_or(1);
        let id = g.next_id;
        g.tabs.insert(
            id,
            TabRecord {
                last_url: url,
                last_title: None,
                owner: TabOwner::Agent,
            },
        );
        if !g.order.contains(&id) {
            g.order.push(id);
        }
        g.agent_tab_id = Some(id);
        if g.active.is_none() {
            g.active = Some(id);
        }
        (id, true)
    }

    fn agent_tab_id(&self) -> Option<TabId> {
        self.0.lock().agent_tab_id
    }

    fn tab_owner(&self, id: TabId) -> Option<TabOwner> {
        self.0.lock().tabs.get(&id).map(|r| r.owner)
    }

    fn remove_tab(&self, id: TabId) -> Option<TabId> {
        let mut g = self.0.lock();
        g.tabs.remove(&id);
        g.order.retain(|x| *x != id);
        g.last_state_url.remove(&id);
        if g.active == Some(id) {
            g.active = g.order.last().copied();
        }
        if g.agent_tab_id == Some(id) {
            g.agent_tab_id = None;
        }
        g.active
    }

    fn set_active(&self, id: TabId) -> Result<(), String> {
        let mut g = self.0.lock();
        if !g.tabs.contains_key(&id) {
            return Err(format!("unknown tab id {id}"));
        }
        g.active = Some(id);
        Ok(())
    }

    fn active(&self) -> Option<TabId> {
        self.0.lock().active
    }

    fn list(&self) -> Vec<TabSummary> {
        let g = self.0.lock();
        let active = g.active;
        g.order
            .iter()
            .filter_map(|id| {
                g.tabs.get(id).map(|t| TabSummary {
                    id: *id,
                    url: t.last_url.clone(),
                    title: t.last_title.clone(),
                    active: active == Some(*id),
                    owner: t.owner,
                })
            })
            .collect()
    }

    fn set_url(&self, id: TabId, url: impl Into<String>) {
        let mut g = self.0.lock();
        if let Some(rec) = g.tabs.get_mut(&id) {
            rec.last_url = Some(url.into());
        }
    }
    fn set_title(&self, id: TabId, title: impl Into<String>) {
        let mut g = self.0.lock();
        if let Some(rec) = g.tabs.get_mut(&id) {
            rec.last_title = Some(title.into());
        }
    }

    fn snapshot_tab(&self, id: TabId) -> (Option<String>, Option<String>) {
        let g = self.0.lock();
        match g.tabs.get(&id) {
            Some(rec) => (rec.last_url.clone(), rec.last_title.clone()),
            None => (None, None),
        }
    }

    fn find_owner_tab_with_url(&self, owner: TabOwner, target_url: &str) -> Option<TabId> {
        let normalized = normalize_url_for_match(target_url);
        if normalized.is_empty() {
            return None;
        }
        let g = self.0.lock();
        for id in &g.order {
            let Some(rec) = g.tabs.get(id) else { continue };
            if rec.owner != owner {
                continue;
            }
            let Some(url) = rec.last_url.as_deref() else { continue };
            if normalize_url_for_match(url) == normalized {
                return Some(*id);
            }
        }
        None
    }

    fn snapshot(&self) -> (Option<String>, Option<String>) {
        let active = self.active();
        match active {
            Some(id) => self.snapshot_tab(id),
            None => (None, None),
        }
    }
}

fn parse_target_url(input: Option<String>) -> Result<Url, String> {
    let raw = input
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| ABOUT_BLANK.to_string());
    Url::parse(&raw).map_err(|err| format!("invalid url '{raw}': {err}"))
}

fn dispatch_bridge_event(
    app: &AppHandle,
    state: &DockSharedState,
    kind: &str,
    data_raw: Option<&str>,
) {
    if kind.is_empty() {
        return;
    }

    let parsed_data: Value = data_raw
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(Value::Null);

    if kind == "result" {
        if let Some(controller) = app.try_state::<TauriDockController>() {
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
        if let Some(controller) = app.try_state::<TauriDockController>() {
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
                controller.deliver_chunk(req_id, seq as usize, total as usize, payload);
                return;
            }
        }
    }

    if kind == "openNewTab" {
        if let Some(url) = parsed_data.get("url").and_then(|v| v.as_str()) {
            let trimmed = url.trim();
            if !trimmed.is_empty() {
                let url_owned = trimmed.to_string();
                let app_clone = app.clone();
                if let Err(err) = app.run_on_main_thread(move || {
                    if let Err(err) = open_url_in_new_tab(&app_clone, url_owned) {
                        tracing::warn!(
                            "[browser_dock] openNewTab open_url_in_new_tab failed: {err}"
                        );
                    }
                }) {
                    tracing::warn!(
                        "[browser_dock] openNewTab run_on_main_thread failed: {err}"
                    );
                }
            }
        }
        return;
    }

    let active_tab = state.active();

    if kind == "state" {
        if let Some(active) = active_tab {
            if let Some(url) = parsed_data.get("url").and_then(|v| v.as_str()) {
                let prev = state.snapshot_tab(active).0;
                if prev.as_deref() != Some(url) {
                    if let Some(controller) = app.try_state::<TauriDockController>() {
                        controller.drain_pending_for_tab(
                            active,
                            "dock navigated to a new page",
                        );
                    }
                }
                state.set_url(active, url);
                state.record_state_url(active, url);
            }
            if let Some(title) = parsed_data.get("title").and_then(|v| v.as_str()) {
                state.set_title(active, title);
            }
            emit_tabs_event(app, state);
            if let Some(controller) = app.try_state::<TauriDockController>() {
                controller.signal_nav_ready(active);
            }
        }
    }

    let payload = serde_json::json!({
        "kind": kind,
        "tabId": active_tab,
        "data": parsed_data,
    });

    if let Err(err) = app.emit("browser_dock_event", payload) {
        tracing::warn!("[browser_dock] emit browser_dock_event failed: {err}");
    }
}

pub fn senbridge_protocol_handler(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
    use std::borrow::Cow;

    let app = ctx.app_handle();
    let uri = request.uri();

    let first_seg = uri
        .path()
        .trim_start_matches('/')
        .split('/')
        .next()
        .unwrap_or("")
        .to_string();

    if crate::fetch_worker::handle_protocol_path(app, &first_seg, uri.query()) {
        return tauri::http::Response::builder()
            .status(204)
            .header("Access-Control-Allow-Origin", "*")
            .header("Cache-Control", "no-store")
            .body(Cow::Borrowed(&b""[..]))
            .unwrap_or_else(|_| {
                tauri::http::Response::new(Cow::Borrowed(&b""[..]))
            });
    }

    if first_seg.eq_ignore_ascii_case("event") {
        let mut kind = String::new();
        let mut data_raw: Option<String> = None;
        if let Some(query) = uri.query() {
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
            if let Some(state) = app.try_state::<DockSharedState>() {
                dispatch_bridge_event(app, state.inner(), &kind, data_raw.as_deref());
            }
        }
    }

    tauri::http::Response::builder()
        .status(204)
        .header("Access-Control-Allow-Origin", "*")
        .header("Cache-Control", "no-store")
        .body(Cow::Borrowed(&b""[..]))
        .unwrap_or_else(|_| {
            tauri::http::Response::new(Cow::Borrowed(&b""[..]))
        })
}

fn emit_tabs_event(app: &AppHandle, state: &DockSharedState) {
    let tabs = state.list();
    let active = state.active();
    if let Err(err) = app.emit(
        "browser_dock_event",
        serde_json::json!({
            "kind": "tabs",
            "data": { "tabs": tabs, "active": active },
        }),
    ) {
        tracing::warn!("[browser_dock] emit tabs failed: {err}");
    }
}

fn ensure_main_window(app: &AppHandle) -> Result<Window, String> {
    app.get_window("main")
        .ok_or_else(|| "main window not yet available".to_string())
}

const DOCK_WEBVIEW_LABEL: &str = "browser_dock_main";

const MIN_INITIAL_DIM: f64 = 320.0;
const FALLBACK_INITIAL_W: f64 = 960.0;
const FALLBACK_INITIAL_H: f64 = 720.0;

fn dock_webview(app: &AppHandle) -> Option<tauri::Webview> {
    app.get_webview(DOCK_WEBVIEW_LABEL)
}

fn dock_logical_size(rect: Option<DockRect>) -> LogicalSize<f64> {
    match rect {
        Some(r) => LogicalSize::new(
            r.w.max(MIN_INITIAL_DIM),
            r.h.max(MIN_INITIAL_DIM),
        ),
        None => LogicalSize::new(FALLBACK_INITIAL_W, FALLBACK_INITIAL_H),
    }
}

fn dock_logical_position(rect: Option<DockRect>) -> LogicalPosition<f64> {
    match rect {
        Some(r) => LogicalPosition::new(r.x.max(0.0), r.y.max(0.0)),
        None => LogicalPosition::new(0.0, 0.0),
    }
}

fn ensure_dock_webview(
    app: &AppHandle,
    state: &DockSharedState,
) -> Result<tauri::Webview, String> {
    if let Some(wv) = dock_webview(app) {
        return Ok(wv);
    }

    let main = ensure_main_window(app)?;

    let initial_url = state
        .active()
        .and_then(|id| state.snapshot_tab(id).0)
        .filter(|u| !u.trim().is_empty());
    let parsed = parse_target_url(initial_url)?;

    let app_for_new_window = app.clone();
    let bridge_js = build_bridge_js();
    let app_for_nav = app.clone();

    let builder =
        WebviewBuilder::new(DOCK_WEBVIEW_LABEL, WebviewUrl::External(parsed))
            .initialization_script(bridge_js)
            .accept_first_mouse(true)
            .on_navigation(move |target: &Url| {
                if target.scheme() == BRIDGE_SCHEME {
                    return false;
                }
                if target.host_str() == Some(BRIDGE_HOST) {
                    return false;
                }
                if let Some(state) = app_for_nav.try_state::<DockSharedState>() {
                    if let Some(active) = state.active() {
                        state.set_url(active, target.as_str());
                    }
                }
                true
            })
            .on_new_window(move |url, _features| {
                let scheme = url.scheme().to_ascii_lowercase();
                let acceptable = matches!(
                    scheme.as_str(),
                    "http" | "https" | "ftp" | "file" | "data"
                );
                if !acceptable {
                    return tauri::webview::NewWindowResponse::Deny;
                }
                if url.host_str() == Some(BRIDGE_HOST) || scheme == BRIDGE_SCHEME
                {
                    return tauri::webview::NewWindowResponse::Deny;
                }
                let url_string = url.to_string();
                let app_for_task = app_for_new_window.clone();
                if let Err(err) =
                    app_for_new_window.run_on_main_thread(move || {
                        if let Err(err) =
                            open_url_in_new_tab(&app_for_task, url_string)
                        {
                            tracing::warn!(
                                "[browser_dock] on_new_window open_url_in_new_tab failed: {err}"
                            );
                        }
                    })
                {
                    tracing::warn!(
                        "[browser_dock] on_new_window run_on_main_thread failed: {err}"
                    );
                }
                tauri::webview::NewWindowResponse::Deny
            });

    let raw_rect = state.rect();
    let initial_size = dock_logical_size(raw_rect);
    let initial_position = dock_logical_position(raw_rect);

    main.add_child(builder, initial_position, initial_size)
        .map_err(|e| format!("add_child({DOCK_WEBVIEW_LABEL}) failed: {e}"))?;

    dock_webview(app)
        .ok_or_else(|| "dock webview missing immediately after add_child".to_string())
}

fn clamp_rect_to_window(rect: DockRect, win: &Window) -> DockRect {
    let physical = win.inner_size().ok();
    let scale = win.scale_factor().unwrap_or(1.0).max(0.0001);
    let (max_w, max_h) = match physical {
        Some(size) => (
            (size.width as f64 / scale).max(1.0),
            (size.height as f64 / scale).max(1.0),
        ),
        None => (10_000.0, 10_000.0),
    };
    let x = rect.x.clamp(0.0, (max_w - 1.0).max(0.0));
    let y = rect.y.clamp(0.0, (max_h - 1.0).max(0.0));
    let max_w_avail = (max_w - x).max(1.0);
    let max_h_avail = (max_h - y).max(1.0);
    let w = rect.w.clamp(1.0, max_w_avail);
    let h = rect.h.clamp(1.0, max_h_avail);
    DockRect { x, y, w, h }
}

fn update_dock_layout(app: &AppHandle, state: &DockSharedState) -> Result<(), String> {
    let raw_rect = state.rect();
    let main = ensure_main_window(app).ok();
    let rect = match (raw_rect, main.as_ref()) {
        (Some(r), Some(win)) => Some(clamp_rect_to_window(r, win)),
        (Some(r), None) => Some(r),
        (None, _) => None,
    };

    let parked = state.parked();
    let has_active = state.active().is_some();
    let want_visible = has_active && rect.is_some() && !parked;

    let Some(wv) = dock_webview(app) else {
        return Ok(());
    };

    if let Some(rect) = rect {
        let pos = rect.position_logical();
        let size = rect.size_logical();
        wv.set_position(pos)
            .map_err(|e| format!("set_position(dock) failed: {e}"))?;
        wv.set_size(size)
            .map_err(|e| format!("set_size(dock) failed: {e}"))?;
    }

    let was_visible = state.dock_visible();
    if want_visible != was_visible {
        if want_visible {
            wv.show().map_err(|e| format!("show(dock) failed: {e}"))?;
        } else {
            wv.hide().map_err(|e| format!("hide(dock) failed: {e}"))?;
        }
        state.set_dock_visible(want_visible);
    }
    Ok(())
}

fn dock_navigate_active(app: &AppHandle, state: &DockSharedState) -> Result<(), String> {
    let Some(active) = state.active() else {
        return Ok(());
    };
    let Some(target) = state.snapshot_tab(active).0 else {
        return Ok(());
    };
    if target.trim().is_empty() {
        return Ok(());
    }
    let Some(wv) = dock_webview(app) else {
        return Ok(());
    };
    let parsed = parse_target_url(Some(target.clone()))?;
    if let Ok(current) = wv.url() {
        let cur_str = current.as_str();
        let tgt_str = parsed.as_str();
        if cur_str == tgt_str || urls_logically_match(cur_str, tgt_str) {
            state.record_state_url(active, cur_str);
            if let Some(controller) = app.try_state::<TauriDockController>() {
                controller.signal_nav_ready(active);
            }
            return Ok(());
        }
    }
    state.forget_state_url(active);
    wv.navigate(parsed)
        .map_err(|e| format!("navigate failed: {e}"))?;
    Ok(())
}

fn focus_dock_webview(app: &AppHandle) {
    let Some(webview) = dock_webview(app) else {
        return;
    };
    #[cfg(windows)]
    {
        let _ = webview.with_webview(focus_webview2_native);
    }
    let _ = webview.set_focus();
}

#[cfg(windows)]
fn focus_webview2_native(platform: tauri::webview::PlatformWebview) {
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows::Win32::UI::WindowsAndMessaging::{
        HWND_TOP, SWP_ASYNCWINDOWPOS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };

    let controller = platform.controller();
    let mut parent_hwnd: HWND = HWND(std::ptr::null_mut());
    let parent_ok = unsafe { controller.ParentWindow(&mut parent_hwnd) }.is_ok();
    if parent_ok && !parent_hwnd.0.is_null() {
        unsafe {
            let _ = SetWindowPos(
                parent_hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            );
            let _ = SetFocus(Some(parent_hwnd));
        }
    }
    let _ = unsafe { controller.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC) };
}

#[tauri::command]
pub async fn browser_dock_open(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    rect: DockRect,
    url: Option<String>,
) -> Result<(), String> {
    state.set_rect(rect);
    state.set_parked(false);
    let s = state.inner().clone();

    let target = url
        .as_ref()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());

    if let Some(target_url) = target.as_deref() {
        if let Some(existing) = s.find_owner_tab_with_url(TabOwner::User, target_url) {
            let _ = s.set_active(existing);
            ensure_dock_webview(&app, &s)?;
            dock_navigate_active(&app, &s)?;
            update_dock_layout(&app, &s)?;
            focus_dock_webview(&app);
            emit_tabs_event(&app, &s);
            return Ok(());
        }
    }

    let (active, _created) = s.acquire_or_create_user_tab(target.clone());
    if let Some(target_url) = target.as_ref() {
        s.set_url(active, target_url.clone());
    }

    ensure_dock_webview(&app, &s)?;
    dock_navigate_active(&app, &s)?;
    update_dock_layout(&app, &s)?;
    focus_dock_webview(&app);
    emit_tabs_event(&app, &s);
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_set_rect(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    rect: DockRect,
) -> Result<(), String> {
    state.set_rect(rect);
    state.set_parked(false);
    update_dock_layout(&app, state.inner())?;
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_resync(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    rect: DockRect,
) -> Result<(), String> {
    state.set_rect(rect);
    state.set_parked(false);
    update_dock_layout(&app, state.inner())?;
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_hide(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    state.set_parked(true);
    update_dock_layout(&app, state.inner())?;
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_focus_active(
    app: AppHandle,
    _state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    focus_dock_webview(&app);
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_park(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    state.set_parked(true);
    update_dock_layout(&app, state.inner())?;
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_close(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    if let Some(webview) = dock_webview(&app) {
        let _ = webview.close();
    }
    state.reset();
    if let Some(controller) = app.try_state::<TauriDockController>() {
        controller.drain_pending("dock closed");
    }
    emit_tabs_event(&app, state.inner());
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_navigate(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    url: String,
) -> Result<(), String> {
    let id = state
        .active()
        .ok_or_else(|| "no active dock tab".to_string())?;
    let trimmed = url.trim();
    if !trimmed.is_empty() {
        if let Some(existing) = state.find_owner_tab_with_url(TabOwner::User, trimmed) {
            if existing != id {
                state.set_active(existing)?;
            }
            ensure_dock_webview(&app, state.inner())?;
            dock_navigate_active(&app, state.inner())?;
            focus_dock_webview(&app);
            emit_tabs_event(&app, state.inner());
            return Ok(());
        }
    }
    state.set_url(id, url);
    ensure_dock_webview(&app, state.inner())?;
    dock_navigate_active(&app, state.inner())?;
    focus_dock_webview(&app);
    emit_tabs_event(&app, state.inner());
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_new_tab(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    url: Option<String>,
    activate: Option<bool>,
) -> Result<TabId, String> {
    let id = state.alloc_id();
    state.register_tab(id, url.clone(), TabOwner::User);
    let want_activate = activate.unwrap_or(true);
    if want_activate {
        let _ = state.set_active(id);
    }
    if want_activate {
        ensure_dock_webview(&app, state.inner())?;
        dock_navigate_active(&app, state.inner())?;
        update_dock_layout(&app, state.inner())?;
        focus_dock_webview(&app);
    }
    emit_tabs_event(&app, state.inner());
    Ok(id)
}

fn open_url_in_new_tab(app: &AppHandle, url: String) -> Result<TabId, String> {
    let state_handle = app
        .try_state::<DockSharedState>()
        .ok_or_else(|| "browser dock state not initialised".to_string())?;
    let state = state_handle.inner();
    let trimmed = url.trim();
    if !trimmed.is_empty() {
        if let Some(existing) = state.find_owner_tab_with_url(TabOwner::User, trimmed) {
            let _ = state.set_active(existing);
            ensure_dock_webview(app, state)?;
            dock_navigate_active(app, state)?;
            let _ = update_dock_layout(app, state);
            focus_dock_webview(app);
            emit_tabs_event(app, state);
            return Ok(existing);
        }
    }
    let id = state.alloc_id();
    state.register_tab(id, Some(url), TabOwner::User);
    let _ = state.set_active(id);
    ensure_dock_webview(app, state)?;
    dock_navigate_active(app, state)?;
    let _ = update_dock_layout(app, state);
    focus_dock_webview(app);
    emit_tabs_event(app, state);
    Ok(id)
}

#[tauri::command]
pub async fn browser_dock_close_tab(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    tab_id: TabId,
) -> Result<Option<TabId>, String> {
    if let Some(controller) = app.try_state::<TauriDockController>() {
        controller.drain_pending_for_tab(tab_id, "tab closed");
    }
    let was_active = state.active() == Some(tab_id);
    let new_active = state.remove_tab(tab_id);
    if was_active && new_active.is_some() {
        dock_navigate_active(&app, state.inner())?;
    } else if new_active.is_none() {
        if let Some(wv) = dock_webview(&app) {
            let _ = wv.hide();
        }
        state.set_dock_visible(false);
    }
    update_dock_layout(&app, state.inner())?;
    emit_tabs_event(&app, state.inner());
    Ok(new_active)
}

#[tauri::command]
pub async fn browser_dock_activate_tab(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    tab_id: TabId,
) -> Result<(), String> {
    state.set_active(tab_id)?;
    ensure_dock_webview(&app, state.inner())?;
    dock_navigate_active(&app, state.inner())?;
    update_dock_layout(&app, state.inner())?;
    focus_dock_webview(&app);
    emit_tabs_event(&app, state.inner());
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_list_tabs(
    state: tauri::State<'_, DockSharedState>,
) -> Result<Vec<TabSummary>, String> {
    Ok(state.list())
}

#[tauri::command]
pub async fn browser_dock_pin_test_target(tab_id: TabId) -> Result<(), String> {
    set_test_target_tab(tab_id);
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_clear_test_target() -> Result<(), String> {
    clear_test_target_tab();
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_get_test_target() -> Result<Option<TabId>, String> {
    Ok(current_test_target_tab())
}

fn eval_dock(app: &AppHandle, _state: &DockSharedState, source: &str) -> Result<(), String> {
    let webview = dock_webview(app)
        .ok_or_else(|| "dock webview is not open".to_string())?;
    webview
        .eval(source)
        .map_err(|e| format!("eval failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_back(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    eval_dock(&app, state.inner(), "history.back();")
}

#[tauri::command]
pub async fn browser_dock_forward(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    eval_dock(&app, state.inner(), "history.forward();")
}

#[tauri::command]
pub async fn browser_dock_reload(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    hard: Option<bool>,
) -> Result<(), String> {
    if hard.unwrap_or(false) {
        eval_dock(
            &app,
            state.inner(),
            "(async () => { try { await window.__senDockBridge?.clearStorage({ history: false }); } catch(_){}; location.reload(); })();",
        )
    } else {
        eval_dock(&app, state.inner(), "location.reload();")
    }
}

#[tauri::command]
pub async fn browser_dock_set_zoom(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    factor: f64,
) -> Result<(), String> {
    let safe = factor.clamp(0.25, 3.0);
    eval_dock(
        &app,
        state.inner(),
        &format!(
            "window.__senDockBridge && window.__senDockBridge.zoom({});",
            safe
        ),
    )
}

#[tauri::command]
pub async fn browser_dock_set_pick_mode(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    enabled: bool,
) -> Result<(), String> {
    eval_dock(
        &app,
        state.inner(),
        &format!(
            "window.__senDockBridge && window.__senDockBridge.setPick({});",
            if enabled { "true" } else { "false" }
        ),
    )
}

#[tauri::command]
pub async fn browser_dock_inspect_selector(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    selector: String,
) -> Result<(), String> {
    let escaped =
        serde_json::to_string(&selector).map_err(|e| format!("escape selector: {e}"))?;
    eval_dock(
        &app,
        state.inner(),
        &format!(
            "window.__senDockBridge && window.__senDockBridge.inspect({});",
            escaped
        ),
    )
}

#[tauri::command]
pub async fn browser_dock_clear(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    cookies: Option<bool>,
    cache: Option<bool>,
    history: Option<bool>,
) -> Result<(), String> {
    let opts = serde_json::json!({
        "cookies": cookies.unwrap_or(true),
        "cache": cache.unwrap_or(true),
        "history": history.unwrap_or(false),
    });
    let opts_js = serde_json::to_string(&opts).map_err(|e| format!("opts: {e}"))?;
    eval_dock(
        &app,
        state.inner(),
        &format!(
            "window.__senDockBridge && window.__senDockBridge.clearStorage({});",
            opts_js
        ),
    )
}

#[tauri::command]
pub async fn browser_dock_request_state(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    eval_dock(
        &app,
        state.inner(),
        "window.__senDockBridge && window.__senDockBridge.snapshot();",
    )
}

#[tauri::command]
pub async fn browser_dock_screenshot(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    full_page: Option<bool>,
) -> Result<serde_json::Value, String> {
    let _ = state;
    let full = full_page.unwrap_or(false);
    if full {
        if let Some(webview) = dock_webview(&app) {
            let warmup = r#"(async () => {
                  try {
                    const total = Math.max(
                      document.documentElement.scrollHeight,
                      document.body && document.body.scrollHeight || 0,
                    );
                    const step = Math.max(window.innerHeight * 0.9, 400);
                    for (let y = 0; y <= total; y += step) {
                      window.scrollTo({ top: y, behavior: 'instant' in window ? 'instant' : 'auto' });
                      await new Promise((r) => setTimeout(r, 30));
                    }
                    window.scrollTo({ top: 0, behavior: 'instant' in window ? 'instant' : 'auto' });
                  } catch (_) {}
                })();"#;
            let _ = webview.eval(warmup);
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        }
    }
    let app_xcap = app.clone();
    let bytes = tokio::task::spawn_blocking(move || capture_dock_window(&app_xcap))
        .await
        .map_err(|e| format!("screenshot join: {e}"))?
        .map_err(|e| format!("screenshot capture: {e}"))?;
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(serde_json::json!({
        "png_base64": encoded,
        "bytes": bytes.len(),
        "full_page": full,
    }))
}

#[tauri::command]
pub async fn browser_dock_get_state(
    state: tauri::State<'_, DockSharedState>,
) -> Result<serde_json::Value, String> {
    let (url, title) = state.snapshot();
    Ok(serde_json::json!({
        "url": url,
        "title": title,
    }))
}

#[tauri::command]
pub async fn browser_dock_open_devtools(
    app: AppHandle,
    _state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    let webview = dock_webview(&app)
        .ok_or_else(|| "dock webview is not open".to_string())?;
    #[cfg(debug_assertions)]
    {
        webview.open_devtools();
        return Ok(());
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = webview;
        Err("DevTools are only available in debug builds".to_string())
    }
}

#[tauri::command]
pub async fn browser_dock_close_devtools(
    app: AppHandle,
    _state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    let webview = dock_webview(&app)
        .ok_or_else(|| "dock webview is not open".to_string())?;
    #[cfg(debug_assertions)]
    {
        webview.close_devtools();
        return Ok(());
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = webview;
        Err("DevTools are only available in debug builds".to_string())
    }
}

#[derive(Clone)]
pub struct TauriDockController(Arc<TauriDockControllerInner>);

struct TauriDockControllerInner {
    app: AppHandle,

    pending: Mutex<HashMap<u64, oneshot::Sender<DockResponse>>>,

    pending_tab: Mutex<HashMap<u64, TabId>>,

    chunks: Mutex<HashMap<u64, ChunkBuffer>>,

    drive_locks: Mutex<HashMap<TabId, Arc<AsyncMutex<()>>>>,

    nav_waiters: Mutex<HashMap<TabId, Vec<oneshot::Sender<()>>>>,
    next_req_id: AtomicU64,
    takeover_state: Mutex<HashMap<TabId, TakeoverEntry>>,
}

struct TakeoverEntry {
    started_at: u64,
    abort: tokio::task::AbortHandle,
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
        let mut buf = String::with_capacity(self.parts.iter().map(|p| p.as_ref().map(|s| s.len()).unwrap_or(0)).sum());
        for part in self.parts.into_iter().flatten() {
            buf.push_str(&part);
        }
        buf
    }
}

impl TauriDockController {
    pub fn new(app: AppHandle) -> Self {
        Self(Arc::new(TauriDockControllerInner {
            app,
            pending: Mutex::new(HashMap::new()),
            pending_tab: Mutex::new(HashMap::new()),
            chunks: Mutex::new(HashMap::new()),
            drive_locks: Mutex::new(HashMap::new()),
            nav_waiters: Mutex::new(HashMap::new()),
            next_req_id: AtomicU64::new(1),
            takeover_state: Mutex::new(HashMap::new()),
        }))
    }

    fn note_takeover_activity(&self, tab_id: TabId, owner: Option<TabOwner>) {
        if !matches!(owner, Some(TabOwner::User)) {
            return;
        }
        let now = now_millis() as u64;
        let inner = self.0.clone();
        let mut started_at = now;
        let mut emit_start = false;
        {
            let mut guard = inner.takeover_state.lock();
            if let Some(prev) = guard.remove(&tab_id) {
                started_at = prev.started_at;
                prev.abort.abort();
            } else {
                emit_start = true;
            }
            let inner_for_task = inner.clone();
            let handle = tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(3_000)).await;
                let removed = {
                    let mut g = inner_for_task.takeover_state.lock();
                    g.remove(&tab_id)
                };
                if removed.is_some() {
                    let payload = serde_json::json!({
                        "kind": "dock_takeover_end",
                        "tabId": tab_id,
                        "data": { "tab_id": tab_id, "ended_at": now_millis() },
                    });
                    let _ = inner_for_task.app.emit("browser_dock_event", payload);
                }
            });
            guard.insert(
                tab_id,
                TakeoverEntry {
                    started_at,
                    abort: handle.abort_handle(),
                },
            );
        }
        if emit_start {
            let payload = serde_json::json!({
                "kind": "dock_takeover",
                "tabId": tab_id,
                "data": { "tab_id": tab_id, "started_at": started_at },
            });
            let _ = self.0.app.emit("browser_dock_event", payload);
        }
    }

    fn register_nav_waiter(&self, tab_id: TabId) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        self.0
            .nav_waiters
            .lock()
            .entry(tab_id)
            .or_default()
            .push(tx);
        rx
    }

    pub fn signal_nav_ready(&self, tab_id: TabId) {
        let waiters: Vec<oneshot::Sender<()>> = {
            let mut g = self.0.nav_waiters.lock();
            g.remove(&tab_id).unwrap_or_default()
        };
        for tx in waiters {
            let _ = tx.send(());
        }
    }

    async fn await_dock_ready(
        &self,
        tab_id: TabId,
        timeout: Duration,
    ) -> Result<()> {
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;

        if dock_webview(&self.0.app).is_none() {
            return Err(anyhow::anyhow!("dock webview is not open"));
        }

        let expected = state
            .snapshot_tab(tab_id)
            .0
            .filter(|u| !u.trim().is_empty());

        if expected.is_none() {
            return Ok(());
        }

        let expected_url = expected.unwrap();
        let last_seen = state.last_state_url(tab_id);
        if let Some(seen) = &last_seen {
            if urls_logically_match(seen, &expected_url) {
                return Ok(());
            }
        }

        let rx = self.register_nav_waiter(tab_id);

        let last_seen = state.last_state_url(tab_id);
        if let Some(seen) = &last_seen {
            if urls_logically_match(seen, &expected_url) {
                self.signal_nav_ready(tab_id);
                return Ok(());
            }
        }

        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(_)) => {
                Err(anyhow::anyhow!("nav ready channel closed"))
            }
            Err(_) => Err(anyhow::anyhow!(
                "timed out waiting for dock navigation to complete on tab {tab_id}"
            )),
        }
    }

    fn tab_lock(&self, tab_id: TabId) -> Arc<AsyncMutex<()>> {
        let mut g = self.0.drive_locks.lock();
        g.entry(tab_id)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn forget_request(&self, req_id: u64) {
        self.0.pending.lock().remove(&req_id);
        self.0.pending_tab.lock().remove(&req_id);
        self.0.chunks.lock().remove(&req_id);
    }

    pub fn deliver_result(&self, req_id: u64, ok: bool, value: Value, error: Option<String>) {
        self.0.chunks.lock().remove(&req_id);
        self.0.pending_tab.lock().remove(&req_id);
        let sender = self.0.pending.lock().remove(&req_id);
        if let Some(tx) = sender {
            let _ = tx.send(DockResponse { ok, value, error });
        }
    }

    pub fn deliver_chunk(&self, req_id: u64, seq: usize, total: usize, payload: String) {
        let assembled = {
            let mut guard = self.0.chunks.lock();
            let buf = guard
                .entry(req_id)
                .or_insert_with(|| ChunkBuffer::new(total));

            if buf.parts.len() != total {
                *buf = ChunkBuffer::new(total);
            }
            if buf.ingest(seq, payload) {
                Some(guard.remove(&req_id).unwrap().assemble())
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
                    self.0.pending_tab.lock().remove(&req_id);
                    let sender = self.0.pending.lock().remove(&req_id);
                    if let Some(tx) = sender {
                        let _ = tx.send(DockResponse { ok, value, error });
                    }
                }
                Err(err) => {
                    self.0.pending_tab.lock().remove(&req_id);
                    let sender = self.0.pending.lock().remove(&req_id);
                    if let Some(tx) = sender {
                        let _ = tx.send(DockResponse {
                            ok: false,
                            value: Value::Null,
                            error: Some(format!("chunk reassembly parse error: {err}")),
                        });
                    }
                }
            }
        }
    }

    pub fn drain_pending(&self, reason: &str) {
        self.0.chunks.lock().clear();
        self.0.pending_tab.lock().clear();
        let drained: Vec<oneshot::Sender<DockResponse>> = {
            let mut guard = self.0.pending.lock();
            std::mem::take(&mut *guard).into_values().collect()
        };
        for tx in drained {
            let _ = tx.send(DockResponse {
                ok: false,
                value: Value::Null,
                error: Some(reason.to_string()),
            });
        }
    }

    pub fn drain_pending_for_tab(&self, tab_id: TabId, reason: &str) {
        let req_ids: Vec<u64> = {
            let mut guard = self.0.pending_tab.lock();
            let ids: Vec<u64> = guard
                .iter()
                .filter_map(|(rid, tid)| if *tid == tab_id { Some(*rid) } else { None })
                .collect();
            for rid in &ids {
                guard.remove(rid);
            }
            ids
        };
        for rid in req_ids {
            self.0.chunks.lock().remove(&rid);
            if let Some(tx) = self.0.pending.lock().remove(&rid) {
                let _ = tx.send(DockResponse {
                    ok: false,
                    value: Value::Null,
                    error: Some(reason.to_string()),
                });
            }
        }
    }
}

fn normalize_url_for_match(s: &str) -> String {
    let trimmed = s.trim();
    let no_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let trimmed_tail = no_fragment.trim_end_matches('/');
    if trimmed_tail.is_empty() {
        no_fragment.to_string()
    } else {
        trimmed_tail.to_string()
    }
}

fn urls_logically_match(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    normalize_url_for_match(a) == normalize_url_for_match(b)
}

fn resolve_agent_target_tab(req: &DockRequest, state: &DockSharedState) -> TabId {
    if let Some(id) = req
        .args
        .as_object()
        .and_then(|m| m.get("tab_id"))
        .and_then(|v| v.as_u64())
    {
        return id as TabId;
    }
    if let Some(agent_id) = state.agent_tab_id() {
        return agent_id;
    }
    let (id, _) = state.acquire_or_create_agent_tab(None);
    id
}

#[async_trait]
impl DockController for TauriDockController {
    async fn ensure_visible(&self, session_hint: Option<String>) -> Result<()> {
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .unwrap_or_default();
        let (agent_tab, created) = state.acquire_or_create_agent_tab(None);
        if created {
            if state.rect().is_none() {
                state.set_rect(DockRect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
            }
            state.set_parked(false);
            let _ = state.set_active(agent_tab);
            if let Err(err) = ensure_dock_webview(&self.0.app, &state) {
                tracing::warn!(
                    "[browser_dock] auto-create on ensure_visible failed: {err}"
                );
            } else {
                let _ = dock_navigate_active(&self.0.app, &state);
                let _ = update_dock_layout(&self.0.app, &state);
                emit_tabs_event(&self.0.app, &state);
            }
        }

        let payload = serde_json::json!({
            "kind": "visible",
            "data": { "session": session_hint, "source": "agent", "agentTabId": agent_tab },
        });
        if let Err(err) = self.0.app.emit("browser_dock_event", payload) {
            tracing::warn!("[browser_dock] emit visible failed: {err}");
        }
        Ok(())
    }

    async fn exec(&self, req: DockRequest) -> Result<DockResponse> {
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        let tab_id = resolve_agent_target_tab(&req, &state);
        let tab_owner = state.tab_owner(tab_id);
        if tab_owner.is_none() {
            return Err(anyhow::anyhow!(
                "dock tab {tab_id} does not exist; call ensure_visible or browser.open_tab first"
            ));
        }
        self.note_takeover_activity(tab_id, tab_owner);

        let active = state.active();
        if active != Some(tab_id) {
            state
                .set_active(tab_id)
                .map_err(|e| anyhow::anyhow!("set_active: {e}"))?;
            ensure_dock_webview(&self.0.app, &state)
                .map_err(|e| anyhow::anyhow!("ensure_dock_webview: {e}"))?;
            let _ = dock_navigate_active(&self.0.app, &state);
            let _ = update_dock_layout(&self.0.app, &state);
            emit_tabs_event(&self.0.app, &state);
        } else {
            ensure_dock_webview(&self.0.app, &state)
                .map_err(|e| anyhow::anyhow!("ensure_dock_webview: {e}"))?;
        }

        let lock = self.tab_lock(tab_id);
        let _drive = lock.lock().await;

        const READY_TIMEOUT: Duration = Duration::from_millis(15_000);

        if req.kind == "navigate" {
            return self
                .exec_navigate_via_rust(tab_id, &req, &state, READY_TIMEOUT)
                .await;
        }

        if let Err(err) = self.await_dock_ready(tab_id, READY_TIMEOUT).await {
            tracing::warn!(
                "[browser_dock] await_dock_ready before exec failed: {err}"
            );
        }

        let preview_id = self.0.next_req_id.load(Ordering::SeqCst);
        let _ = self.0.app.emit(
            "browser_dock_event",
            serde_json::json!({
                "kind": "agent_action",
                "tabId": tab_id,
                "data": {
                    "reqId": preview_id,
                    "kind": req.kind,
                    "args": req.args,
                    "tabId": tab_id,
                    "ts": now_millis(),
                },
            }),
        );

        self.exec_on_tab(tab_id, req).await
    }

    async fn screenshot(&self, full_page: bool) -> Result<Vec<u8>> {
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        let tab_id = state
            .agent_tab_id()
            .or_else(|| state.active())
            .ok_or_else(|| anyhow::anyhow!("no active dock tab to capture"))?;
        if state.active() != Some(tab_id) {
            let _ = state.set_active(tab_id);
            let _ = dock_navigate_active(&self.0.app, &state);
            let _ = update_dock_layout(&self.0.app, &state);
            emit_tabs_event(&self.0.app, &state);
        }

        let lock = self.tab_lock(tab_id);
        let _drive = lock.lock().await;

        if let Err(err) = self
            .await_dock_ready(tab_id, Duration::from_millis(15_000))
            .await
        {
            tracing::warn!(
                "[browser_dock] await_dock_ready before screenshot failed: {err}"
            );
        }

        if full_page {
            if let Some(webview) = dock_webview(&self.0.app) {
                let warmup = r#"(async () => {
                  try {
                    const total = Math.max(
                      document.documentElement.scrollHeight,
                      document.body && document.body.scrollHeight || 0,
                    );
                    const step = Math.max(window.innerHeight * 0.9, 400);
                    for (let y = 0; y <= total; y += step) {
                      window.scrollTo({ top: y, behavior: 'instant' in window ? 'instant' : 'auto' });
                      await new Promise((r) => setTimeout(r, 30));
                    }
                    window.scrollTo({ top: 0, behavior: 'instant' in window ? 'instant' : 'auto' });
                  } catch (_) {}
                })();"#;
                let _ = webview.eval(warmup);
                tokio::time::sleep(Duration::from_millis(120)).await;
            }
        }

        let app_xcap = self.0.app.clone();
        let xcap_result =
            tokio::task::spawn_blocking(move || capture_dock_window(&app_xcap)).await;
        match xcap_result {
            Ok(Ok(bytes)) => return Ok(bytes),
            Ok(Err(err)) => {
                tracing::warn!(
                    "[browser_dock] xcap capture failed, falling back to DOM metadata: {err}"
                );
            }
            Err(err) => {
                tracing::warn!(
                    "[browser_dock] xcap join failed, falling back to DOM metadata: {err}"
                );
            }
        }

        let req = DockRequest {
            kind: "screenshot_dom".to_string(),
            args: Value::Null,
            timeout_ms: 5_000,
        };
        let resp = self.exec_on_tab(tab_id, req).await?;
        if !resp.ok {
            return Err(anyhow::anyhow!(
                "screenshot fallback failed: {}",
                resp.error.unwrap_or_else(|| "unknown".into())
            ));
        }
        Ok(render_dom_fallback_png(&resp.value))
    }

    async fn new_tab(&self, url: Option<String>, activate: bool) -> Result<u32> {
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        let id = state.alloc_id();
        state.register_tab(id, url, TabOwner::Agent);
        if activate {
            let _ = state.set_active(id);
            ensure_dock_webview(&self.0.app, &state)
                .map_err(|e| anyhow::anyhow!("ensure_dock_webview: {e}"))?;
            let _ = dock_navigate_active(&self.0.app, &state);
            let _ = update_dock_layout(&self.0.app, &state);
        }
        emit_tabs_event(&self.0.app, &state);
        Ok(id)
    }

    async fn close_tab(&self, tab_id: u32) -> Result<Option<u32>> {
        self.drain_pending_for_tab(tab_id, "tab closed");
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        let was_active = state.active() == Some(tab_id);
        let new_active = state.remove_tab(tab_id);
        if was_active && new_active.is_some() {
            let _ = dock_navigate_active(&self.0.app, &state);
        } else if new_active.is_none() {
            if let Some(wv) = dock_webview(&self.0.app) {
                let _ = wv.hide();
            }
            state.set_dock_visible(false);
        }
        update_dock_layout(&self.0.app, &state)
            .map_err(|e| anyhow::anyhow!("update_dock_layout: {e}"))?;
        emit_tabs_event(&self.0.app, &state);
        Ok(new_active)
    }

    async fn activate_tab(&self, tab_id: u32) -> Result<()> {
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        let owner = state.tab_owner(tab_id);
        self.note_takeover_activity(tab_id, owner);
        state
            .set_active(tab_id)
            .map_err(|e| anyhow::anyhow!("set_active: {e}"))?;
        ensure_dock_webview(&self.0.app, &state)
            .map_err(|e| anyhow::anyhow!("ensure_dock_webview: {e}"))?;
        let _ = dock_navigate_active(&self.0.app, &state);
        update_dock_layout(&self.0.app, &state)
            .map_err(|e| anyhow::anyhow!("update_dock_layout: {e}"))?;
        emit_tabs_event(&self.0.app, &state);
        Ok(())
    }

    async fn list_tabs(&self) -> Result<Vec<DockTabInfo>> {
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        Ok(state
            .list()
            .into_iter()
            .map(|t| DockTabInfo {
                id: t.id,
                url: t.url,
                title: t.title,
                active: t.active,
                owner: Some(match t.owner {
                    TabOwner::User => "user".to_string(),
                    TabOwner::Agent => "agent".to_string(),
                }),
            })
            .collect())
    }
}

impl TauriDockController {
    async fn exec_navigate_via_rust(
        &self,
        tab_id: TabId,
        req: &DockRequest,
        state: &DockSharedState,
        ready_timeout: Duration,
    ) -> Result<DockResponse> {
        let url_raw = req
            .args
            .get("url")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("navigate requires a non-empty 'url' argument"))?;

        let parsed = Url::parse(&url_raw)
            .map_err(|err| anyhow::anyhow!("invalid url '{url_raw}': {err}"))?;
        let normalized = parsed.as_str().to_string();

        let owner = state.tab_owner(tab_id).unwrap_or(TabOwner::Agent);
        if let Some(existing) = state.find_owner_tab_with_url(owner, &normalized) {
            if existing != tab_id {
                state
                    .set_active(existing)
                    .map_err(|e| anyhow::anyhow!("set_active: {e}"))?;
                ensure_dock_webview(&self.0.app, state)
                    .map_err(|e| anyhow::anyhow!("ensure_dock_webview: {e}"))?;
                update_dock_layout(&self.0.app, state)
                    .map_err(|e| anyhow::anyhow!("update_dock_layout: {e}"))?;
                emit_tabs_event(&self.0.app, state);
                self.signal_nav_ready(existing);
            } else {
                self.signal_nav_ready(tab_id);
            }
            return Ok(DockResponse {
                ok: true,
                value: serde_json::json!({
                    "navigated": false,
                    "reused": true,
                    "url": normalized,
                    "tab_id": existing,
                }),
                error: None,
            });
        }

        state.set_url(tab_id, normalized.clone());
        state.forget_state_url(tab_id);

        if state.active() != Some(tab_id) {
            state
                .set_active(tab_id)
                .map_err(|e| anyhow::anyhow!("set_active: {e}"))?;
        }
        ensure_dock_webview(&self.0.app, state)
            .map_err(|e| anyhow::anyhow!("ensure_dock_webview: {e}"))?;
        update_dock_layout(&self.0.app, state)
            .map_err(|e| anyhow::anyhow!("update_dock_layout: {e}"))?;
        emit_tabs_event(&self.0.app, state);

        let preview_id = self.0.next_req_id.load(Ordering::SeqCst);
        let _ = self.0.app.emit(
            "browser_dock_event",
            serde_json::json!({
                "kind": "agent_action",
                "tabId": tab_id,
                "data": {
                    "reqId": preview_id,
                    "kind": "navigate",
                    "args": { "url": normalized },
                    "tabId": tab_id,
                    "ts": now_millis(),
                },
            }),
        );

        let webview = dock_webview(&self.0.app)
            .ok_or_else(|| anyhow::anyhow!("dock webview is not open"))?;
        webview
            .navigate(parsed)
            .map_err(|err| anyhow::anyhow!("webview navigate failed: {err}"))?;

        let wait_budget = Duration::from_millis(req.timeout_ms.max(ready_timeout.as_millis() as u64));
        match self.await_dock_ready(tab_id, wait_budget).await {
            Ok(()) => Ok(DockResponse {
                ok: true,
                value: serde_json::json!({
                    "navigated": true,
                    "url": normalized,
                    "tab_id": tab_id,
                }),
                error: None,
            }),
            Err(err) => Ok(DockResponse {
                ok: false,
                value: Value::Null,
                error: Some(format!(
                    "navigate dispatched but page did not signal ready in time: {err}"
                )),
            }),
        }
    }

    async fn exec_on_tab(&self, tab_id: TabId, req: DockRequest) -> Result<DockResponse> {
        let webview = dock_webview(&self.0.app)
            .ok_or_else(|| anyhow::anyhow!("dock webview is not open"))?;

        let req_id = self.0.next_req_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = oneshot::channel();
        self.0.pending.lock().insert(req_id, tx);
        self.0.pending_tab.lock().insert(req_id, tab_id);

        let payload = serde_json::json!({
            "reqId": req_id,
            "kind": req.kind,
            "args": req.args,
        });
        let payload_js = serde_json::to_string(&payload)
            .with_context(|| "serialise dock exec payload")?;
        let source = format!(
            "window.__senDockBridge && window.__senDockBridge.exec({});",
            payload_js
        );
        if let Err(err) = webview.eval(&source) {
            self.forget_request(req_id);
            return Err(anyhow::anyhow!("dock eval failed: {err}"));
        }

        let timeout = Duration::from_millis(req.timeout_ms.max(1_000));
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => {
                self.forget_request(req_id);
                Err(anyhow::anyhow!("dock bridge channel closed before reply"))
            }
            Err(_) => {
                self.forget_request(req_id);
                Err(anyhow::anyhow!(
                    "dock bridge timed out waiting for kind={} (tab={})",
                    req.kind,
                    tab_id
                ))
            }
        }
    }
}

fn render_dom_fallback_png(meta: &Value) -> Vec<u8> {
    use image::{ImageFormat, Rgba, RgbaImage};

    let title = meta
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("(no title)");
    let url = meta
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("(no url)");
    let viewport = meta
        .get("viewport")
        .map(|v| v.to_string())
        .unwrap_or_else(|| "{}".into());

    let header = format!(
        "DOM-fallback screenshot\nurl: {}\ntitle: {}\nviewport: {}",
        url.chars().take(160).collect::<String>(),
        title.chars().take(120).collect::<String>(),
        viewport.chars().take(120).collect::<String>(),
    );

    let width = 800u32;
    let height = 120u32;
    let mut img = RgbaImage::from_pixel(width, height, Rgba([245, 245, 250, 255]));
    let bytes = header.as_bytes();
    for (i, byte) in bytes.iter().enumerate().take((width * height) as usize) {
        let x = (i as u32) % width;
        let y = (i as u32) / width;
        let v = 80u8.saturating_add(*byte / 4);
        img.put_pixel(x, y, Rgba([v, v, v, 255]));
    }
    let mut out = Vec::with_capacity(8 * 1024);
    let _ = image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png);
    out
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn capture_dock_window(app: &AppHandle) -> Result<Vec<u8>> {
    use image::{ImageFormat, RgbaImage};

    let main_window = app
        .get_window("main")
        .ok_or_else(|| anyhow::anyhow!("main window unavailable"))?;
    let main_title = main_window.title().unwrap_or_default();

    let xcap_windows = xcap::Window::all()
        .map_err(|err| anyhow::anyhow!("xcap::Window::all failed: {err}"))?;
    let target = xcap_windows
        .into_iter()
        .find(|w| {
            let same_title = w
                .title()
                .map(|t| t == main_title)
                .unwrap_or(false);
            let same_app = w
                .app_name()
                .map(|a| a.contains("sen-desktop"))
                .unwrap_or(false);
            same_title || same_app
        })
        .ok_or_else(|| anyhow::anyhow!("could not locate main window via xcap"))?;
    let captured: RgbaImage = target
        .capture_image()
        .map_err(|err| anyhow::anyhow!("xcap capture_image failed: {err}"))?;

    let cropped: RgbaImage = if let Some(rect) =
        app.try_state::<DockSharedState>().and_then(|s| s.rect())
    {
        let scale = main_window.scale_factor().unwrap_or(1.0);
        let x = ((rect.x.max(0.0)) * scale) as u32;
        let y = ((rect.y.max(0.0)) * scale) as u32;
        let w = ((rect.w.max(1.0)) * scale) as u32;
        let h = ((rect.h.max(1.0)) * scale) as u32;
        let img_w = captured.width();
        let img_h = captured.height();
        let x = x.min(img_w.saturating_sub(1));
        let y = y.min(img_h.saturating_sub(1));
        let w = w.min(img_w.saturating_sub(x));
        let h = h.min(img_h.saturating_sub(y));
        if w > 0 && h > 0 {
            let mut sub = RgbaImage::new(w, h);
            for (sy, py) in (y..y + h).enumerate() {
                for (sx, px) in (x..x + w).enumerate() {
                    sub.put_pixel(sx as u32, sy as u32, *captured.get_pixel(px, py));
                }
            }
            sub
        } else {
            captured
        }
    } else {
        captured
    };

    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    image::DynamicImage::ImageRgba8(cropped)
        .write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)
        .map_err(|err| anyhow::anyhow!("png encode failed: {err}"))?;
    Ok(buf)
}

pub fn install_into(app: &AppHandle) {
    let controller = TauriDockController::new(app.clone());
    app.manage(controller.clone());
    senweavercoding::tools::browser::install_dock_controller(Arc::new(controller));

    if let Some(window) = app.get_window("main") {
        let app_for_resize = app.clone();
        window.on_window_event(move |event| {
            if matches!(
                event,
                tauri::WindowEvent::Resized(_) | tauri::WindowEvent::ScaleFactorChanged { .. }
            ) {
                if let Some(state) = app_for_resize.try_state::<DockSharedState>() {
                    let _ = update_dock_layout(&app_for_resize, state.inner());
                }
            }
        });
    }
}
