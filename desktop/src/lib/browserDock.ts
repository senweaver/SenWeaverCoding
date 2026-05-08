// SPDX-License-Identifier: MIT
//
// Thin TypeScript wrapper around the Tauri `browser_dock_*` commands
// declared in `desktop/src-tauri/src/browser_dock.rs`.  The
// `EmbeddedBrowserPanel` and `browserPanelStore` go through this
// surface so the panel renders cleanly in the Vite browser dev mode
// (no Tauri IPC available — every call resolves to a no-op).

import { isTauriRuntime } from './desktopRuntime'

export type BrowserDockRect = { x: number; y: number; w: number; h: number }

export type BrowserDockTabInfo = {
  id: number
  url: string | null
  title: string | null
  active: boolean
}

export type BrowserDockEvent =
  | { kind: 'state'; tabId?: number; data: { url: string; title: string; canBack: boolean; ts: number } }
  | { kind: 'console'; tabId?: number; data: { level: string; message: string; ts: number } }
  | { kind: 'pick'; tabId?: number; data: { selector: string; text: string; props: Record<string, string> } }
  | { kind: 'inspect'; tabId?: number; data: { selector: string; props?: Record<string, string>; error?: string } }
  | { kind: 'zoom'; tabId?: number; data: { factor: number } }
  | { kind: 'cleared'; tabId?: number; data: { history: boolean; cookies: boolean; storage: boolean; cache: boolean } }
  | { kind: 'tabs'; data: { tabs: BrowserDockTabInfo[]; active: number | null } }
  | { kind: 'visible'; data: { session?: string | null; source?: string } }
  | { kind: 'agent_action'; tabId?: number; data: { reqId: number; kind: string; args: unknown; tabId?: number; ts: number } }
  | { kind: string; tabId?: number; data: unknown }

type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>
type ListenFn = <T>(event: string, cb: (event: { payload: T }) => void) => Promise<() => void>

let invokeRef: InvokeFn | null = null
let listenRef: ListenFn | null = null
let bootPromise: Promise<void> | null = null

async function ensureBoot(): Promise<void> {
  if (!isTauriRuntime()) return
  if (invokeRef && listenRef) return
  if (!bootPromise) {
    bootPromise = (async () => {
      const core = (await import(/* @vite-ignore */ '@tauri-apps/api/core')) as {
        invoke: InvokeFn
      }
      const events = (await import(/* @vite-ignore */ '@tauri-apps/api/event')) as {
        listen: ListenFn
      }
      invokeRef = core.invoke
      listenRef = events.listen
    })().catch((err) => {
      console.warn('[browserDock] failed to bootstrap Tauri IPC', err)
      bootPromise = null
      throw err
    })
  }
  await bootPromise
}

async function invokeIfTauri<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!isTauriRuntime()) return null
  await ensureBoot()
  if (!invokeRef) return null
  return invokeRef<T>(cmd, args)
}

export async function dockOpen(rect: BrowserDockRect, url: string | null): Promise<void> {
  await invokeIfTauri('browser_dock_open', {
    rect,
    url: url && url.trim() ? url : null,
  })
}

export async function dockSetRect(rect: BrowserDockRect): Promise<void> {
  await invokeIfTauri('browser_dock_set_rect', { rect })
}

export async function dockHide(): Promise<void> {
  await invokeIfTauri('browser_dock_hide')
}

export async function dockClose(): Promise<void> {
  await invokeIfTauri('browser_dock_close')
}

export async function dockNavigate(url: string): Promise<void> {
  if (!url || !url.trim()) return
  await invokeIfTauri('browser_dock_navigate', { url })
}

export async function dockBack(): Promise<void> {
  await invokeIfTauri('browser_dock_back')
}

export async function dockForward(): Promise<void> {
  await invokeIfTauri('browser_dock_forward')
}

export async function dockReload(hard = false): Promise<void> {
  await invokeIfTauri('browser_dock_reload', { hard })
}

export async function dockSetZoom(factor: number): Promise<void> {
  await invokeIfTauri('browser_dock_set_zoom', { factor })
}

export async function dockSetPickMode(enabled: boolean): Promise<void> {
  await invokeIfTauri('browser_dock_set_pick_mode', { enabled })
}

export async function dockInspectSelector(selector: string): Promise<void> {
  if (!selector) return
  await invokeIfTauri('browser_dock_inspect_selector', { selector })
}

export async function dockClear(opts: {
  cookies?: boolean
  cache?: boolean
  history?: boolean
}): Promise<void> {
  await invokeIfTauri('browser_dock_clear', {
    cookies: opts.cookies ?? false,
    cache: opts.cache ?? false,
    history: opts.history ?? false,
  })
}

export async function dockRequestState(): Promise<void> {
  await invokeIfTauri('browser_dock_request_state')
}

export async function dockGetState(): Promise<{ url: string | null; title: string | null }> {
  const result = await invokeIfTauri<{ url: string | null; title: string | null }>(
    'browser_dock_get_state',
  )
  return result ?? { url: null, title: null }
}

export async function dockOpenDevTools(): Promise<void> {
  await invokeIfTauri('browser_dock_open_devtools')
}

export async function dockCloseDevTools(): Promise<void> {
  await invokeIfTauri('browser_dock_close_devtools')
}

export async function dockNewTab(url: string | null, activate = true): Promise<number | null> {
  return invokeIfTauri<number>('browser_dock_new_tab', {
    url: url && url.trim() ? url : null,
    activate,
  })
}

export async function dockCloseTab(tabId: number): Promise<number | null> {
  return invokeIfTauri<number | null>('browser_dock_close_tab', { tabId })
}

export async function dockActivateTab(tabId: number): Promise<void> {
  await invokeIfTauri('browser_dock_activate_tab', { tabId })
}

export async function dockListTabs(): Promise<BrowserDockTabInfo[]> {
  const res = await invokeIfTauri<BrowserDockTabInfo[]>('browser_dock_list_tabs')
  return res ?? []
}

export type BrowserDockScreenshotPayload = {
  png_base64: string
  bytes: number
  full_page: boolean
}

export async function dockScreenshot(
  fullPage = false,
): Promise<BrowserDockScreenshotPayload | null> {
  return invokeIfTauri<BrowserDockScreenshotPayload>('browser_dock_screenshot', {
    fullPage,
  })
}

export async function listenDockEvents(
  cb: (event: BrowserDockEvent) => void,
): Promise<() => void> {
  if (!isTauriRuntime()) return () => {}
  await ensureBoot()
  if (!listenRef) return () => {}
  try {
    const unlisten = await listenRef<BrowserDockEvent>(
      'browser_dock_event',
      ({ payload }) => {
        try {
          cb(payload)
        } catch (err) {
          console.warn('[browserDock] event handler threw', err)
        }
      },
    )
    return unlisten
  } catch (err) {
    console.warn('[browserDock] listen failed', err)
    return () => {}
  }
}
