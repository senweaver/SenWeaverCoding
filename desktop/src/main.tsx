// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import './theme/globals.css'
import 'katex/dist/katex.min.css'

let bootCompleted = false
let revealRequested = false

function currentWindowLabelSync(): string | null {
  try {
    const internals = (window as unknown as {
      __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: unknown } } }
    }).__TAURI_INTERNALS__
    const label = internals?.metadata?.currentWindow?.label
    return typeof label === 'string' ? label : null
  } catch {
    return null
  }
}

type MinimalWindowKind = 'minimal' | 'minimal-input' | null

function minimalWindowKindSync(): MinimalWindowKind {
  try {
    const hash = window.location.hash.replace(/^#/, '').split('?')[0]
    if (hash === 'minimal-input') return 'minimal-input'
    if (hash === 'minimal') return 'minimal'
  } catch {
  }
  const label = currentWindowLabelSync()
  if (label === 'minimal-input') return 'minimal-input'
  if (label === 'minimal') return 'minimal'
  try {
    if (/(?:^|[?&])minimal=1(?:&|$)/.test(window.location.search)) return 'minimal'
  } catch {
  }
  return null
}

function isMinimalContextSync(): boolean {
  return minimalWindowKindSync() !== null
}

async function showCurrentWindow() {
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    const win = getCurrentWindow()
    await win.show()
    await win.setFocus()
  } catch {
  }
}

function revealWindowNow() {
  if (revealRequested) return
  revealRequested = true
  void (async () => {
    if (isMinimalContextSync()) {
      await showCurrentWindow()
      return
    }
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('signal_frontend_ready')
    } catch {
      await showCurrentWindow()
    }
  })()
}

function paintBootError(label: string, message: string, stack?: string) {
  const root = document.getElementById('root')
  if (!root) return
  if (root.dataset.bootErrorPainted === '1') return
  root.dataset.bootErrorPainted = '1'
  try {
    ;(window as unknown as { __SEN_BOOT_ERROR__?: string }).__SEN_BOOT_ERROR__ =
      `${label}: ${message}${stack ? `\n${stack}` : ''}`
  } catch {
  }
  root.innerHTML = ''
  const wrap = document.createElement('div')
  wrap.style.cssText =
    'min-height:100vh;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:12px;padding:24px;font:13px -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,Arial,sans-serif;color:#1a1a1a;background:#fcfcfc;'
  const title = document.createElement('div')
  title.textContent = '应用启动失败 / Boot error: ' + label
  title.style.cssText = 'font-size:15px;font-weight:600;'
  const body = document.createElement('div')
  body.textContent = message
  body.style.cssText = 'max-width:720px;text-align:center;color:#444;'
  wrap.appendChild(title)
  wrap.appendChild(body)
  if (stack) {
    const pre = document.createElement('pre')
    pre.textContent = stack
    pre.style.cssText =
      'font-size:11px;max-height:260px;max-width:800px;width:100%;overflow:auto;background:#f4f4f4;border:1px solid #ddd;border-radius:6px;padding:12px;white-space:pre-wrap;word-break:break-all;'
    wrap.appendChild(pre)
  }
  const btn = document.createElement('button')
  btn.textContent = '重新加载 / Reload'
  btn.style.cssText =
    'padding:6px 14px;font-size:13px;border:1px solid #888;border-radius:6px;background:#fff;cursor:pointer;'
  btn.onclick = () => window.location.reload()
  wrap.appendChild(btn)
  root.appendChild(wrap)
  if (isMinimalContextSync()) {
    void (async () => {
      try {
        const { emit } = await import('@tauri-apps/api/event')
        await emit('minimal://error', `${label}: ${message}`)
      } catch {
      }
    })()
    return
  }
  revealWindowNow()
}

function reportRuntimeError(label: string, message: string, stack?: string) {
  if (typeof console !== 'undefined') {
    if (stack) {
      console.error(`[runtime:${label}]`, message, '\n', stack)
    } else {
      console.error(`[runtime:${label}]`, message)
    }
  }
  try {
    window.dispatchEvent(
      new CustomEvent('app:runtime-error', { detail: { label, message, stack } }),
    )
  } catch {
  }
  if (isMinimalContextSync()) {
    void (async () => {
      try {
        const { emit } = await import('@tauri-apps/api/event')
        await emit('minimal://error', `${label}: ${message}`)
      } catch {
      }
    })()
  }
}

window.addEventListener('error', (e) => {
  const message = e.message ?? String(e.error ?? 'Unknown error')
  const stack = e.error instanceof Error ? e.error.stack : undefined
  if (bootCompleted) {
    reportRuntimeError('window.error', message, stack)
    return
  }
  paintBootError('window.error', message, stack)
})

window.addEventListener('unhandledrejection', (e) => {
  const reason = e.reason
  const message = reason instanceof Error ? reason.message : String(reason ?? 'Unknown rejection')
  const stack = reason instanceof Error ? reason.stack : undefined
  if (bootCompleted) {
    e.preventDefault?.()
    reportRuntimeError('unhandledrejection', message, stack)
    return
  }
  paintBootError('unhandledrejection', message, stack)
})

function storedLocale(): 'en' | 'zh' {
  try {
    const stored = localStorage.getItem('sen-locale')
    if (stored === 'en' || stored === 'zh') return stored
  } catch {  }
  return 'zh'
}

async function detectMinimalWindowKind(): Promise<MinimalWindowKind> {
  const sync = minimalWindowKindSync()
  if (sync) return sync
  try {
    if (
      typeof window !== 'undefined' &&
      ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)
    ) {
      const { getCurrentWindow } = await import('@tauri-apps/api/window')
      const label = getCurrentWindow().label
      if (label === 'minimal-input') return 'minimal-input'
      if (label === 'minimal') return 'minimal'
    }
  } catch {
  }
  return null
}

async function boot() {
  try {
    const minimalKind = await detectMinimalWindowKind()
    const [{ default: React }, { default: ReactDOM }, rootModule, uiModule, boundaryModule, i18nModule] =
      await Promise.all([
        import('react'),
        import('react-dom/client'),
        minimalKind === 'minimal-input'
          ? import('./MinimalInputWindow')
          : minimalKind === 'minimal'
            ? import('./MinimalApp')
            : import('./App'),
        import('./stores/uiStore'),
        import('./components/layout/AppErrorBoundary'),
        import('./i18n'),
      ])
    await i18nModule.ensureLocaleLoaded(storedLocale())
    uiModule.initializeTheme()
    const root = document.getElementById('root')
    if (!root) {
      paintBootError('missing-root', '#root element missing in index.html')
      return
    }
    const RootComponent =
      minimalKind === 'minimal-input'
        ? (rootModule as typeof import('./MinimalInputWindow')).MinimalInputWindow
        : minimalKind === 'minimal'
          ? (rootModule as typeof import('./MinimalApp')).MinimalApp
          : (rootModule as typeof import('./App')).App
    ReactDOM.createRoot(root).render(
      React.createElement(
        React.StrictMode,
        null,
        React.createElement(
          boundaryModule.AppErrorBoundary,
          null,
          React.createElement(RootComponent),
        ),
      ),
    )
    bootCompleted = true
    if (!minimalKind) {
      setTimeout(() => revealWindowNow(), 0)
    }
  } catch (err) {
    paintBootError(
      'module-load',
      err instanceof Error ? err.message : String(err),
      err instanceof Error ? err.stack : undefined,
    )
  }
}

void boot()
