import './theme/globals.css'

function paintBootError(label: string, message: string, stack?: string) {
  const root = document.getElementById('root')
  if (!root) return
  if (root.dataset.bootErrorPainted === '1') return
  root.dataset.bootErrorPainted = '1'
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
}

window.addEventListener('error', (e) => {
  paintBootError(
    'window.error',
    e.message ?? String(e.error ?? 'Unknown error'),
    e.error instanceof Error ? e.error.stack : undefined,
  )
})
window.addEventListener('unhandledrejection', (e) => {
  const reason = e.reason
  paintBootError(
    'unhandledrejection',
    reason instanceof Error ? reason.message : String(reason ?? 'Unknown rejection'),
    reason instanceof Error ? reason.stack : undefined,
  )
})

async function boot() {
  try {
    const [{ default: React }, { default: ReactDOM }, appModule, uiModule, boundaryModule] =
      await Promise.all([
        import('react'),
        import('react-dom/client'),
        import('./App'),
        import('./stores/uiStore'),
        import('./components/layout/AppErrorBoundary'),
      ])
    uiModule.initializeTheme()
    const root = document.getElementById('root')
    if (!root) {
      paintBootError('missing-root', '#root element missing in index.html')
      return
    }
    ReactDOM.createRoot(root).render(
      React.createElement(
        React.StrictMode,
        null,
        React.createElement(
          boundaryModule.AppErrorBoundary,
          null,
          React.createElement(appModule.App),
        ),
      ),
    )
  } catch (err) {
    paintBootError(
      'module-load',
      err instanceof Error ? err.message : String(err),
      err instanceof Error ? err.stack : undefined,
    )
  }
}

void boot()
