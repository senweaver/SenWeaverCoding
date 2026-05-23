// SPDX-License-Identifier: MIT

import { isTauriRuntime } from './desktopRuntime'

const GW_SESSION_PREFIX = 'gw_'

export function normalizeBrowserSessionId(
  raw: string | null | undefined,
): string | null {
  if (raw == null) return null
  const trimmed = raw.trim()
  if (!trimmed) return null
  return trimmed.startsWith(GW_SESSION_PREFIX)
    ? trimmed.slice(GW_SESSION_PREFIX.length)
    : trimmed
}

export type BrowserDockRect = { x: number; y: number; w: number; h: number }

export type BrowserHostBounds = {
  left: number
  top: number
  right: number
  bottom: number
}

export function clampRectToHost(
  rect: BrowserDockRect,
  host: BrowserHostBounds | null | undefined,
): BrowserDockRect {
  if (!host) {
    return {
      x: Math.max(0, Math.round(rect.x)),
      y: Math.max(0, Math.round(rect.y)),
      w: Math.max(1, Math.round(rect.w)),
      h: Math.max(1, Math.round(rect.h)),
    }
  }
  const hostLeft = Math.max(0, host.left)
  const hostTop = Math.max(0, host.top)
  const hostRight = Math.max(hostLeft + 1, host.right)
  const hostBottom = Math.max(hostTop + 1, host.bottom)
  const left = Math.min(Math.max(rect.x, hostLeft), hostRight - 1)
  const top = Math.min(Math.max(rect.y, hostTop), hostBottom - 1)
  const rectRight = rect.x + rect.w
  const rectBottom = rect.y + rect.h
  const right = Math.min(rectRight, hostRight)
  const bottom = Math.min(rectBottom, hostBottom)
  return {
    x: Math.round(left),
    y: Math.round(top),
    w: Math.max(1, Math.round(right - left)),
    h: Math.max(1, Math.round(bottom - top)),
  }
}

export type BrowserDockTabOwner = 'user' | 'agent'

export type BrowserDockTabInfo = {
  id: number
  url: string | null
  title: string | null
  active: boolean
  owner?: BrowserDockTabOwner
  sessionId?: string | null
}

export type BrowserDockEvent =
  | { kind: 'state'; tabId?: number; sessionId?: string | null; data: { url: string; title: string; canBack: boolean; ts: number } }
  | { kind: 'console'; tabId?: number; sessionId?: string | null; data: { level: string; message: string; ts: number } }
  | { kind: 'pick'; tabId?: number; sessionId?: string | null; data: { selector: string; text: string; props: Record<string, string> } }
  | { kind: 'inspect'; tabId?: number; sessionId?: string | null; data: { selector: string; props?: Record<string, string>; error?: string } }
  | { kind: 'zoom'; tabId?: number; sessionId?: string | null; data: { factor: number } }
  | { kind: 'cleared'; tabId?: number; sessionId?: string | null; data: { history: boolean; cookies: boolean; storage: boolean; cache: boolean } }
  | { kind: 'tabs'; sessionId?: string | null; data: { tabs: BrowserDockTabInfo[]; active: number | null; activeSessionId?: string | null } }
  | { kind: 'visible'; tabId?: number; sessionId?: string | null; data: { session?: string | null; source?: string; agentTabId?: number | null } }
  | { kind: 'agent_action'; tabId?: number; sessionId?: string | null; data: { reqId: number; kind: string; args: unknown; tabId?: number; sessionId?: string | null; ts: number } }
  | { kind: 'dock_takeover'; tabId?: number; sessionId?: string | null; data: { tab_id: number; started_at: number; sessionId?: string | null } }
  | { kind: 'dock_takeover_end'; tabId?: number; sessionId?: string | null; data: { tab_id: number; ended_at: number; sessionId?: string | null } }
  | { kind: string; tabId?: number; sessionId?: string | null; data: unknown }

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

export async function dockOpen(
  rect: BrowserDockRect,
  url: string | null,
  sessionId?: string | null,
): Promise<void> {
  const trimmed = typeof sessionId === 'string' ? sessionId.trim() : ''
  await invokeIfTauri('browser_dock_open', {
    rect,
    url: url && url.trim() ? url : null,
    sessionId: trimmed.length > 0 ? trimmed : null,
  })
}

export async function dockSetForegroundSession(
  sessionId: string | null,
): Promise<void> {
  const trimmed = typeof sessionId === 'string' ? sessionId.trim() : ''
  await invokeIfTauri('browser_dock_set_foreground_session', {
    sessionId: trimmed.length > 0 ? trimmed : null,
  })
}

export async function dockSetRect(rect: BrowserDockRect): Promise<void> {
  await invokeIfTauri('browser_dock_set_rect', { rect })
}

export async function dockResync(rect: BrowserDockRect): Promise<void> {
  await invokeIfTauri('browser_dock_resync', { rect })
}

export async function dockHide(): Promise<void> {
  await invokeIfTauri('browser_dock_hide')
}

export async function dockPark(): Promise<void> {
  await invokeIfTauri('browser_dock_park')
}

export async function dockFocusActive(): Promise<void> {
  await invokeIfTauri('browser_dock_focus_active')
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

export async function dockNewTab(
  url: string | null,
  activate = true,
  sessionId?: string | null,
): Promise<number | null> {
  const trimmed = typeof sessionId === 'string' ? sessionId.trim() : ''
  return invokeIfTauri<number>('browser_dock_new_tab', {
    url: url && url.trim() ? url : null,
    activate,
    sessionId: trimmed.length > 0 ? trimmed : null,
  })
}

export async function dockCloseTab(tabId: number): Promise<number | null> {
  return invokeIfTauri<number | null>('browser_dock_close_tab', { tabId })
}

export async function dockActivateTab(
  tabId: number,
  sessionId?: string | null,
): Promise<void> {
  const trimmed = typeof sessionId === 'string' ? sessionId.trim() : ''
  await invokeIfTauri('browser_dock_activate_tab', {
    tabId,
    sessionId: trimmed.length > 0 ? trimmed : null,
  })
}

export async function dockListTabs(
  sessionId?: string | null,
): Promise<BrowserDockTabInfo[]> {
  const trimmed = typeof sessionId === 'string' ? sessionId.trim() : ''
  const res = await invokeIfTauri<BrowserDockTabInfo[]>('browser_dock_list_tabs', {
    sessionId: trimmed.length > 0 ? trimmed : null,
  })
  return res ?? []
}

export async function dockPinTestTarget(sessionId: string, tabId: number): Promise<void> {
  await invokeIfTauri('browser_dock_pin_test_target', { sessionId, tabId })
}

export async function dockClearTestTarget(sessionId: string): Promise<void> {
  await invokeIfTauri('browser_dock_clear_test_target', { sessionId })
}

export async function dockGetTestTarget(sessionId: string): Promise<number | null> {
  const res = await invokeIfTauri<number | null>('browser_dock_get_test_target', {
    sessionId,
  })
  return res ?? null
}

export async function dockPresentSession(sessionId: string): Promise<number | null> {
  const res = await invokeIfTauri<number | null>('browser_dock_present_session', {
    sessionId,
  })
  return res ?? null
}

export async function dockReleaseAgentTabForSession(
  sessionId: string,
): Promise<number[]> {
  const res = await invokeIfTauri<number[]>(
    'browser_dock_release_agent_tab_for_session',
    { sessionId },
  )
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
