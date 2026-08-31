// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use senweavercoding::tools::browser::{
    clear_test_target_for_tab, clear_test_target_tab, current_test_target_tab,
    set_test_target_tab, DockController, DockRequest, DockResponse, DockTabInfo,
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
    format!("{header}\n{AUTOPLAY_GUARD_JS}\n{BRIDGE_JS}")
}

const AUTOPLAY_GUARD_JS: &str = r#"
(() => {
  if (window.__senAutoplayGuard) return;
  window.__senAutoplayGuard = true;
  let userInteracted = false;
  const mark = () => { userInteracted = true; };
  for (const ev of ['pointerdown', 'keydown', 'touchstart', 'mousedown']) {
    window.addEventListener(ev, mark, { capture: true });
  }
  const guard = (el) => {
    if (!el || el.__senAutoplayGuarded) return;
    el.__senAutoplayGuarded = true;
    el.addEventListener(
      'play',
      () => {
        if (!userInteracted && !el.muted && el.volume > 0) {
          try { el.pause(); } catch (_) {}
        }
      },
      true,
    );
  };
  const scan = (root) => {
    if (!root || !root.querySelectorAll) return;
    for (const el of root.querySelectorAll('video, audio')) guard(el);
  };
  scan(document);
  document.addEventListener('DOMContentLoaded', () => scan(document), { once: true });
  try {
    const mo = new MutationObserver((mutations) => {
      for (const m of mutations) {
        for (const node of m.addedNodes || []) {
          if (node.tagName === 'VIDEO' || node.tagName === 'AUDIO') guard(node);
          else scan(node);
        }
      }
    });
    mo.observe(document.documentElement, { childList: true, subtree: true });
  } catch (_) {}
})();
"#;

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

  const vitals = {
    lcp: null,
    cls: 0,
    inpMax: 0,
    longTasks: 0,
    longTaskTotalMs: 0,
    fcp: null,
  };
  try {
    new PerformanceObserver((list) => {
      const entries = list.getEntries();
      const last = entries[entries.length - 1];
      if (last) vitals.lcp = Math.round(last.startTime);
    }).observe({ type: 'largest-contentful-paint', buffered: true });
  } catch (_) {}
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        if (!e.hadRecentInput) vitals.cls += e.value;
      }
    }).observe({ type: 'layout-shift', buffered: true });
  } catch (_) {}
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        if (e.duration > vitals.inpMax) vitals.inpMax = Math.round(e.duration);
      }
    }).observe({ type: 'event', buffered: true, durationThreshold: 40 });
  } catch (_) {}
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        vitals.longTasks += 1;
        vitals.longTaskTotalMs += Math.round(e.duration);
      }
    }).observe({ type: 'longtask', buffered: true });
  } catch (_) {}
  try {
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        if (e.name === 'first-contentful-paint') vitals.fcp = Math.round(e.startTime);
      }
    }).observe({ type: 'paint', buffered: true });
  } catch (_) {}

  if (!navigator.modelContext) {
    const webMcpRegistry = new Map();
    try {
      Object.defineProperty(navigator, 'modelContext', {
        configurable: true,
        value: {
          registerTool(tool) {
            if (!tool || typeof tool.name !== 'string' || !tool.name ||
                typeof tool.description !== 'string' || !tool.description ||
                typeof tool.execute !== 'function') {
              return Promise.reject(new TypeError('invalid tool definition'));
            }
            if (webMcpRegistry.has(tool.name)) {
              return Promise.reject(new Error(`tool already registered: ${tool.name}`));
            }
            webMcpRegistry.set(tool.name, tool);
            return Promise.resolve();
          },
          unregisterTool(name) {
            webMcpRegistry.delete(name);
            return Promise.resolve();
          },
        },
      });
      window.__senWebMcpRegistry = webMcpRegistry;
    } catch (_) {}
  }

  const ringMax = 256;
  const consoleRing = [];
  const consolePending = [];
  const netErrorPending = [];
  let batchFlushTimer = null;
  function drainBatch(pending, kind) {
    while (pending.length) {
      const entries = pending.splice(0, 32);
      send(kind, { entries });
    }
  }
  function flushEventBatches() {
    batchFlushTimer = null;
    drainBatch(consolePending, 'console_batch');
    drainBatch(netErrorPending, 'network_error_batch');
  }
  function scheduleBatchFlush() {
    if (batchFlushTimer !== null) return;
    batchFlushTimer = setTimeout(flushEventBatches, 200);
  }
  function queueConsoleEntry(entry) {
    consoleRing.push(entry);
    while (consoleRing.length > ringMax) consoleRing.shift();
    consolePending.push(entry);
    while (consolePending.length > ringMax) consolePending.shift();
    scheduleBatchFlush();
  }
  const wrapConsole = (level) => {
    const orig = console[level] && console[level].bind(console);
    if (!orig) return;
    console[level] = (...args) => {
      try {
        const message = args.map((a) => {
          if (typeof a === 'string') return a;
          try { return JSON.stringify(a); } catch (_) { return String(a); }
        }).join(' ');
        queueConsoleEntry({ level, message, ts: Date.now() });
      } catch (_) {}
      return orig(...args);
    };
  };
  ['log', 'info', 'warn', 'error', 'debug'].forEach(wrapConsole);

  window.addEventListener('error', (ev) => {
    try {
      queueConsoleEntry({ level: 'error', message: `[uncaught] ${ev.message} (${ev.filename}:${ev.lineno})`, ts: Date.now() });
    } catch (_) {}
  });
  window.addEventListener('unhandledrejection', (ev) => {
    try {
      const reason = ev.reason && (ev.reason.stack || ev.reason.message || String(ev.reason));
      queueConsoleEntry({ level: 'error', message: `[unhandledrejection] ${reason}`, ts: Date.now() });
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
      netErrorPending.push(safe);
      while (netErrorPending.length > netErrorsMax) netErrorPending.shift();
      scheduleBatchFlush();
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
    get_styles(args) {
      const sel = args && args.selector;
      if (sel) {
        const el = findOne(sel);
        return { selector: sel, tag: el.tagName.toLowerCase(), styles: computedStyleOf(el) };
      }
      const limit = (args && Number(args.limit)) || 600;
      const textColors = new Map();
      const bgColors = new Map();
      const fontFamilies = new Map();
      const fontSizes = new Map();
      const radii = new Map();
      const bump = (m, k) => { if (k) m.set(k, (m.get(k) || 0) + 1); };
      const all = document.querySelectorAll('body *');
      let sampled = 0;
      for (let i = 0; i < all.length && sampled < limit; i++) {
        const el = all[i];
        if (!isVisibleEl(el)) continue;
        const r = el.getBoundingClientRect();
        if (r.width < 2 || r.height < 2) continue;
        sampled++;
        const cs = getComputedStyle(el);
        const hasOwnText = Array.prototype.some.call(
          el.childNodes,
          (n) => n.nodeType === 3 && n.nodeValue && n.nodeValue.trim(),
        );
        if (hasOwnText) {
          bump(textColors, cs.color);
          bump(fontFamilies, cs.fontFamily);
          bump(fontSizes, cs.fontSize);
        }
        const bg = cs.backgroundColor;
        if (bg && bg !== 'rgba(0, 0, 0, 0)' && bg !== 'transparent') bump(bgColors, bg);
        const br = cs.borderRadius;
        if (br && br !== '0px') bump(radii, br);
      }
      const top = (m, n) => Array.from(m.entries())
        .sort((a, b) => b[1] - a[1])
        .slice(0, n)
        .map(([value, count]) => ({ value, count }));
      return {
        url: window.location.href,
        audit: {
          sampled_elements: sampled,
          distinct_text_colors: textColors.size,
          text_colors: top(textColors, 16),
          distinct_background_colors: bgColors.size,
          background_colors: top(bgColors, 16),
          distinct_font_families: fontFamilies.size,
          font_families: top(fontFamilies, 8),
          distinct_font_sizes: fontSizes.size,
          font_sizes: top(fontSizes, 12),
          distinct_border_radii: radii.size,
          border_radii: top(radii, 8),
        },
      };
    },
    perf_vitals() {
      const nav = performance.getEntriesByType('navigation')[0] || null;
      const resources = performance.getEntriesByType('resource');
      let transfer = 0;
      for (const r of resources) transfer += (r.transferSize || 0);
      const out = {
        url: window.location.href,
        lcp_ms: vitals.lcp,
        fcp_ms: vitals.fcp,
        cls: Math.round(vitals.cls * 1000) / 1000,
        inp_worst_ms: vitals.inpMax || null,
        long_tasks: vitals.longTasks,
        long_task_total_ms: vitals.longTaskTotalMs,
        resource_count: resources.length,
        transfer_bytes: transfer,
        js_heap_used_bytes: (performance.memory && performance.memory.usedJSHeapSize) || null,
      };
      if (nav) {
        out.ttfb_ms = Math.round(nav.responseStart - nav.requestStart);
        out.dom_content_loaded_ms = Math.round(nav.domContentLoadedEventEnd);
        out.load_ms = Math.round(nav.loadEventEnd || 0) || null;
        out.protocol = nav.nextHopProtocol || null;
      }
      out.verdict = {
        lcp: out.lcp_ms == null ? 'unknown' : out.lcp_ms <= 2500 ? 'good' : out.lcp_ms <= 4000 ? 'needs-improvement' : 'poor',
        cls: out.cls <= 0.1 ? 'good' : out.cls <= 0.25 ? 'needs-improvement' : 'poor',
        inp: out.inp_worst_ms == null ? 'unknown' : out.inp_worst_ms <= 200 ? 'good' : out.inp_worst_ms <= 500 ? 'needs-improvement' : 'poor',
      };
      return out;
    },
    web_tools_list() {
      const reg = window.__senWebMcpRegistry;
      if (reg && reg.size) {
        const tools = [];
        for (const t of reg.values()) {
          tools.push({
            name: t.name,
            description: String(t.description || '').slice(0, 500),
            input_schema: t.inputSchema || null,
          });
        }
        return { available: true, count: tools.length, tools, url: window.location.href };
      }
      if (navigator.modelContext && !window.__senWebMcpRegistry) {
        return { available: false, native_api_present: true, count: 0, tools: [], url: window.location.href };
      }
      return { available: false, count: 0, tools: [], url: window.location.href };
    },
    async web_tools_call(args) {
      const name = args && args.name;
      if (!name) throw new Error('web_tools_call requires args.name');
      const reg = window.__senWebMcpRegistry;
      const tool = reg && reg.get(name);
      if (!tool) throw new Error(`webmcp tool not registered on this page: ${name}`);
      let input = (args && args.tool_args) || {};
      if (typeof input === 'string') {
        try { input = JSON.parse(input); } catch (_) { input = { value: input }; }
      }
      const started = Date.now();
      const result = await Promise.resolve(tool.execute(input));
      let payload = result;
      try {
        const text = JSON.stringify(result);
        if (text && text.length > 16000) payload = { truncated: true, preview: text.slice(0, 16000) };
      } catch (_) {
        payload = String(result).slice(0, 16000);
      }
      return { name, elapsed_ms: Date.now() - started, result: payload };
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
    last_applied_dock_geometry: Option<(f64, f64, f64, f64)>,
    tabs: HashMap<TabId, TabRecord>,
    order: Vec<TabId>,
    active: Option<TabId>,
    next_id: TabId,
    parked: bool,
    dock_visible: bool,
    last_state_url: HashMap<TabId, String>,
    agent_tab_id: Option<TabId>,
    agent_tabs_by_session: HashMap<String, Vec<TabId>>,
    tab_session: HashMap<TabId, String>,
    session_active_tab: HashMap<String, TabId>,
    foreground_session_id: Option<String>,
}

const GW_DOCK_SESSION_PREFIX: &str = "gw_";

fn canonical_dock_session_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    trimmed
        .strip_prefix(GW_DOCK_SESSION_PREFIX)
        .unwrap_or(trimmed)
        .to_string()
}

fn canonical_dock_session_id_opt(opt: Option<&str>) -> Option<String> {
    opt.map(canonical_dock_session_id)
        .filter(|s| !s.is_empty())
}

fn session_ids_equivalent(a: &str, b: &str) -> bool {
    canonical_dock_session_id(a) == canonical_dock_session_id(b)
}

fn reconcile_legacy_session_keys(g: &mut TabsState) {
    for sid in g.tab_session.values_mut() {
        let canon = canonical_dock_session_id(sid);
        if canon != *sid {
            *sid = canon;
        }
    }
    if let Some(ref fg) = g.foreground_session_id.clone() {
        g.foreground_session_id = Some(canonical_dock_session_id(fg));
    }
    let remembered: Vec<_> = g.session_active_tab.drain().collect();
    for (sid, tab) in remembered {
        let canon = canonical_dock_session_id(&sid);
        g.session_active_tab
            .entry(canon)
            .and_modify(|existing| {
                if g.tabs.contains_key(&tab) && !g.tabs.contains_key(existing) {
                    *existing = tab;
                }
            })
            .or_insert(tab);
    }
    let buckets: Vec<_> = g.agent_tabs_by_session.drain().collect();
    for (sid, bucket) in buckets {
        let canon = canonical_dock_session_id(&sid);
        let entry = g.agent_tabs_by_session.entry(canon).or_default();
        for tab in bucket {
            if !entry.contains(&tab) {
                entry.push(tab);
            }
        }
    }
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
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
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

    fn last_applied_dock_geometry(&self) -> Option<(f64, f64, f64, f64)> {
        self.0.lock().last_applied_dock_geometry
    }

    fn set_last_applied_dock_geometry(&self, geometry: Option<(f64, f64, f64, f64)>) {
        self.0.lock().last_applied_dock_geometry = geometry;
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

    fn foreground_session_id(&self) -> Option<String> {
        self.0.lock().foreground_session_id.clone()
    }

    fn set_foreground_session_id(&self, session_id: Option<String>) {
        let mut g = self.0.lock();
        reconcile_legacy_session_keys(&mut g);
        g.foreground_session_id = canonical_dock_session_id_opt(session_id.as_deref());
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

    fn tab_session_of(&self, id: TabId) -> Option<String> {
        self.0.lock().tab_session.get(&id).cloned()
    }

    fn register_tab(&self, id: TabId, url: Option<String>, owner: TabOwner) {
        let session_id = current_session_id();
        self.register_tab_for_session(id, url, owner, session_id.as_deref());
    }

    fn register_tab_for_session(
        &self,
        id: TabId,
        url: Option<String>,
        owner: TabOwner,
        explicit_session: Option<&str>,
    ) {
        let session_id = canonical_dock_session_id_opt(explicit_session)
            .or_else(|| canonical_dock_session_id_opt(current_session_id().as_deref()));
        let mut g = self.0.lock();
        reconcile_legacy_session_keys(&mut g);
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
        if matches!(owner, TabOwner::Agent) {
            if g.agent_tab_id.is_none() {
                g.agent_tab_id = Some(id);
            }
            if let Some(sid) = session_id {
                let bucket = g.agent_tabs_by_session.entry(sid.clone()).or_default();
                if !bucket.contains(&id) {
                    bucket.push(id);
                }
                g.tab_session.insert(id, sid);
            }
        } else if let Some(sid) = session_id {
            g.tab_session.insert(id, sid.clone());
            let bucket = g.agent_tabs_by_session.entry(sid).or_default();
            if !bucket.contains(&id) {
                bucket.push(id);
            }
        }
    }

    fn acquire_or_create_user_tab(
        &self,
        url: Option<String>,
        session_id: Option<&str>,
    ) -> (TabId, bool) {
        let session_normalized = canonical_dock_session_id_opt(session_id);
        let mut g = self.0.lock();
        reconcile_legacy_session_keys(&mut g);
        if let Some(ref sid) = session_normalized {
            let owned_user = g
                .order
                .iter()
                .rev()
                .find(|tid| {
                    let owner_match = g
                        .tabs
                        .get(tid)
                        .is_some_and(|rec| matches!(rec.owner, TabOwner::User));
                    let session_match = g
                        .tab_session
                        .get(tid)
                        .is_some_and(|s| session_ids_equivalent(s, sid));
                    owner_match && session_match
                })
                .copied();
            if let Some(uid) = owned_user {
                g.active = Some(uid);
                return (uid, false);
            }
        } else if let Some(active) = g.active {
            if let Some(rec) = g.tabs.get(&active) {
                if matches!(rec.owner, TabOwner::User)
                    && !g.tab_session.contains_key(&active)
                {
                    return (active, false);
                }
            }
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
        if let Some(sid) = session_normalized {
            g.tab_session.insert(id, sid.clone());
            let bucket = g.agent_tabs_by_session.entry(sid).or_default();
            if !bucket.contains(&id) {
                bucket.push(id);
            }
        }
        (id, true)
    }

    fn acquire_or_create_agent_tab(&self, url: Option<String>) -> (TabId, bool) {
        let session_id = current_session_id();
        if let Some(ref sid) = session_id {
            return self.acquire_or_create_agent_tab_for_session(sid, url);
        }
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

    fn acquire_or_create_agent_tab_for_session(
        &self,
        session_id: &str,
        url: Option<String>,
    ) -> (TabId, bool) {
        let session_id = canonical_dock_session_id(session_id);
        if session_id.is_empty() {
            return self.acquire_or_create_agent_tab(url);
        }
        let mut g = self.0.lock();
        reconcile_legacy_session_keys(&mut g);
        if let Some(bucket) = g.agent_tabs_by_session.get(&session_id) {
            if let Some(existing) = bucket
                .iter()
                .rev()
                .find(|tid| g.tabs.contains_key(tid))
                .copied()
            {
                if let Some(target) = url {
                    if let Some(rec) = g.tabs.get_mut(&existing) {
                        rec.last_url = Some(target);
                    }
                }
                if g.agent_tab_id.is_none() {
                    g.agent_tab_id = Some(existing);
                }
                return (existing, false);
            }
            g.agent_tabs_by_session.remove(&session_id);
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
        if g.agent_tab_id.is_none() {
            g.agent_tab_id = Some(id);
        }
        g.agent_tabs_by_session
            .entry(session_id.clone())
            .or_default()
            .push(id);
        g.tab_session.insert(id, session_id);
        if g.active.is_none() {
            g.active = Some(id);
        }
        (id, true)
    }

    fn agent_tab_id(&self) -> Option<TabId> {
        self.0.lock().agent_tab_id
    }

    fn agent_tab_id_for_session(&self, session_id: &str) -> Option<TabId> {
        let session_id = canonical_dock_session_id(session_id);
        if session_id.is_empty() {
            return None;
        }
        let g = self.0.lock();
        g.agent_tabs_by_session
            .get(&session_id)
            .and_then(|bucket| {
                bucket
                    .iter()
                    .rev()
                    .find(|tid| g.tabs.contains_key(tid))
                    .copied()
            })
    }

    fn bind_user_tab_to_session(
        &self,
        session_id: &str,
        tab_id: TabId,
    ) -> Result<(), String> {
        let session_id = canonical_dock_session_id(session_id);
        if session_id.is_empty() {
            return Err("session_id is required".to_string());
        }
        {
            let mut g = self.0.lock();
            reconcile_legacy_session_keys(&mut g);
            if !g.tabs.contains_key(&tab_id) {
                return Err(format!("unknown tab id {tab_id}"));
            }
            if let Some(prev) = g.tab_session.get(&tab_id).cloned() {
                if !session_ids_equivalent(&prev, &session_id) {
                    return Err(format!(
                        "tab {tab_id} belongs to session {prev}, cannot bind to {session_id}"
                    ));
                }
            } else {
                g.tab_session.insert(tab_id, session_id.clone());
            }
            let bucket = g
                .agent_tabs_by_session
                .entry(session_id.to_string())
                .or_default();
            bucket.retain(|t| *t != tab_id);
            bucket.push(tab_id);
            if g.agent_tab_id.is_none() {
                g.agent_tab_id = Some(tab_id);
            }
        }
        set_test_target_tab(&session_id, tab_id);
        Ok(())
    }

    fn unbind_tab_from_session(
        &self,
        session_id: &str,
        tab_id: TabId,
    ) -> Result<(), String> {
        let session_id = canonical_dock_session_id(session_id);
        if session_id.is_empty() {
            return Err("session_id is required".to_string());
        }
        {
            let mut g = self.0.lock();
            reconcile_legacy_session_keys(&mut g);
            if let Some(bucket) = g.agent_tabs_by_session.get_mut(&session_id) {
                bucket.retain(|t| *t != tab_id);
                if bucket.is_empty() {
                    g.agent_tabs_by_session.remove(&session_id);
                }
            }
            if g.tab_session.get(&tab_id).is_some_and(|s| session_ids_equivalent(s, &session_id)) {
                g.tab_session.remove(&tab_id);
            }
            if g.agent_tab_id == Some(tab_id) {
                let next_agent = g
                    .agent_tabs_by_session
                    .values()
                    .flat_map(|bucket| bucket.iter().rev())
                    .find(|tid| g.tabs.contains_key(tid))
                    .copied();
                g.agent_tab_id = next_agent;
            }
        }
        if current_test_target_tab(&session_id) == Some(tab_id) {
            clear_test_target_tab(&session_id);
        }
        Ok(())
    }

    fn release_agent_tabs_for_session(&self, session_id: &str) -> Vec<TabId> {
        let session_id = canonical_dock_session_id(session_id);
        if session_id.is_empty() {
            return Vec::new();
        }
        let released = {
            let mut g = self.0.lock();
            reconcile_legacy_session_keys(&mut g);
            let mut released = Vec::new();
            if let Some(bucket) = g.agent_tabs_by_session.remove(&session_id) {
                for tab_id in bucket {
                    if !released.contains(&tab_id) {
                        released.push(tab_id);
                    }
                }
            }
            let extras: Vec<TabId> = g
                .tab_session
                .iter()
                .filter_map(|(tab, sid)| {
                    if session_ids_equivalent(sid, &session_id) {
                        Some(*tab)
                    } else {
                        None
                    }
                })
                .collect();
            for tab_id in extras {
                if !released.contains(&tab_id) {
                    released.push(tab_id);
                }
            }
            for tab_id in &released {
                g.tab_session.remove(tab_id);
                if g.agent_tab_id == Some(*tab_id) {
                    let next_agent = g
                        .agent_tabs_by_session
                        .values()
                        .flat_map(|bucket| bucket.iter().rev())
                        .find(|tid| g.tabs.contains_key(tid))
                        .copied();
                    g.agent_tab_id = next_agent;
                }
            }
            released
        };
        for tab_id in &released {
            clear_test_target_for_tab(*tab_id);
        }
        released
    }

    fn tab_owner(&self, id: TabId) -> Option<TabOwner> {
        self.0.lock().tabs.get(&id).map(|r| r.owner)
    }

    fn remove_tab(&self, id: TabId) -> Option<TabId> {
        let active = {
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
            if let Some(sid) = g.tab_session.remove(&id) {
                if let Some(bucket) = g.agent_tabs_by_session.get_mut(&sid) {
                    bucket.retain(|t| *t != id);
                    if bucket.is_empty() {
                        g.agent_tabs_by_session.remove(&sid);
                    }
                }
            }
            if g.agent_tab_id.is_none() {
                g.agent_tab_id = g
                    .agent_tabs_by_session
                    .values()
                    .flat_map(|bucket| bucket.iter().rev())
                    .find(|tid| g.tabs.contains_key(tid))
                    .copied();
            }
            g.active
        };
        #[cfg(windows)]
        {
            let mut log = cdp::net_log().lock();
            log.tabs.remove(&id);
            if log.active_capture == Some(id) {
                log.active_capture = None;
            }
        }
        active
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
                    session_id: g.tab_session.get(id).cloned(),
                })
            })
            .collect()
    }

    fn active_session_id(&self) -> Option<String> {
        let g = self.0.lock();
        g.active.and_then(|id| g.tab_session.get(&id).cloned())
    }

    fn present_session_internal(&self, session_id: &str) -> Option<TabId> {
        let session_id = canonical_dock_session_id(session_id);
        if session_id.is_empty() {
            return None;
        }
        let mut g = self.0.lock();
        reconcile_legacy_session_keys(&mut g);
        if let Some(active) = g.active {
            if let Some(prev_session) = g.tab_session.get(&active).cloned() {
                if !prev_session.is_empty() {
                    g.session_active_tab
                        .insert(canonical_dock_session_id(&prev_session), active);
                }
            }
        }
        if let Some(&remembered) = g.session_active_tab.get(&session_id) {
            if g.tabs.contains_key(&remembered) {
                return Some(remembered);
            }
        }
        if let Some(bucket) = g.agent_tabs_by_session.get(&session_id) {
            let candidate = bucket
                .iter()
                .rev()
                .find(|tid| g.tabs.contains_key(tid))
                .copied();
            if let Some(tab_id) = candidate {
                return Some(tab_id);
            }
        }
        g.tab_session
            .iter()
            .find_map(|(tab, sid)| {
                if session_ids_equivalent(sid, &session_id) && g.tabs.contains_key(tab) {
                    Some(*tab)
                } else {
                    None
                }
            })
    }

    fn set_url(&self, id: TabId, url: impl Into<String>) {
        let mut g = self.0.lock();
        if let Some(rec) = g.tabs.get_mut(&id) {
            rec.last_url = Some(canonicalize_loopback_url(&url.into()));
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

    fn find_owner_tab_with_url(
        &self,
        owner: TabOwner,
        target_url: &str,
        session_scope: Option<&str>,
    ) -> Option<TabId> {
        let normalized = normalize_url_for_match(target_url);
        if normalized.is_empty() {
            return None;
        }
        let scope = canonical_dock_session_id_opt(session_scope);
        let g = self.0.lock();
        for id in &g.order {
            let Some(rec) = g.tabs.get(id) else { continue };
            if rec.owner != owner {
                continue;
            }
            if let Some(ref sid) = scope {
                let bound = g.tab_session.get(id);
                if !bound.is_some_and(|s| session_ids_equivalent(s, sid)) {
                    continue;
                }
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
    let normalized = canonicalize_loopback_url(&raw);
    Url::parse(&normalized).map_err(|err| format!("invalid url '{raw}': {err}"))
}

fn canonicalize_loopback_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == ABOUT_BLANK {
        return trimmed.to_string();
    }
    let Ok(mut url) = Url::parse(trimmed) else {
        return trimmed.to_string();
    };
    match url.scheme() {
        "http" | "https" | "ws" | "wss" => {}
        _ => return trimmed.to_string(),
    }
    let Some(host) = url.host_str() else {
        return trimmed.to_string();
    };
    if host.ends_with(".localhost") {
        return trimmed.to_string();
    }
    let rewrite = host.eq_ignore_ascii_case("localhost") || host == "::1";
    if !rewrite {
        return trimmed.to_string();
    }
    if url.set_host(Some("127.0.0.1")).is_ok() {
        url.to_string()
    } else {
        trimmed.to_string()
    }
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
                let opener = state.active();
                let app_clone = app.clone();
                let app_for_err = app.clone();
                let url_for_err = url_owned.clone();
                if let Err(err) = app.run_on_main_thread(move || {
                    if let Err(err) =
                        open_url_in_new_tab(&app_clone, url_owned.clone(), opener)
                    {
                        tracing::warn!(
                            "[browser_dock] openNewTab open_url_in_new_tab failed: {err}"
                        );
                        emit_dock_error(&app_clone, opener, "openNewTab", &url_owned, &err);
                    }
                }) {
                    tracing::warn!(
                        "[browser_dock] openNewTab run_on_main_thread failed: {err}"
                    );
                    emit_dock_error(
                        &app_for_err,
                        opener,
                        "openNewTab",
                        &url_for_err,
                        &err.to_string(),
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
                if !state_url_allowed_for_tab(prev.clone(), url) {
                    return;
                }
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

    let session_id = active_tab.and_then(|id| state.tab_session_of(id));
    let payload = serde_json::json!({
        "kind": kind,
        "tabId": active_tab,
        "sessionId": session_id,
        "data": parsed_data,
    });

    if let Err(err) = app.emit_to("main", "browser_dock_event", payload) {
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

fn emit_dock_error(
    app: &AppHandle,
    tab_id: Option<TabId>,
    action: &str,
    url: &str,
    message: &str,
) {
    let session_id = tab_id.and_then(|id| {
        app.try_state::<DockSharedState>()
            .and_then(|s| s.tab_session_of(id))
    });
    let payload = serde_json::json!({
        "kind": "error",
        "tabId": tab_id,
        "sessionId": session_id,
        "data": {
            "action": action,
            "url": url,
            "message": message,
            "tabId": tab_id,
            "sessionId": session_id,
            "ts": now_millis(),
        },
    });
    if let Err(err) = app.emit_to("main", "browser_dock_event", payload) {
        tracing::warn!("[browser_dock] emit dock error event failed: {err}");
    }
}

fn emit_tabs_event(app: &AppHandle, state: &DockSharedState) {
    let tabs = state.list();
    let active = state.active();
    let active_session = state.foreground_session_id().or_else(|| state.active_session_id());
    if let Err(err) = app.emit_to(
        "main",
        "browser_dock_event",
        serde_json::json!({
            "kind": "tabs",
            "sessionId": active_session,
            "data": {
                "tabs": tabs,
                "active": active,
                "activeSessionId": active_session,
            },
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

static DOCK_MAIN_THREAD_ID: std::sync::OnceLock<std::thread::ThreadId> =
    std::sync::OnceLock::new();

fn record_dock_main_thread() {
    let _ = DOCK_MAIN_THREAD_ID.set(std::thread::current().id());
}

fn dock_thread_is_main() -> bool {
    DOCK_MAIN_THREAD_ID
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

fn with_dock_on_main_thread<F>(app: &AppHandle, label: &'static str, f: F) -> Result<(), String>
where
    F: FnOnce(&AppHandle) -> Result<(), String> + Send + 'static,
{
    if dock_thread_is_main() {
        return f(app);
    }
    let app_for_main = app.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    app.run_on_main_thread(move || {
        let _ = tx.send(f(&app_for_main));
    })
    .map_err(|e| format!("schedule {label} on the main thread failed: {e}"))?;
    blocking_recv_without_starving_runtime(&rx, Duration::from_secs(15))
        .map_err(|e| format!("await {label} result failed: {e}"))?
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

    let raw_rect = state.rect();
    let initial_size = dock_logical_size(raw_rect);
    let initial_position = dock_logical_position(raw_rect);

    let app_for_attach = app.clone();
    let (add_tx, add_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    app.run_on_main_thread(move || {
        let result = build_and_attach_dock_webview(
            &app_for_attach,
            &main,
            parsed,
            initial_position,
            initial_size,
        );
        let _ = add_tx.send(result);
    })
    .map_err(|e| {
        format!("schedule add_child({DOCK_WEBVIEW_LABEL}) on the main thread failed: {e}")
    })?;
    blocking_recv_without_starving_runtime(&add_rx, Duration::from_secs(15))
        .map_err(|e| format!("await add_child({DOCK_WEBVIEW_LABEL}) result failed: {e}"))??;

    state.set_last_applied_dock_geometry(None);
    state.set_dock_visible(true);

    dock_webview(app)
        .ok_or_else(|| "dock webview missing immediately after add_child".to_string())
}

fn build_and_attach_dock_webview(
    app: &AppHandle,
    main: &Window,
    parsed: Url,
    initial_position: LogicalPosition<f64>,
    initial_size: LogicalSize<f64>,
) -> Result<(), String> {
    if dock_webview(app).is_some() {
        return Ok(());
    }

    let app_for_new_window = app.clone();
    let bridge_js = build_bridge_js();
    let app_for_nav = app.clone();

    let builder =
        WebviewBuilder::new(DOCK_WEBVIEW_LABEL, WebviewUrl::External(parsed))
            .initialization_script(bridge_js)
            .additional_browser_args(
                "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection \
                 --autoplay-policy=document-user-activation-required \
                 --disable-background-timer-throttling \
                 --disable-renderer-backgrounding \
                 --disable-backgrounding-occluded-windows",
            )
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
                let opener = app_for_new_window
                    .try_state::<DockSharedState>()
                    .and_then(|s| s.inner().active());
                let app_for_task = app_for_new_window.clone();
                if let Err(err) =
                    app_for_new_window.run_on_main_thread(move || {
                        if let Err(err) =
                            open_url_in_new_tab(&app_for_task, url_string, opener)
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

    main.add_child(builder, initial_position, initial_size)
        .map(|_| ())
        .map_err(|e| format!("add_child({DOCK_WEBVIEW_LABEL}) failed: {e}"))
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
    let session_matches_foreground = match (
        state.active_session_id(),
        state.foreground_session_id(),
    ) {
        (Some(active), Some(foreground)) => session_ids_equivalent(&active, &foreground),
        (None, Some(_)) => false,
        _ => true,
    };
    let want_visible = has_active && rect.is_some() && !parked && session_matches_foreground;

    if dock_webview(app).is_none() {
        return Ok(());
    }

    let was_visible = state.dock_visible();
    let toggle_visible = want_visible != was_visible;
    let pos_size = rect.map(|r| (r.position_logical(), r.size_logical()));
    let geometry_key = pos_size.map(|(pos, size)| (pos.x, pos.y, size.width, size.height));
    let geometry_unchanged =
        geometry_key.is_some() && state.last_applied_dock_geometry() == geometry_key;

    if !toggle_visible && geometry_unchanged {
        return Ok(());
    }

    with_dock_on_main_thread(app, "dock layout", move |app| {
        let Some(wv) = dock_webview(app) else {
            return Ok(());
        };
        if let Some((pos, size)) = pos_size {
            if !geometry_unchanged {
                wv.set_position(pos)
                    .map_err(|e| format!("set_position(dock) failed: {e}"))?;
                wv.set_size(size)
                    .map_err(|e| format!("set_size(dock) failed: {e}"))?;
            }
        }
        if toggle_visible {
            if want_visible {
                wv.show().map_err(|e| format!("show(dock) failed: {e}"))?;
            } else {
                wv.hide().map_err(|e| format!("hide(dock) failed: {e}"))?;
            }
        }
        Ok(())
    })?;

    if geometry_key.is_some() {
        state.set_last_applied_dock_geometry(geometry_key);
    }

    if toggle_visible {
        if want_visible {
            if let Some(active) = state.active() {
                state.forget_state_url(active);
            }
            let _ = dock_navigate_active(app, state);
        }
        state.set_dock_visible(want_visible);
    }
    Ok(())
}

fn effective_nav_url(stored: Option<String>) -> String {
    stored
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| ABOUT_BLANK.to_string())
}

fn state_url_allowed_for_tab(stored: Option<String>, reported: &str) -> bool {
    let expected = effective_nav_url(stored);
    urls_logically_match(reported, &expected)
}

fn dock_navigate_active(app: &AppHandle, state: &DockSharedState) -> Result<(), String> {
    let Some(active) = state.active() else {
        return Ok(());
    };
    let stored = state.snapshot_tab(active).0;
    let target = effective_nav_url(stored);
    let parsed = parse_target_url(Some(target))?;
    with_dock_on_main_thread(app, "dock navigate", move |app| {
        let Some(wv) = dock_webview(app) else {
            return Ok(());
        };
        let Some(state) = app.try_state::<DockSharedState>() else {
            return Ok(());
        };
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
    })
}

fn focus_dock_webview(app: &AppHandle) {
    let _ = with_dock_on_main_thread(app, "dock focus", move |app| {
        let Some(webview) = dock_webview(app) else {
            return Ok(());
        };
        #[cfg(windows)]
        {
            let _ = webview.with_webview(focus_webview2_native);
        }
        let _ = webview.set_focus();
        Ok(())
    });
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

#[cfg(windows)]
pub(crate) mod cdp {
    use std::collections::{HashMap, HashSet};
    use std::sync::OnceLock;
    use std::time::Duration;

    use parking_lot::Mutex;
    use serde_json::{json, Value};
    use tauri::Manager;
    use windows::core::HSTRING;

    use super::TabId;

    const NET_LOG_CAP: usize = 600;

    pub struct NetEntry {
        pub url: String,
        pub method: String,
        pub resource_type: Option<String>,
        pub status: Option<u64>,
        pub mime: Option<String>,
        pub encoded_len: Option<f64>,
        pub error: Option<String>,
        pub started_ms: i64,
        pub finished_ms: Option<i64>,
    }

    #[derive(Default)]
    pub struct TabNetLog {
        pub capturing: bool,
        pub order: Vec<String>,
        pub map: HashMap<String, NetEntry>,
    }

    pub struct NetLog {
        pub tabs: HashMap<TabId, TabNetLog>,
        pub subscribed: HashSet<usize>,
        pub event_tokens: HashMap<usize, Vec<(String, i64)>>,
        pub active_capture: Option<TabId>,
    }

    impl NetLog {
        pub fn bucket_mut(&mut self, tab_id: TabId) -> &mut TabNetLog {
            self.tabs.entry(tab_id).or_default()
        }
    }

    pub fn net_log() -> &'static Mutex<NetLog> {
        static LOG: OnceLock<Mutex<NetLog>> = OnceLock::new();
        LOG.get_or_init(|| {
            Mutex::new(NetLog {
                tabs: HashMap::new(),
                subscribed: HashSet::new(),
                event_tokens: HashMap::new(),
                active_capture: None,
            })
        })
    }

    pub async fn call(
        webview: &tauri::Webview,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> anyhow::Result<Value> {
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
        let method_s = method.to_string();
        let params_s =
            serde_json::to_string(&params).unwrap_or_else(|_| "{}".to_string());
        let app = webview.app_handle().clone();
        let wv = webview.clone();
        super::with_dock_on_main_thread(&app, "cdp call dispatch", move |_app| {
            wv.with_webview(move |platform| {
                use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
                let outcome: windows::core::Result<()> = (move || {
                    let controller = platform.controller();
                    let core = unsafe { controller.CoreWebView2() }?;
                    let handler = CallDevToolsProtocolMethodCompletedHandler::create(
                        Box::new(move |hr: windows::core::Result<()>, body: String| {
                            let _ = tx.send(match hr {
                                Ok(()) => Ok(body),
                                Err(e) => Err(e.message().to_string()),
                            });
                            Ok(())
                        }),
                    );
                    let method_h = HSTRING::from(method_s.as_str());
                    let params_h = HSTRING::from(params_s.as_str());
                    unsafe {
                        core.CallDevToolsProtocolMethod(&method_h, &params_h, &handler)
                    }
                })();
                if let Err(err) = outcome {
                    tracing::warn!("[browser_dock] cdp dispatch failed: {err}");
                }
            })
            .map_err(|e| format!("with_webview: {e}"))
        })
        .map_err(|e| anyhow::anyhow!(e))?;
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(Ok(body))) => Ok(serde_json::from_str(&body)
                .unwrap_or_else(|_| json!({ "raw": body }))),
            Ok(Ok(Err(err))) => Err(anyhow::anyhow!("cdp {method}: {err}")),
            Ok(Err(_)) => Err(anyhow::anyhow!(
                "cdp {method}: dispatch dropped (webview2 unavailable)"
            )),
            Err(_) => Err(anyhow::anyhow!("cdp {method}: timeout")),
        }
    }

    pub fn subscribe_network(webview: &tauri::Webview) -> anyhow::Result<()> {
        let app = webview.app_handle().clone();
        let wv = webview.clone();
        super::with_dock_on_main_thread(&app, "cdp subscribe network", move |_app| {
            wv.with_webview(|platform| {
                use webview2_com::DevToolsProtocolEventReceivedEventHandler;
                use windows::core::Interface;
                let outcome: windows::core::Result<()> = (|| {
                    let controller = platform.controller();
                    let core = unsafe { controller.CoreWebView2() }?;
                    let key = core.as_raw() as usize;
                    {
                        let mut log = net_log().lock();
                        if !log.subscribed.insert(key) {
                            return Ok(());
                        }
                    }
                    let mut tokens: Vec<(String, i64)> = Vec::with_capacity(4);
                    for event in [
                        "Network.requestWillBeSent",
                        "Network.responseReceived",
                        "Network.loadingFinished",
                        "Network.loadingFailed",
                    ] {
                        let event_h = HSTRING::from(event);
                        let receiver = unsafe {
                            core.GetDevToolsProtocolEventReceiver(&event_h)
                        }?;
                        let event_name = event.to_string();
                        let handler = DevToolsProtocolEventReceivedEventHandler::create(
                            Box::new(move |_sender, args| {
                                if let Some(args) = args {
                                    let mut raw = windows::core::PWSTR::null();
                                    if unsafe { args.ParameterObjectAsJson(&mut raw) }
                                        .is_ok()
                                    {
                                        let body = webview2_com::take_pwstr(raw);
                                        record_net_event(&event_name, &body);
                                    }
                                }
                                Ok(())
                            }),
                        );
                        let mut token: i64 = 0;
                        unsafe {
                            receiver.add_DevToolsProtocolEventReceived(&handler, &mut token)
                        }?;
                        tokens.push((event.to_string(), token));
                    }
                    net_log().lock().event_tokens.insert(key, tokens);
                    Ok(())
                })();
                if let Err(err) = outcome {
                    tracing::warn!("[browser_dock] cdp network subscribe failed: {err}");
                }
            })
            .map_err(|e| format!("with_webview: {e}"))
        })
        .map_err(|e| anyhow::anyhow!(e))
    }

    pub fn unsubscribe_network(app: &tauri::AppHandle) {
        let Some(webview) = app.get_webview(super::DOCK_WEBVIEW_LABEL) else {
            let mut log = net_log().lock();
            log.subscribed.clear();
            log.event_tokens.clear();
            return;
        };
        let _ = super::with_dock_on_main_thread(app, "cdp unsubscribe network", move |_app| {
            let _ = webview.with_webview(|platform| {
                use windows::core::Interface;
                let outcome: windows::core::Result<()> = (|| {
                    let controller = platform.controller();
                    let core = unsafe { controller.CoreWebView2() }?;
                    let key = core.as_raw() as usize;
                    let tokens = {
                        let mut log = net_log().lock();
                        log.subscribed.remove(&key);
                        log.event_tokens.remove(&key).unwrap_or_default()
                    };
                    for (event, token) in tokens {
                        let event_h = HSTRING::from(event.as_str());
                        if let Ok(receiver) =
                            unsafe { core.GetDevToolsProtocolEventReceiver(&event_h) }
                        {
                            let _ = unsafe {
                                receiver.remove_DevToolsProtocolEventReceived(token)
                            };
                        }
                    }
                    Ok(())
                })();
                if let Err(err) = outcome {
                    tracing::warn!("[browser_dock] cdp network unsubscribe failed: {err}");
                }
            });
            Ok(())
        });
    }

    fn record_net_event(event: &str, body: &str) {
        let Ok(v) = serde_json::from_str::<Value>(body) else {
            return;
        };
        let Some(request_id) = v
            .get("requestId")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
        else {
            return;
        };
        let mut log = net_log().lock();
        let Some(tab_id) = log.active_capture else {
            return;
        };
        let bucket = log.bucket_mut(tab_id);
        if !bucket.capturing {
            return;
        }
        match event {
            "Network.requestWillBeSent" => {
                let url = v
                    .pointer("/request/url")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if url.is_empty() || url.starts_with("data:") {
                    return;
                }
                let method = v
                    .pointer("/request/method")
                    .and_then(|x| x.as_str())
                    .unwrap_or("GET")
                    .to_string();
                let resource_type = v
                    .get("type")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string());
                if !bucket.map.contains_key(&request_id) {
                    if bucket.order.len() >= NET_LOG_CAP {
                        let oldest = bucket.order.remove(0);
                        bucket.map.remove(&oldest);
                    }
                    bucket.order.push(request_id.clone());
                }
                bucket.map.insert(
                    request_id,
                    NetEntry {
                        url,
                        method,
                        resource_type,
                        status: None,
                        mime: None,
                        encoded_len: None,
                        error: None,
                        started_ms: super::now_millis(),
                        finished_ms: None,
                    },
                );
            }
            "Network.responseReceived" => {
                if let Some(e) = bucket.map.get_mut(&request_id) {
                    e.status = v.pointer("/response/status").and_then(|x| x.as_u64());
                    e.mime = v
                        .pointer("/response/mimeType")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string());
                }
            }
            "Network.loadingFinished" => {
                if let Some(e) = bucket.map.get_mut(&request_id) {
                    e.encoded_len =
                        v.get("encodedDataLength").and_then(|x| x.as_f64());
                    e.finished_ms = Some(super::now_millis());
                }
            }
            "Network.loadingFailed" => {
                if let Some(e) = bucket.map.get_mut(&request_id) {
                    e.error = v
                        .get("errorText")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string());
                    e.finished_ms = Some(super::now_millis());
                }
            }
            _ => {}
        }
    }
}

#[cfg(windows)]
async fn exec_cdp_emulate(
    webview: &tauri::Webview,
    args: &Value,
    timeout: Duration,
) -> Result<Value, anyhow::Error> {
    let mut applied: Vec<&str> = Vec::new();
    if args.get("reset").and_then(|v| v.as_bool()).unwrap_or(false) {
        cdp::call(
            webview,
            "Emulation.clearDeviceMetricsOverride",
            serde_json::json!({}),
            timeout,
        )
        .await?;
        let _ = cdp::call(webview, "Network.enable", serde_json::json!({}), timeout).await;
        cdp::call(
            webview,
            "Network.emulateNetworkConditions",
            serde_json::json!({
                "offline": false,
                "latency": 0,
                "downloadThroughput": -1.0,
                "uploadThroughput": -1.0,
            }),
            timeout,
        )
        .await?;
        cdp::call(
            webview,
            "Emulation.setCPUThrottlingRate",
            serde_json::json!({"rate": 1}),
            timeout,
        )
        .await?;
        applied.push("reset");
        return Ok(serde_json::json!({"applied": applied}));
    }
    if let Some(vp) = args.get("viewport").filter(|v| v.is_object()) {
        let width = vp.get("width").and_then(|v| v.as_u64()).unwrap_or(375).clamp(240, 7680);
        let height = vp.get("height").and_then(|v| v.as_u64()).unwrap_or(812).clamp(320, 4320);
        let mobile = vp
            .get("mobile")
            .and_then(|v| v.as_bool())
            .unwrap_or(width <= 500);
        let scale = vp
            .get("device_scale_factor")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        cdp::call(
            webview,
            "Emulation.setDeviceMetricsOverride",
            serde_json::json!({
                "width": width,
                "height": height,
                "deviceScaleFactor": scale,
                "mobile": mobile,
            }),
            timeout,
        )
        .await?;
        applied.push("viewport");
    }
    if let Some(net) = args.get("network").and_then(|v| v.as_str()) {
        let conditions = match net {
            "offline" => serde_json::json!({
                "offline": true, "latency": 0,
                "downloadThroughput": 0.0, "uploadThroughput": 0.0,
            }),
            "slow-3g" => serde_json::json!({
                "offline": false, "latency": 400,
                "downloadThroughput": 50_000.0, "uploadThroughput": 25_000.0,
            }),
            "fast-3g" => serde_json::json!({
                "offline": false, "latency": 150,
                "downloadThroughput": 180_000.0, "uploadThroughput": 84_000.0,
            }),
            "none" => serde_json::json!({
                "offline": false, "latency": 0,
                "downloadThroughput": -1.0, "uploadThroughput": -1.0,
            }),
            other => {
                return Err(anyhow::anyhow!(
                    "unknown network preset: {other} (use offline | slow-3g | fast-3g | none)"
                ));
            }
        };
        cdp::call(webview, "Network.enable", serde_json::json!({}), timeout).await?;
        cdp::call(
            webview,
            "Network.emulateNetworkConditions",
            conditions,
            timeout,
        )
        .await?;
        applied.push("network");
    }
    if let Some(rate) = args.get("cpu_rate").and_then(|v| v.as_f64()) {
        cdp::call(
            webview,
            "Emulation.setCPUThrottlingRate",
            serde_json::json!({"rate": rate.clamp(1.0, 20.0)}),
            timeout,
        )
        .await?;
        applied.push("cpu");
    }
    if applied.is_empty() {
        return Err(anyhow::anyhow!(
            "emulate requires at least one of: viewport / network / cpu_rate / reset"
        ));
    }
    Ok(serde_json::json!({"applied": applied}))
}

#[cfg(windows)]
async fn exec_cdp_network_capture(
    webview: &tauri::Webview,
    tab_id: TabId,
    args: &Value,
    timeout: Duration,
) -> Result<Value, anyhow::Error> {
    let mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("dump");
    match mode {
        "start" => {
            cdp::call(
                webview,
                "Network.enable",
                serde_json::json!({
                    "maxResourceBufferSize": 16_000_000,
                    "maxTotalBufferSize": 64_000_000,
                }),
                timeout,
            )
            .await?;
            cdp::subscribe_network(webview)?;
            let mut log = cdp::net_log().lock();
            if let Some(previous) = log.active_capture {
                if previous != tab_id {
                    tracing::warn!(
                        previous_tab = previous,
                        new_tab = tab_id,
                        "[browser_dock] network capture is single-instance; superseding the \
                         capture on the previous tab (only one tab can capture at a time)"
                    );
                    if let Some(bucket) = log.tabs.get_mut(&previous) {
                        bucket.capturing = false;
                    }
                }
            }
            {
                let bucket = log.bucket_mut(tab_id);
                bucket.order.clear();
                bucket.map.clear();
                bucket.capturing = true;
            }
            log.active_capture = Some(tab_id);
            Ok(serde_json::json!({"capturing": true, "tab_id": tab_id}))
        }
        "stop" => {
            let captured = {
                let mut log = cdp::net_log().lock();
                let captured = log
                    .tabs
                    .get_mut(&tab_id)
                    .map(|b| {
                        b.capturing = false;
                        b.order.len()
                    })
                    .unwrap_or(0);
                if log.active_capture == Some(tab_id) {
                    log.active_capture = None;
                }
                captured
            };
            Ok(serde_json::json!({"capturing": false, "captured": captured, "tab_id": tab_id}))
        }
        "clear" => {
            let mut log = cdp::net_log().lock();
            if let Some(bucket) = log.tabs.get_mut(&tab_id) {
                bucket.order.clear();
                bucket.map.clear();
            }
            Ok(serde_json::json!({"cleared": true, "tab_id": tab_id}))
        }
        "body" => {
            let request_id = args
                .get("request_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("mode=body requires request_id"))?;
            let resp = cdp::call(
                webview,
                "Network.getResponseBody",
                serde_json::json!({"requestId": request_id}),
                timeout,
            )
            .await?;
            let base64_encoded = resp
                .get("base64Encoded")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let mut body = resp
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let full_len = body.len();
            if body.len() > 16_000 {
                let mut cut = 16_000;
                while !body.is_char_boundary(cut) {
                    cut -= 1;
                }
                body.truncate(cut);
            }
            Ok(serde_json::json!({
                "request_id": request_id,
                "base64_encoded": base64_encoded,
                "total_bytes": full_len,
                "truncated": full_len > body.len(),
                "body": body,
            }))
        }
        "dump" => {
            let limit = args
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(120)
                .min(400) as usize;
            let url_filter = args
                .get("url_contains")
                .and_then(|v| v.as_str())
                .map(|s| s.to_lowercase());
            let only_failures = args
                .get("only_failures")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let api_only = args
                .get("api_only")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let log = cdp::net_log().lock();
            let mut total = 0usize;
            let mut failed = 0usize;
            let mut items = Vec::new();
            let bucket = log.tabs.get(&tab_id);
            let capturing = bucket.map(|b| b.capturing).unwrap_or(false);
            let (order, map) = match bucket {
                Some(b) => (&b.order, &b.map),
                None => {
                    return Ok(serde_json::json!({
                        "capturing": false,
                        "total": 0,
                        "failed": 0,
                        "returned": 0,
                        "requests": [],
                        "tab_id": tab_id,
                    }));
                }
            };
            for id in order.iter() {
                let Some(e) = map.get(id) else { continue };
                total += 1;
                let is_fail =
                    e.error.is_some() || e.status.map(|s| s >= 400).unwrap_or(false);
                if is_fail {
                    failed += 1;
                }
                if let Some(f) = &url_filter {
                    if !e.url.to_lowercase().contains(f.as_str()) {
                        continue;
                    }
                }
                if only_failures && !is_fail {
                    continue;
                }
                if api_only {
                    let rt = e.resource_type.as_deref().unwrap_or("");
                    if rt != "XHR" && rt != "Fetch" {
                        continue;
                    }
                }
                if items.len() >= limit {
                    continue;
                }
                let mut url = e.url.clone();
                if url.len() > 300 {
                    let mut cut = 300;
                    while !url.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    url.truncate(cut);
                }
                items.push(serde_json::json!({
                    "request_id": id,
                    "url": url,
                    "method": e.method,
                    "type": e.resource_type,
                    "status": e.status,
                    "mime": e.mime,
                    "encoded_bytes": e.encoded_len,
                    "error": e.error,
                    "duration_ms": e.finished_ms.map(|f| f.saturating_sub(e.started_ms)),
                }));
            }
            Ok(serde_json::json!({
                "capturing": capturing,
                "total": total,
                "failed": failed,
                "returned": items.len(),
                "requests": items,
                "tab_id": tab_id,
            }))
        }
        other => Err(anyhow::anyhow!(
            "unknown network_capture mode: {other} (use start | stop | dump | body | clear)"
        )),
    }
}

#[tauri::command]
pub async fn browser_dock_open(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    rect: DockRect,
    url: Option<String>,
    session_id: Option<String>,
) -> Result<(), String> {
    state.set_rect(rect);
    state.set_parked(false);
    let s = state.inner().clone();

    let target = url
        .as_ref()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty());

    let session_normalized = canonical_dock_session_id_opt(session_id.as_deref());

    if let Some(ref sid) = session_normalized {
        s.set_foreground_session_id(Some(sid.clone()));
    }

    if let Some(target_url) = target.as_deref() {
        if let Some(existing) = s.find_owner_tab_with_url(
            TabOwner::User,
            target_url,
            session_normalized.as_deref(),
        ) {
            let _ = s.set_active(existing);
            ensure_dock_webview(&app, &s)?;
            dock_navigate_active(&app, &s)?;
            update_dock_layout(&app, &s)?;
            focus_dock_webview(&app);
            emit_tabs_event(&app, &s);
            return Ok(());
        }
    }

    let (active, _created) =
        s.acquire_or_create_user_tab(target.clone(), session_normalized.as_deref());
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
    let state_for_layout = state.inner().clone();
    let app_for_layout = app.clone();
    app.run_on_main_thread(move || {
        if let Err(err) = update_dock_layout(&app_for_layout, &state_for_layout) {
            tracing::warn!("[browser_dock] set_rect dock layout failed: {err}");
        }
    })
    .map_err(|e| format!("schedule dock layout on the main thread failed: {e}"))?;
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
    if let Err(err) = update_dock_layout(&app, state.inner()) {
        tracing::warn!("[browser_dock] park update_dock_layout failed: {err}");
    }
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_close(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    #[cfg(windows)]
    cdp::unsubscribe_network(&app);
    let _ = with_dock_on_main_thread(&app, "dock close webview", |app| {
        if let Some(webview) = dock_webview(app) {
            let _ = webview.close();
        }
        Ok(())
    });
    #[cfg(windows)]
    {
        let mut log = cdp::net_log().lock();
        log.tabs.clear();
        log.subscribed.clear();
        log.event_tokens.clear();
        log.active_capture = None;
    }
    state.reset();
    if let Some(controller) = app.try_state::<TauriDockController>() {
        controller.drain_pending("dock closed");
    }
    emit_tabs_event(&app, state.inner());
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_release_agent_tab_for_session(
    state: tauri::State<'_, DockSharedState>,
    session_id: String,
) -> Result<Vec<TabId>, String> {
    let released = state.release_agent_tabs_for_session(&session_id);
    Ok(released)
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
        let scope = state.tab_session_of(id);
        if let Some(existing) = state.find_owner_tab_with_url(
            TabOwner::User,
            trimmed,
            scope.as_deref(),
        ) {
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
    session_id: Option<String>,
) -> Result<TabId, String> {
    let id = state.alloc_id();
    let explicit_session = session_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| state.foreground_session_id());
    state.register_tab_for_session(id, url.clone(), TabOwner::User, explicit_session.as_deref());
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

fn open_url_in_new_tab(
    app: &AppHandle,
    url: String,
    opener_tab_id: Option<TabId>,
) -> Result<TabId, String> {
    let state_handle = app
        .try_state::<DockSharedState>()
        .ok_or_else(|| "browser dock state not initialised".to_string())?;
    let state = state_handle.inner();
    let opener_session = opener_tab_id.and_then(|opener| state.tab_session_of(opener));
    let inherit_as_agent = opener_session.is_some();
    let trimmed = url.trim();
    if !trimmed.is_empty() {
        let lookup_owner = if inherit_as_agent {
            TabOwner::Agent
        } else {
            TabOwner::User
        };
        if let Some(existing) = state.find_owner_tab_with_url(
            lookup_owner,
            trimmed,
            opener_session.as_deref(),
        ) {
            let _ = state.set_active(existing);
            ensure_dock_webview(app, state)?;
            dock_navigate_active(app, state)?;
            let _ = update_dock_layout(app, state);
            emit_tabs_event(app, state);
            return Ok(existing);
        }
    }
    let id = state.alloc_id();
    let owner = if inherit_as_agent {
        TabOwner::Agent
    } else {
        TabOwner::User
    };
    state.register_tab_for_session(id, Some(url), owner, opener_session.as_deref());
    let _ = state.set_active(id);
    ensure_dock_webview(app, state)?;
    dock_navigate_active(app, state)?;
    let _ = update_dock_layout(app, state);
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
        let _ = with_dock_on_main_thread(&app, "dock hide", |app| {
            if let Some(wv) = dock_webview(app) {
                let _ = wv.hide();
            }
            Ok(())
        });
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
    session_id: Option<String>,
) -> Result<(), String> {
    if let Some(sid) = canonical_dock_session_id_opt(session_id.as_deref()) {
        match state.tab_session_of(tab_id).as_deref() {
            Some(tab_session) if session_ids_equivalent(tab_session, &sid) => {
                state.set_foreground_session_id(Some(sid));
            }
            Some(other) => {
                return Err(format!(
                    "tab {tab_id} belongs to session {other}, not {sid}"
                ));
            }
            None => {
                return Err(format!("tab {tab_id} is not bound to any session"));
            }
        }
    }
    if let Some(prev) = state.active() {
        if prev != tab_id {
            if let Some(controller) = app.try_state::<TauriDockController>() {
                controller.drain_pending_for_tab(
                    prev,
                    "dock tab was switched while a browser action was in flight; retry the action",
                );
            }
        }
    }
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
    session_id: Option<String>,
) -> Result<Vec<TabSummary>, String> {
    let all = state.list();
    let Some(sid) = canonical_dock_session_id_opt(session_id.as_deref()) else {
        return Ok(all);
    };
    Ok(all
        .into_iter()
        .filter(|t| {
            t.session_id
                .as_deref()
                .is_some_and(|tab_sid| session_ids_equivalent(tab_sid, &sid))
        })
        .collect())
}

fn mirror_test_target_to_gateway(app: &AppHandle, session_id: &str, tab_id: Option<TabId>) {
    let Some(url) = crate::current_gateway_url(app) else {
        return;
    };
    let payload = serde_json::json!({ "sessionId": session_id, "tabId": tab_id });
    tauri::async_runtime::spawn(async move {
        static MIRROR_FAILURES: AtomicU64 = AtomicU64::new(0);
        match crate::adapters_restart_client()
            .post(format!("{url}/api/debug/test-target"))
            .header(
                senweavercoding::gateway::loopback_auth::TOKEN_HEADER,
                senweavercoding::gateway::loopback_auth::loopback_token(),
            )
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    crate::warn_emit_failure(
                        &MIRROR_FAILURES,
                        "browser_dock test-target mirror",
                        &format!("gateway returned HTTP {}", resp.status().as_u16()),
                    );
                }
            }
            Err(err) => {
                crate::warn_emit_failure(
                    &MIRROR_FAILURES,
                    "browser_dock test-target mirror",
                    &err,
                );
            }
        }
    });
}

#[tauri::command]
pub async fn browser_dock_pin_test_target(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    session_id: String,
    tab_id: TabId,
) -> Result<(), String> {
    let sid = canonical_dock_session_id(&session_id);
    if sid.is_empty() {
        return Err("session_id is required".to_string());
    }
    match state.tab_session_of(tab_id).as_deref() {
        Some(tab_session) if session_ids_equivalent(tab_session, &sid) => {}
        Some(other) => {
            return Err(format!(
                "tab {tab_id} belongs to session {other}, not {sid}"
            ));
        }
        None => {
            return Err(format!("tab {tab_id} is not bound to any session"));
        }
    }
    set_test_target_tab(&sid, tab_id);
    mirror_test_target_to_gateway(&app, &sid, Some(tab_id));
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_clear_test_target(
    app: AppHandle,
    session_id: String,
) -> Result<(), String> {
    let sid = canonical_dock_session_id(&session_id);
    clear_test_target_tab(&sid);
    mirror_test_target_to_gateway(&app, &sid, None);
    Ok(())
}

#[tauri::command]
pub async fn browser_dock_get_test_target(
    session_id: String,
) -> Result<Option<TabId>, String> {
    let sid = canonical_dock_session_id(&session_id);
    Ok(current_test_target_tab(&sid))
}

#[tauri::command]
pub async fn browser_dock_present_session(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    session_id: String,
) -> Result<Option<TabId>, String> {
    let inner = state.inner();
    let trimmed = canonical_dock_session_id(&session_id);
    if trimmed.is_empty() {
        inner.set_foreground_session_id(None);
        inner.set_parked(true);
        update_dock_layout(&app, inner)?;
        emit_tabs_event(&app, inner);
        return Ok(None);
    }
    inner.set_foreground_session_id(Some(trimmed.clone()));
    let target = inner.present_session_internal(&trimmed);
    if let Some(tab_id) = target {
        let _ = inner.set_active(tab_id);
        if let Err(err) = ensure_dock_webview(&app, inner) {
            inner.set_parked(true);
            let _ = update_dock_layout(&app, inner);
            emit_tabs_event(&app, inner);
            return Err(err);
        }
        inner.set_parked(false);
        if let Err(err) = dock_navigate_active(&app, inner) {
            inner.set_parked(true);
            let _ = update_dock_layout(&app, inner);
            emit_tabs_event(&app, inner);
            return Err(err);
        }
        if let Err(err) = update_dock_layout(&app, inner) {
            tracing::warn!("[browser_dock] present_session update_dock_layout failed: {err}");
        }
        focus_dock_webview(&app);
        emit_tabs_event(&app, inner);
    } else {
        {
            let mut g = inner.0.lock();
            if let Some(active) = g.active {
                let foreign = g
                    .tab_session
                    .get(&active)
                    .is_some_and(|sid| !session_ids_equivalent(sid, &trimmed));
                if foreign {
                    g.active = None;
                }
            }
        }
        let _ = with_dock_on_main_thread(&app, "dock navigate blank", |app| {
            if let Some(wv) = dock_webview(app) {
                if let Ok(parsed) = parse_target_url(None) {
                    let _ = wv.navigate(parsed);
                }
            }
            Ok(())
        });
        inner.set_parked(true);
        update_dock_layout(&app, inner)?;
        emit_tabs_event(&app, inner);
    }
    Ok(target)
}

#[tauri::command]
pub async fn browser_dock_set_foreground_session(
    app: AppHandle,
    state: tauri::State<'_, DockSharedState>,
    session_id: Option<String>,
) -> Result<(), String> {
    let inner = state.inner();
    let normalized = canonical_dock_session_id_opt(session_id.as_deref());
    inner.set_foreground_session_id(normalized.clone());
    if normalized.is_none() {
        inner.set_parked(true);
    }
    update_dock_layout(&app, inner)?;
    emit_tabs_event(&app, inner);
    Ok(())
}

fn eval_dock(app: &AppHandle, _state: &DockSharedState, source: &str) -> Result<(), String> {
    let source = source.to_string();
    with_dock_on_main_thread(app, "dock eval", move |app| {
        let webview = dock_webview(app)
            .ok_or_else(|| "dock webview is not open".to_string())?;
        webview
            .eval(&source)
            .map_err(|e| format!("eval failed: {e}"))?;
        Ok(())
    })
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
    if full && dock_webview(&app).is_some() {
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
        let _ = with_dock_on_main_thread(&app, "dock screenshot warmup", move |app| {
            if let Some(webview) = dock_webview(app) {
                let _ = webview.eval(warmup);
            }
            Ok(())
        });
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
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
    let _ = dock_webview(&app)
        .ok_or_else(|| "dock webview is not open".to_string())?;
    #[cfg(debug_assertions)]
    {
        with_dock_on_main_thread(&app, "dock open devtools", |app| {
            if let Some(webview) = dock_webview(app) {
                webview.open_devtools();
            }
            Ok(())
        })?;
        return Ok(());
    }
    #[cfg(not(debug_assertions))]
    {
        Err("DevTools are only available in debug builds".to_string())
    }
}

#[tauri::command]
pub async fn browser_dock_close_devtools(
    app: AppHandle,
    _state: tauri::State<'_, DockSharedState>,
) -> Result<(), String> {
    let _ = dock_webview(&app)
        .ok_or_else(|| "dock webview is not open".to_string())?;
    #[cfg(debug_assertions)]
    {
        with_dock_on_main_thread(&app, "dock close devtools", |app| {
            if let Some(webview) = dock_webview(app) {
                webview.close_devtools();
            }
            Ok(())
        })?;
        return Ok(());
    }
    #[cfg(not(debug_assertions))]
    {
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
                    let session_id = inner_for_task
                        .app
                        .try_state::<DockSharedState>()
                        .and_then(|s| s.tab_session_of(tab_id));
                    let payload = serde_json::json!({
                        "kind": "dock_takeover_end",
                        "tabId": tab_id,
                        "sessionId": session_id,
                        "data": {
                            "tab_id": tab_id,
                            "ended_at": now_millis(),
                            "sessionId": session_id,
                        },
                    });
                    static TAKEOVER_END_EMIT_FAILURES: AtomicU64 = AtomicU64::new(0);
                    if let Err(err) =
                        inner_for_task.app.emit_to("main", "browser_dock_event", payload)
                    {
                        crate::warn_emit_failure(
                            &TAKEOVER_END_EMIT_FAILURES,
                            "browser_dock takeover-end",
                            &err,
                        );
                    }
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
            let session_id = self
                .0
                .app
                .try_state::<DockSharedState>()
                .and_then(|s| s.tab_session_of(tab_id));
            let payload = serde_json::json!({
                "kind": "dock_takeover",
                "tabId": tab_id,
                "sessionId": session_id,
                "data": {
                    "tab_id": tab_id,
                    "started_at": started_at,
                    "sessionId": session_id,
                },
            });
            static TAKEOVER_START_EMIT_FAILURES: AtomicU64 = AtomicU64::new(0);
            if let Err(err) = self.0.app.emit_to("main", "browser_dock_event", payload) {
                crate::warn_emit_failure(
                    &TAKEOVER_START_EMIT_FAILURES,
                    "browser_dock takeover-start",
                    &err,
                );
            }
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

    fn prune_nav_waiters(&self, tab_id: TabId) {
        let mut g = self.0.nav_waiters.lock();
        if let Some(waiters) = g.get_mut(&tab_id) {
            waiters.retain(|tx| !tx.is_closed());
            if waiters.is_empty() {
                g.remove(&tab_id);
            }
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
                self.prune_nav_waiters(tab_id);
                Err(anyhow::anyhow!("nav ready channel closed"))
            }
            Err(_) => {
                self.prune_nav_waiters(tab_id);
                Err(anyhow::anyhow!(
                    "timed out waiting for dock navigation to complete on tab {tab_id}"
                ))
            }
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
    normalize_url_for_match(&canonicalize_loopback_url(a))
        == normalize_url_for_match(&canonicalize_loopback_url(b))
}

fn current_session_id() -> Option<String> {
    senweavercoding::session::current_session_context()
        .map(|c| canonical_dock_session_id(&c.session_id))
        .filter(|s| !s.is_empty())
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
    if let Some(sid) = current_session_id() {
        if let Some(tab_id) = state.agent_tab_id_for_session(&sid) {
            return tab_id;
        }
        let (id, _) = state.acquire_or_create_agent_tab_for_session(&sid, None);
        return id;
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
        let session_hint_norm = canonical_dock_session_id_opt(session_hint.as_deref());
        let (agent_tab, created) = match session_hint_norm.as_deref() {
            Some(sid) => state.acquire_or_create_agent_tab_for_session(sid, None),
            None => state.acquire_or_create_agent_tab(None),
        };
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
            "tabId": agent_tab,
            "sessionId": session_hint,
            "data": { "session": session_hint, "source": "agent", "agentTabId": agent_tab },
        });
        if let Err(err) = self.0.app.emit_to("main", "browser_dock_event", payload) {
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
            if let Some(expected) = state.snapshot_tab(tab_id).0 {
                state.record_state_url(tab_id, &expected);
            }
        }

        let preview_id = self.0.next_req_id.load(Ordering::SeqCst);
        let preview_session = state.tab_session_of(tab_id);
        static EXEC_PREVIEW_EMIT_FAILURES: AtomicU64 = AtomicU64::new(0);
        if let Err(err) = self.0.app.emit_to(
            "main",
            "browser_dock_event",
            serde_json::json!({
                "kind": "agent_action",
                "tabId": tab_id,
                "sessionId": preview_session,
                "data": {
                    "reqId": preview_id,
                    "kind": req.kind,
                    "args": req.args,
                    "tabId": tab_id,
                    "sessionId": preview_session,
                    "ts": now_millis(),
                },
            }),
        ) {
            crate::warn_emit_failure(
                &EXEC_PREVIEW_EMIT_FAILURES,
                "browser_dock exec agent_action",
                &err,
            );
        }

        if matches!(req.kind.as_str(), "emulate" | "network_capture") {
            return self.exec_cdp_kind(tab_id, &req).await;
        }

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
            if let Some(expected) = state.snapshot_tab(tab_id).0 {
                state.record_state_url(tab_id, &expected);
            }
        }

        if full_page && dock_webview(&self.0.app).is_some() {
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
            let _ = with_dock_on_main_thread(&self.0.app, "dock screenshot warmup", move |app| {
                if let Some(webview) = dock_webview(app) {
                    let _ = webview.eval(warmup);
                }
                Ok(())
            });
            tokio::time::sleep(Duration::from_millis(120)).await;
        }

        #[cfg(windows)]
        {
            if let Some(webview) = dock_webview(&self.0.app) {
                match cdp::call(
                    &webview,
                    "Page.captureScreenshot",
                    serde_json::json!({
                        "format": "png",
                        "captureBeyondViewport": full_page,
                    }),
                    Duration::from_millis(10_000),
                )
                .await
                {
                    Ok(v) => {
                        if let Some(data) = v.get("data").and_then(|d| d.as_str()) {
                            use base64::Engine;
                            if let Ok(bytes) =
                                base64::engine::general_purpose::STANDARD.decode(data)
                            {
                                if !bytes.is_empty() {
                                    return Ok(bytes);
                                }
                            }
                        }
                        tracing::warn!(
                            "[browser_dock] cdp screenshot returned no data, falling back"
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            "[browser_dock] cdp screenshot failed, falling back: {err}"
                        );
                    }
                }
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
            let _ = with_dock_on_main_thread(&self.0.app, "dock hide", |app| {
                if let Some(wv) = dock_webview(app) {
                    let _ = wv.hide();
                }
                Ok(())
            });
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
        if let Some(prev) = state.active() {
            if prev != tab_id {
                self.drain_pending_for_tab(
                    prev,
                    "dock tab was switched while a browser action was in flight; retry the action",
                );
            }
        }
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

    async fn bind_tab_to_session(
        &self,
        session_id: String,
        tab_id: u32,
    ) -> Result<()> {
        let session_id = canonical_dock_session_id(&session_id);
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        state
            .bind_user_tab_to_session(&session_id, tab_id)
            .map_err(|e| anyhow::anyhow!("bind_tab_to_session: {e}"))?;
        emit_tabs_event(&self.0.app, &state);
        Ok(())
    }

    async fn unbind_tab_from_session(
        &self,
        session_id: String,
        tab_id: u32,
    ) -> Result<()> {
        let session_id = canonical_dock_session_id(&session_id);
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        state
            .unbind_tab_from_session(&session_id, tab_id)
            .map_err(|e| anyhow::anyhow!("unbind_tab_from_session: {e}"))?;
        emit_tabs_event(&self.0.app, &state);
        Ok(())
    }

    async fn release_agent_tabs_for_session(
        &self,
        session_id: String,
    ) -> Result<Vec<u32>> {
        let session_id = canonical_dock_session_id(&session_id);
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        let released = state.release_agent_tabs_for_session(&session_id);
        if !released.is_empty() {
            emit_tabs_event(&self.0.app, &state);
        }
        Ok(released)
    }

    async fn present_session(&self, session_id: String) -> Result<Option<u32>> {
        let session_id = canonical_dock_session_id(&session_id);
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        state.set_foreground_session_id(Some(session_id.clone()));
        let target = state.present_session_internal(&session_id);
        if let Some(tab_id) = target {
            state
                .set_active(tab_id)
                .map_err(|e| anyhow::anyhow!("set_active: {e}"))?;
            ensure_dock_webview(&self.0.app, &state)
                .map_err(|e| anyhow::anyhow!("ensure_dock_webview: {e}"))?;
            state.set_parked(false);
            let _ = dock_navigate_active(&self.0.app, &state);
            let _ = update_dock_layout(&self.0.app, &state);
            emit_tabs_event(&self.0.app, &state);
        } else {
            {
                let mut g = state.0.lock();
                if let Some(active) = g.active {
                    let foreign = g
                        .tab_session
                        .get(&active)
                        .is_some_and(|sid| !session_ids_equivalent(sid, &session_id));
                    if foreign {
                        g.active = None;
                    }
                }
            }
            let _ = with_dock_on_main_thread(&self.0.app, "dock navigate blank", |app| {
                if let Some(wv) = dock_webview(app) {
                    if let Ok(parsed) = parse_target_url(None) {
                        let _ = wv.navigate(parsed);
                    }
                }
                Ok(())
            });
            state.set_parked(true);
            let _ = update_dock_layout(&self.0.app, &state);
            emit_tabs_event(&self.0.app, &state);
        }
        Ok(target)
    }

    async fn park(&self) -> Result<()> {
        let state = self
            .0
            .app
            .try_state::<DockSharedState>()
            .map(|s| s.inner().clone())
            .ok_or_else(|| anyhow::anyhow!("dock state not initialised"))?;
        state.set_parked(true);
        if let Err(err) = update_dock_layout(&self.0.app, &state) {
            tracing::warn!("[browser_dock] park update_dock_layout failed: {err}");
        }
        emit_tabs_event(&self.0.app, &state);
        Ok(())
    }
}

impl TauriDockController {
    async fn exec_cdp_kind(&self, tab_id: TabId, req: &DockRequest) -> Result<DockResponse> {
        #[cfg(windows)]
        {
            let webview = dock_webview(&self.0.app)
                .ok_or_else(|| anyhow::anyhow!("dock webview is not open"))?;
            let timeout = Duration::from_millis(req.timeout_ms.clamp(2_000, 30_000));
            let value = match req.kind.as_str() {
                "emulate" => exec_cdp_emulate(&webview, &req.args, timeout).await?,
                "network_capture" => {
                    exec_cdp_network_capture(&webview, tab_id, &req.args, timeout).await?
                }
                other => return Err(anyhow::anyhow!("unsupported cdp kind: {other}")),
            };
            Ok(DockResponse {
                ok: true,
                value,
                error: None,
            })
        }
        #[cfg(not(windows))]
        {
            let _ = tab_id;
            Err(anyhow::anyhow!(
                "browser action '{}' requires the Windows WebView2 dock (CDP); use snapshot / get_styles / network_errors instead",
                req.kind
            ))
        }
    }

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
        let scope = state.tab_session_of(tab_id);
        if let Some(existing) = state.find_owner_tab_with_url(
            owner,
            &normalized,
            scope.as_deref(),
        ) {
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
        let preview_session = state.tab_session_of(tab_id);
        static NAVIGATE_PREVIEW_EMIT_FAILURES: AtomicU64 = AtomicU64::new(0);
        if let Err(err) = self.0.app.emit_to(
            "main",
            "browser_dock_event",
            serde_json::json!({
                "kind": "agent_action",
                "tabId": tab_id,
                "sessionId": preview_session,
                "data": {
                    "reqId": preview_id,
                    "kind": "navigate",
                    "args": { "url": normalized },
                    "tabId": tab_id,
                    "sessionId": preview_session,
                    "ts": now_millis(),
                },
            }),
        ) {
            crate::warn_emit_failure(
                &NAVIGATE_PREVIEW_EMIT_FAILURES,
                "browser_dock navigate agent_action",
                &err,
            );
        }

        let _ = dock_webview(&self.0.app)
            .ok_or_else(|| anyhow::anyhow!("dock webview is not open"))?;
        with_dock_on_main_thread(&self.0.app, "dock navigate", move |app| {
            let webview = dock_webview(app)
                .ok_or_else(|| "dock webview is not open".to_string())?;
            webview
                .navigate(parsed)
                .map_err(|err| format!("webview navigate failed: {err}"))?;
            Ok(())
        })
        .map_err(|e| anyhow::anyhow!(e))?;

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
        let _ = dock_webview(&self.0.app)
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
        let no_bridge_error = serde_json::json!({
            "reqId": req_id,
            "ok": false,
            "value": Value::Null,
            "error": "dock bridge unavailable: the page has not finished loading, failed to load, or blocks injected scripts",
        });
        let no_bridge_json = serde_json::to_string(&no_bridge_error)
            .with_context(|| "serialise dock no-bridge payload")?;
        let source = format!(
            r#"(() => {{
  const payload = {payload_js};
  if (window.__senDockBridge) {{ window.__senDockBridge.exec(payload); return; }}
  try {{
    const base = window.__SEN_BRIDGE_BASE || {bridge_base:?};
    const params = new URLSearchParams();
    params.set('kind', 'result');
    params.set('data', {no_bridge_json:?});
    fetch(base + '/event?' + params.toString(), {{
      method: 'GET',
      mode: 'no-cors',
      cache: 'no-store',
      credentials: 'omit',
      keepalive: true,
    }}).catch(() => {{}});
  }} catch (_) {{}}
}})();"#,
            bridge_base = bridge_base_url(),
        );
        if let Err(err) = with_dock_on_main_thread(&self.0.app, "dock exec eval", move |app| {
            let webview = dock_webview(app)
                .ok_or_else(|| "dock webview is not open".to_string())?;
            webview
                .eval(&source)
                .map_err(|e| format!("{e}"))?;
            Ok(())
        }) {
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
                let diag = self
                    .0
                    .app
                    .try_state::<DockSharedState>()
                    .map(|state| {
                        let (tab_url, _) = state.snapshot_tab(tab_id);
                        let seen = state.last_state_url(tab_id);
                        format!(
                            " tab_url={} bridge_last_seen_url={}",
                            tab_url.unwrap_or_else(|| "(none)".into()),
                            seen.unwrap_or_else(|| "(never)".into()),
                        )
                    })
                    .unwrap_or_default();
                Err(anyhow::anyhow!(
                    "dock bridge timed out waiting for kind={} (tab={});{} the page may still be loading, may have failed to load, or its content security policy blocks bridge callbacks",
                    req.kind,
                    tab_id,
                    diag
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

    let current_pid = std::process::id();
    let xcap_windows = xcap::Window::all()
        .map_err(|err| anyhow::anyhow!("xcap::Window::all failed: {err}"))?;
    let mut own_windows: Vec<xcap::Window> = xcap_windows
        .into_iter()
        .filter(|w| w.pid().map(|p| p == current_pid).unwrap_or(false))
        .collect();
    let target = if let Some(idx) = own_windows.iter().position(|w| {
        w.title().map(|t| t == main_title).unwrap_or(false)
    }) {
        own_windows.swap_remove(idx)
    } else if let Some(first) = own_windows.into_iter().next() {
        first
    } else {
        return Err(anyhow::anyhow!("could not locate main window via xcap"));
    };
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
    record_dock_main_thread();
    let controller = TauriDockController::new(app.clone());
    app.manage(controller.clone());
    senweavercoding::tools::browser::install_dock_controller(Arc::new(controller));

    install_main_window_dock_layout_events(app);
}

pub(crate) fn install_main_window_dock_layout_events(app: &AppHandle) {
    let Some(window) = app.get_window("main") else {
        return;
    };
    let app_for_resize = app.clone();
    let resize_scheduled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::ScaleFactorChanged { .. } => {
            if let Some(state) = app_for_resize.try_state::<DockSharedState>() {
                state.set_last_applied_dock_geometry(None);
                let _ = update_dock_layout(&app_for_resize, state.inner());
            }
        }
        tauri::WindowEvent::Resized(_) => {
            if resize_scheduled
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                let app_throttled = app_for_resize.clone();
                let scheduled = resize_scheduled.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(60)).await;
                    let app_main = app_throttled.clone();
                    let _ = app_throttled.run_on_main_thread(move || {
                        if let Some(state) = app_main.try_state::<DockSharedState>() {
                            let _ = update_dock_layout(&app_main, state.inner());
                        }
                    });
                    scheduled.store(false, Ordering::Release);
                });
            }
        }
        _ => {}
    });
}
