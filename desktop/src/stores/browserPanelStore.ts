// SPDX-License-Identifier: MIT
//
// Per-session state for the embedded browser dock that hovers above
// the chat composer.  The Tauri shell owns a single child WebView
// keyed by the static `browser_dock` label, so the store also
// remembers which session "owns" the dock right now and keeps the
// React panel in lock-step with the underlying Tauri view.
//
// Wire-up:
//   - `chatStore.handleServerMessage` calls `openForTool` whenever a
//     `tool_use_complete` for a browser-family tool arrives, seeding
//     the address bar with the tool's `url` argument.
//   - `Sidebar` exposes a manual entry point that calls
//     `toggle(activeSessionId, { source: 'manual' })`.
//   - `EmbeddedBrowserPanel` reads `getPanelState(sessionId)` and
//     dispatches every user action (navigate / back / pick / clear /
//     zoom) through the actions exported here.

import { create } from 'zustand'

import {
  dockActivateTab,
  dockBack,
  dockClear,
  dockClearTestTarget,
  dockClose,
  dockCloseTab,
  dockForward,
  dockHide,
  dockInspectSelector,
  dockListTabs,
  dockNavigate,
  dockNewTab,
  dockOpen,
  dockPinTestTarget,
  dockRequestState,
  dockReload,
  dockSetPickMode,
  dockSetZoom,
  type BrowserDockEvent,
  type BrowserDockRect,
  type BrowserDockTabInfo,
} from '../lib/browserDock'

const SCHEME_RE = /^[a-z][a-z0-9+\-.]*:/i
const WINDOWS_PATH_RE = /^[a-z]:[\\/]/i
const UNIX_PATH_RE = /^\/[^/]/

function isLoopbackAuthority(input: string): boolean {
  const authority = input.split('/')[0] ?? ''
  const afterUserInfo = authority.includes('@') ? authority.split('@').pop()! : authority
  if (!afterUserInfo) return false
  if (afterUserInfo.startsWith('[')) {
    const close = afterUserInfo.indexOf(']')
    if (close === -1) return false
    const v6 = afterUserInfo.slice(1, close).toLowerCase()
    return v6 === '::1'
  }
  const host = afterUserInfo.split(':')[0]?.toLowerCase() ?? ''
  if (!host) return false
  if (host === 'localhost') return true
  if (host.endsWith('.localhost')) return true
  if (/^127\.\d+\.\d+\.\d+$/.test(host)) return true
  return false
}

export function normalizeAddressBarUrl(raw: string): string {
  const trimmed = raw.trim()
  if (!trimmed) return ''
  if (trimmed === 'about:blank') return trimmed
  if (SCHEME_RE.test(trimmed)) return trimmed

  if (WINDOWS_PATH_RE.test(trimmed)) {
    const forward = trimmed.replace(/\\/g, '/')
    return `file:///${forward}`
  }

  if (UNIX_PATH_RE.test(trimmed)) {
    return `file://${trimmed}`
  }

  if (isLoopbackAuthority(trimmed)) {
    return `http://${trimmed}`
  }

  return `https://${trimmed}`
}

export type BrowserPanelSource = 'manual' | 'tool' | 'agent'

export type BrowserConsoleEntry = {
  id: number
  level: string
  message: string
  ts: number
}

export type BrowserInspectorSnapshot = {
  selector: string
  props: Record<string, string>
  text?: string
  ts: number
}

export type BrowserAgentActionEntry = {
  id: number
  reqId: number
  kind: string
  args: unknown
  ts: number
}

export type BrowserUserActionEntry = {
  id: number
  kind: string
  detail: string
  ts: number
}

export const BROWSER_COLUMN_WIDTH_BOUNDS = {
  min: 360,
  max: 1200,
  default: 520,
} as const

export type BrowserPanelState = {

  visible: boolean

  url: string

  liveUrl: string

  title: string

  canBack: boolean

  zoom: number

  consoleOpen: boolean
  inspectorOpen: boolean
  pickMode: boolean
  consoleLog: BrowserConsoleEntry[]
  inspector: BrowserInspectorSnapshot | null

  lastSource: BrowserPanelSource

  anchorRect: BrowserDockRect | null

  driverOpen: boolean

  agentLog: BrowserAgentActionEntry[]

  userLog: BrowserUserActionEntry[]

  lastAgentActionAt: number

  tabActivity: Record<number, number>

  tabs: BrowserDockTabInfo[]

  activeTabId: number | null

  preferredTestTabId: number | null

  columnWidth: number

  columnWidthAuto: boolean

  drawerHeightRatio: number
}

type ToggleOptions = {
  source?: BrowserPanelSource
  url?: string | null
}

type StoreState = {

  activeSessionId: string | null
  panels: Record<string, BrowserPanelState>
  ensure: (sessionId: string) => BrowserPanelState
  setVisible: (sessionId: string, visible: boolean) => void
  setAnchorRect: (sessionId: string, rect: BrowserDockRect) => void
  setUrl: (sessionId: string, url: string) => void
  setLiveState: (
    sessionId: string,
    next: { url?: string; title?: string; canBack?: boolean },
  ) => void
  setZoom: (sessionId: string, factor: number) => void
  setConsoleOpen: (sessionId: string, open: boolean) => void
  setInspectorOpen: (sessionId: string, open: boolean) => void
  setPickMode: (sessionId: string, enabled: boolean) => void
  appendConsole: (sessionId: string, entry: { level: string; message: string; ts: number }) => void
  setInspector: (sessionId: string, snap: BrowserInspectorSnapshot | null) => void
  clearConsole: (sessionId: string) => void
  reset: (sessionId: string) => void

  openForTool: (sessionId: string, opts?: ToggleOptions) => Promise<void>
  toggle: (sessionId: string, opts?: ToggleOptions) => Promise<void>
  navigate: (sessionId: string, url: string) => Promise<void>
  back: (sessionId: string) => Promise<void>
  forward: (sessionId: string) => Promise<void>
  reload: (sessionId: string, hard?: boolean) => Promise<void>
  zoom: (sessionId: string, delta: number | 'reset') => Promise<void>
  togglePick: (sessionId: string) => Promise<void>
  toggleConsole: (sessionId: string) => Promise<void>
  toggleInspector: (sessionId: string) => Promise<void>
  inspectSelector: (sessionId: string, selector: string) => Promise<void>
  clearStorage: (sessionId: string, opts: { cookies?: boolean; cache?: boolean; history?: boolean }) => Promise<void>
  closeForSession: (sessionId: string) => Promise<void>
  ingestEvent: (event: BrowserDockEvent) => void

  toggleDriver: (sessionId: string) => void

  clearAgentLog: (sessionId: string) => void

  refreshTabs: (sessionId: string) => Promise<void>

  newTab: (sessionId: string, url?: string | null, activate?: boolean) => Promise<number | null>

  closeTab: (sessionId: string, tabId: number) => Promise<void>

  activateTab: (sessionId: string, tabId: number) => Promise<void>

  setPreferredTestTab: (sessionId: string, tabId: number) => Promise<void>

  clearPreferredTestTab: (sessionId: string) => Promise<void>

  setColumnWidth: (sessionId: string, px: number) => void

  setColumnWidthAuto: (sessionId: string, auto: boolean) => void

  setDrawerHeightRatio: (sessionId: string, ratio: number) => void

  appendUserAction: (sessionId: string, entry: { kind: string; detail: string; ts?: number }) => void

  clearUserLog: (sessionId: string) => void
}

const COLUMN_WIDTH_STORAGE_KEY = 'sen-browser-column-width'
const COLUMN_WIDTH_AUTO_STORAGE_KEY = 'sen-browser-column-width-auto'
const DRAWER_RATIO_STORAGE_KEY = 'sen-browser-drawer-ratio'

function clampRatio(value: number, min: number, max: number, fallback: number): number {
  if (!Number.isFinite(value)) return fallback
  if (value < min) return min
  if (value > max) return max
  return value
}

export function clampColumnWidth(value: number): number {
  if (!Number.isFinite(value)) return BROWSER_COLUMN_WIDTH_BOUNDS.default
  return Math.min(
    BROWSER_COLUMN_WIDTH_BOUNDS.max,
    Math.max(BROWSER_COLUMN_WIDTH_BOUNDS.min, Math.round(value)),
  )
}

function readStoredColumnWidth(): number {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') {
    return BROWSER_COLUMN_WIDTH_BOUNDS.default
  }
  try {
    const raw = localStorage.getItem(COLUMN_WIDTH_STORAGE_KEY)
    if (!raw) return BROWSER_COLUMN_WIDTH_BOUNDS.default
    const value = Number.parseInt(raw, 10)
    if (Number.isFinite(value)) return clampColumnWidth(value)
  } catch {

  }
  return BROWSER_COLUMN_WIDTH_BOUNDS.default
}

function writeStoredColumnWidth(value: number) {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(COLUMN_WIDTH_STORAGE_KEY, String(clampColumnWidth(value)))
  } catch {

  }
}

function readStoredColumnWidthAuto(): boolean {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return true
  try {
    const raw = localStorage.getItem(COLUMN_WIDTH_AUTO_STORAGE_KEY)
    if (raw === 'false') return false
    if (raw === 'true') return true
  } catch {

  }
  return true
}

function writeStoredColumnWidthAuto(value: boolean) {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(COLUMN_WIDTH_AUTO_STORAGE_KEY, value ? 'true' : 'false')
  } catch {

  }
}

function readStoredDrawerRatio(): number {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return 0.35
  try {
    const raw = localStorage.getItem(DRAWER_RATIO_STORAGE_KEY)
    if (!raw) return 0.35
    const value = Number.parseFloat(raw)
    return clampRatio(value, 0.15, 0.6, 0.35)
  } catch {
    return 0.35
  }
}

function writeStoredDrawerRatio(value: number) {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(DRAWER_RATIO_STORAGE_KEY, String(value))
  } catch {

  }
}

const DEFAULT_STATE: BrowserPanelState = {
  visible: false,
  url: '',
  liveUrl: '',
  title: '',
  canBack: false,
  zoom: 1,
  consoleOpen: false,
  inspectorOpen: false,
  pickMode: false,
  consoleLog: [],
  inspector: null,
  lastSource: 'manual',
  anchorRect: null,
  driverOpen: false,
  agentLog: [],
  userLog: [],
  lastAgentActionAt: 0,
  tabActivity: {},
  tabs: [],
  activeTabId: null,
  preferredTestTabId: null,
  columnWidth: BROWSER_COLUMN_WIDTH_BOUNDS.default,
  columnWidthAuto: true,
  drawerHeightRatio: 0.35,
}

let consoleSeq = 0
let agentLogSeq = 0
let userLogSeq = 0
const CONSOLE_RING_MAX = 200
const AGENT_LOG_RING_MAX = 200
const USER_LOG_RING_MAX = 200

function patchPanel(
  panels: Record<string, BrowserPanelState>,
  sessionId: string,
  patch: Partial<BrowserPanelState>,
): Record<string, BrowserPanelState> {
  const prev = panels[sessionId] ?? DEFAULT_STATE
  return {
    ...panels,
    [sessionId]: { ...prev, ...patch },
  }
}

function pickActiveSessionPanel(
  state: StoreState,
): { sessionId: string; panel: BrowserPanelState } | null {
  if (!state.activeSessionId) return null
  const panel = state.panels[state.activeSessionId]
  if (!panel) return null
  return { sessionId: state.activeSessionId, panel }
}

export const useBrowserPanelStore = create<StoreState>((set, get) => ({
  activeSessionId: null,
  panels: {},

  ensure: (sessionId) => {
    const existing = get().panels[sessionId]
    if (existing) return existing
    const next: BrowserPanelState = {
      ...DEFAULT_STATE,
      columnWidth: readStoredColumnWidth(),
      columnWidthAuto: readStoredColumnWidthAuto(),
      drawerHeightRatio: readStoredDrawerRatio(),
    }
    set((state) => ({ panels: { ...state.panels, [sessionId]: next } }))
    return next
  },

  setVisible: (sessionId, visible) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { visible }) })),

  setAnchorRect: (sessionId, rect) => {
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { anchorRect: rect }) }))
  },

  setUrl: (sessionId, url) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { url }) })),

  setLiveState: (sessionId, next) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const liveUrl = next.url ?? prev.liveUrl
      const url = liveUrl || prev.url
      return {
        panels: patchPanel(state.panels, sessionId, {
          liveUrl,
          url,
          title: next.title ?? prev.title,
          canBack: next.canBack ?? prev.canBack,
        }),
      }
    }),

  setZoom: (sessionId, factor) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { zoom: factor }) })),

  setConsoleOpen: (sessionId, open) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { consoleOpen: open }) })),

  setInspectorOpen: (sessionId, open) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { inspectorOpen: open }) })),

  setPickMode: (sessionId, enabled) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { pickMode: enabled }) })),

  appendConsole: (sessionId, entry) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const id = ++consoleSeq
      const next = [...prev.consoleLog, { id, ...entry }]
      while (next.length > CONSOLE_RING_MAX) next.shift()
      return { panels: patchPanel(state.panels, sessionId, { consoleLog: next }) }
    }),

  setInspector: (sessionId, snap) =>
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { inspector: snap, inspectorOpen: !!snap }),
    })),

  clearConsole: (sessionId) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { consoleLog: [] }) })),

  reset: (sessionId) =>
    set((state) => {
      const next = { ...state.panels }
      delete next[sessionId]
      return {
        panels: next,
        activeSessionId: state.activeSessionId === sessionId ? null : state.activeSessionId,
      }
    }),

  openForTool: async (sessionId, opts) => {
    const source: BrowserPanelSource = opts?.source ?? 'tool'
    const seedUrl = opts?.url ?? null
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      return {
        activeSessionId: sessionId,
        panels: patchPanel(state.panels, sessionId, {
          visible: true,
          lastSource: source,
          url: seedUrl ?? prev.url,
        }),
      }
    })
    const rect = get().panels[sessionId]?.anchorRect ?? null
    try {
      if (rect) {
        await dockOpen(rect, seedUrl)
      } else {
        await dockOpen({ x: 0, y: 0, w: 1, h: 1 }, seedUrl)
      }
    } catch (err) {
      console.warn('[browserDock] openForTool dockOpen failed', err)
    }
    dockRequestState().catch((err) => {
      console.warn('[browserDock] dockRequestState failed', err)
    })
  },

  toggle: async (sessionId, opts) => {
    const cur = get().panels[sessionId] ?? DEFAULT_STATE
    const ownsActive = get().activeSessionId === sessionId
    const wasVisible = ownsActive && cur.visible
    const wantsVisible = !wasVisible
    const source: BrowserPanelSource = opts?.source ?? 'manual'
    const seedUrl = opts?.url ?? null
    set((state) => ({
      activeSessionId: wantsVisible ? sessionId : state.activeSessionId,
      panels: patchPanel(state.panels, sessionId, {
        visible: wantsVisible,
        lastSource: source,
        url: seedUrl ?? cur.url,
      }),
    }))
    try {
      if (wantsVisible) {
        const rect = get().panels[sessionId]?.anchorRect ?? { x: 0, y: 0, w: 1, h: 1 }
        await dockOpen(rect, seedUrl ?? cur.url ?? null)
        dockRequestState().catch((err) => {
          console.warn('[browserDock] dockRequestState failed', err)
        })
      } else {
        await dockHide()
      }
    } catch (err) {
      console.warn('[browserDock] toggle dock failed', err)
    }
  },

  navigate: async (sessionId, url) => {
    const trimmed = url.trim()
    if (!trimmed) return
    const normalized = normalizeAddressBarUrl(trimmed)
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { url: normalized }) }))
    get().appendUserAction(sessionId, { kind: 'navigate', detail: normalized })
    if (get().activeSessionId !== sessionId) {
      try {
        await get().openForTool(sessionId, { source: 'manual', url: normalized })
      } catch (err) {
        console.warn('[browserDock] navigate openForTool failed', err)
      }
      return
    }
    try {
      await dockNavigate(normalized)
    } catch (err) {
      console.warn('[browserDock] dockNavigate failed', err)
    }
  },

  back: async (sessionId) => {
    try {
      await dockBack()
      if (sessionId) get().appendUserAction(sessionId, { kind: 'back', detail: '' })
    } catch (err) {
      console.warn('[browserDock] back failed', err)
    }
  },

  forward: async (sessionId) => {
    try {
      await dockForward()
      if (sessionId) get().appendUserAction(sessionId, { kind: 'forward', detail: '' })
    } catch (err) {
      console.warn('[browserDock] forward failed', err)
    }
  },

  reload: async (sessionId, hard) => {
    try {
      await dockReload(hard ?? false)
      if (sessionId) {
        get().appendUserAction(sessionId, {
          kind: hard ? 'hard_reload' : 'reload',
          detail: '',
        })
      }
    } catch (err) {
      console.warn('[browserDock] reload failed', err)
    }
  },

  zoom: async (sessionId, delta) => {
    const cur = get().panels[sessionId]?.zoom ?? 1
    const next = delta === 'reset' ? 1 : Math.min(3, Math.max(0.25, cur + delta))
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { zoom: next }) }))
    try {
      await dockSetZoom(next)
    } catch (err) {
      console.warn('[browserDock] zoom failed', err)
    }
  },

  togglePick: async (sessionId) => {
    const enabled = !(get().panels[sessionId]?.pickMode ?? false)
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { pickMode: enabled }) }))
    try {
      await dockSetPickMode(enabled)
    } catch (err) {
      console.warn('[browserDock] togglePick failed', err)
    }
  },

  toggleConsole: async (sessionId) => {
    const open = !(get().panels[sessionId]?.consoleOpen ?? false)
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { consoleOpen: open }) }))
  },

  toggleInspector: async (sessionId) => {
    const open = !(get().panels[sessionId]?.inspectorOpen ?? false)
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { inspectorOpen: open }) }))
  },

  inspectSelector: async (_sessionId, selector) => {
    if (!selector) return
    try {
      await dockInspectSelector(selector)
    } catch (err) {
      console.warn('[browserDock] inspectSelector failed', err)
    }
  },

  clearStorage: async (_sessionId, opts) => {
    try {
      await dockClear(opts)
    } catch (err) {
      console.warn('[browserDock] clearStorage failed', err)
    }
  },

  closeForSession: async (sessionId) => {
    set((state) => ({
      activeSessionId: state.activeSessionId === sessionId ? null : state.activeSessionId,
      panels: patchPanel(state.panels, sessionId, { visible: false, pickMode: false }),
    }))
    try {
      await dockClose()
    } catch (err) {
      console.warn('[browserDock] closeForSession dockClose failed', err)
    }
  },

  toggleDriver: (sessionId) => {
    set((state) => {
      const cur = state.panels[sessionId] ?? DEFAULT_STATE
      return { panels: patchPanel(state.panels, sessionId, { driverOpen: !cur.driverOpen }) }
    })
  },

  clearAgentLog: (sessionId) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { agentLog: [] }) })),

  refreshTabs: async (sessionId) => {
    try {
      const tabs = await dockListTabs()
      const active = tabs.find((t) => t.active)?.id ?? null
      set((state) => ({
        panels: patchPanel(state.panels, sessionId, { tabs, activeTabId: active }),
      }))
    } catch (err) {
      console.warn('[browserDock] refreshTabs failed', err)
    }
  },

  newTab: async (sessionId, url, activate) => {
    try {
      const id = await dockNewTab(url ?? null, activate ?? true)
      if (id != null && (activate ?? true)) {
        set((state) => ({
          panels: patchPanel(state.panels, sessionId, { activeTabId: id }),
        }))
      }
      get().appendUserAction(sessionId, {
        kind: 'new_tab',
        detail: url ?? '',
      })
      return id
    } catch (err) {
      console.warn('[browserDock] newTab failed', err)
      return null
    }
  },

  closeTab: async (sessionId, tabId) => {
    try {
      await dockCloseTab(tabId)
      if (sessionId) {
        get().appendUserAction(sessionId, {
          kind: 'close_tab',
          detail: String(tabId),
        })
      }
    } catch (err) {
      console.warn('[browserDock] closeTab failed', err)
    }
  },

  activateTab: async (sessionId, tabId) => {
    try {
      await dockActivateTab(tabId)
      set((state) => ({
        panels: patchPanel(state.panels, sessionId, { activeTabId: tabId }),
      }))
      get().appendUserAction(sessionId, {
        kind: 'activate_tab',
        detail: String(tabId),
      })
    } catch (err) {
      console.warn('[browserDock] activateTab failed', err)
    }
  },

  setPreferredTestTab: async (sessionId, tabId) => {
    try {
      await dockPinTestTarget(tabId)
      set((state) => ({
        panels: patchPanel(state.panels, sessionId, { preferredTestTabId: tabId }),
      }))
      get().appendUserAction(sessionId, {
        kind: 'pin_test_target',
        detail: String(tabId),
      })
    } catch (err) {
      console.warn('[browserDock] pinTestTarget failed', err)
    }
  },

  clearPreferredTestTab: async (sessionId) => {
    try {
      await dockClearTestTarget()
      set((state) => ({
        panels: patchPanel(state.panels, sessionId, { preferredTestTabId: null }),
      }))
      get().appendUserAction(sessionId, {
        kind: 'clear_test_target',
        detail: '',
      })
    } catch (err) {
      console.warn('[browserDock] clearTestTarget failed', err)
    }
  },

  setColumnWidth: (sessionId, px) => {
    const next = clampColumnWidth(px)
    set((state) => {
      const updated: Record<string, BrowserPanelState> = { ...state.panels }
      for (const [id, panel] of Object.entries(state.panels)) {
        updated[id] = { ...panel, columnWidth: next, columnWidthAuto: false }
      }
      if (!updated[sessionId]) {
        updated[sessionId] = {
          ...DEFAULT_STATE,
          columnWidth: next,
          columnWidthAuto: false,
        }
      }
      return { panels: updated }
    })
    writeStoredColumnWidth(next)
    writeStoredColumnWidthAuto(false)
  },

  setColumnWidthAuto: (sessionId, auto) => {
    set((state) => {
      const updated: Record<string, BrowserPanelState> = { ...state.panels }
      for (const [id, panel] of Object.entries(state.panels)) {
        updated[id] = { ...panel, columnWidthAuto: auto }
      }
      if (!updated[sessionId]) {
        updated[sessionId] = { ...DEFAULT_STATE, columnWidthAuto: auto }
      }
      return { panels: updated }
    })
    writeStoredColumnWidthAuto(auto)
  },

  setDrawerHeightRatio: (sessionId, ratio) => {
    const next = clampRatio(ratio, 0.15, 0.6, 0.35)
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { drawerHeightRatio: next }),
    }))
    writeStoredDrawerRatio(next)
  },

  appendUserAction: (sessionId, entry) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const id = ++userLogSeq
      const ts = entry.ts ?? Date.now()
      const next = [...prev.userLog, { id, kind: entry.kind, detail: entry.detail, ts }]
      while (next.length > USER_LOG_RING_MAX) next.shift()
      return { panels: patchPanel(state.panels, sessionId, { userLog: next }) }
    }),

  clearUserLog: (sessionId) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { userLog: [] }) })),

  ingestEvent: (event) => {

    const resolveSessionFromHint = (hint: string | null | undefined): string | null => {
      if (hint && get().panels[hint]) return hint
      const cur = get().activeSessionId
      if (cur) return cur
      const ids = Object.keys(get().panels)
      return ids[0] ?? null
    }

    const fallbackSession = (): string | null => {
      const cur = get().activeSessionId
      if (cur) return cur
      const ids = Object.keys(get().panels)
      return ids[0] ?? null
    }

    if (event.kind === 'visible') {
      const data = event.data as { session?: string | null; source?: string } | null
      const hinted = data?.session ?? null
      const sessionId = resolveSessionFromHint(hinted)
      if (!sessionId) return
      const prevActive = get().activeSessionId
      set((state) => ({
        activeSessionId: sessionId,
        panels: patchPanel(state.panels, sessionId, {
          visible: true,
          lastSource: 'agent',
        }),
      }))

      if (prevActive === null || prevActive === sessionId) {
        const seedRect = get().panels[sessionId]?.anchorRect ?? {
          x: 0,
          y: 0,
          w: 1,
          h: 1,
        }
        const seedUrl = get().panels[sessionId]?.url || null
        dockOpen(seedRect, seedUrl).catch((err) => {
          console.warn('[browserDock] ingestEvent dockOpen failed', err)
        })
      }
      dockRequestState().catch((err) => {
        console.warn('[browserDock] ingestEvent dockRequestState failed', err)
      })
      return
    }

    if (event.kind === 'tabs') {
      const sessionId = fallbackSession()
      if (!sessionId) return
      const data = event.data as {
        tabs?: BrowserDockTabInfo[]
        active?: number | null
      }
      const tabs = Array.isArray(data.tabs) ? data.tabs : []
      const active =
        typeof data.active === 'number'
          ? data.active
          : tabs.find((t) => t.active)?.id ?? null

      const act = tabs.find((t) => t.id === active)
      set((state) => {
        const prev = state.panels[sessionId] ?? DEFAULT_STATE
        const stalePin =
          prev.preferredTestTabId !== null &&
          !tabs.some((t) => t.id === prev.preferredTestTabId)
        const nextPin = stalePin ? null : prev.preferredTestTabId
        if (stalePin) {
          dockClearTestTarget().catch((err) => {
            console.warn('[browserDock] auto-clear stale pin failed', err)
          })
        }
        return {
          panels: patchPanel(state.panels, sessionId, {
            tabs,
            activeTabId: active,
            url: act?.url ?? prev.url,
            liveUrl: act?.url ?? prev.liveUrl,
            title: act?.title ?? prev.title,
            preferredTestTabId: nextPin,
          }),
        }
      })
      return
    }

    if (event.kind === 'agent_action') {
      const sessionId = fallbackSession()
      if (!sessionId) return
      const data = event.data as {
        reqId?: number
        kind?: string
        args?: unknown
        ts?: number
        tabId?: number
      }
      const evtTabId = (event as { tabId?: number }).tabId
      const tabIdForActivity =
        typeof evtTabId === 'number'
          ? evtTabId
          : typeof data.tabId === 'number'
            ? data.tabId
            : null
      const ts = typeof data.ts === 'number' ? data.ts : Date.now()
      const wasPickMode = get().panels[sessionId]?.pickMode ?? false
      set((state) => {
        const prev = state.panels[sessionId] ?? DEFAULT_STATE
        const id = ++agentLogSeq
        const reqId = typeof data.reqId === 'number' ? data.reqId : id
        const kind = typeof data.kind === 'string' ? data.kind : 'unknown'
        const next = [...prev.agentLog, { id, reqId, kind, args: data.args ?? null, ts }]
        while (next.length > AGENT_LOG_RING_MAX) next.shift()
        const tabActivity =
          tabIdForActivity !== null
            ? { ...prev.tabActivity, [tabIdForActivity]: ts }
            : prev.tabActivity
        return {
          panels: patchPanel(state.panels, sessionId, {
            agentLog: next,
            lastAgentActionAt: ts,
            tabActivity,
            visible: true,
            pickMode: false,
          }),
        }
      })
      if (wasPickMode) {
        dockSetPickMode(false).catch((err) => {
          console.warn('[browserDock] auto-disable pick on agent_action failed', err)
        })
      }
      return
    }

    const target = pickActiveSessionPanel(get())
    if (!target) return
    const { sessionId } = target
    if (event.kind === 'state') {
      const data = event.data as { url?: string; title?: string; canBack?: boolean }
      const evtTabId = (event as { tabId?: number }).tabId
      const panel = get().panels[sessionId]

      if (typeof evtTabId === 'number' && panel) {
        const tabs = panel.tabs.map((t) =>
          t.id === evtTabId
            ? {
                ...t,
                url: data.url ?? t.url,
                title: data.title ?? t.title,
              }
            : t,
        )
        set((state) => ({ panels: patchPanel(state.panels, sessionId, { tabs }) }))
      }

      if (
        typeof evtTabId !== 'number' ||
        evtTabId === panel?.activeTabId
      ) {
        get().setLiveState(sessionId, data)
      }
      return
    }
    if (event.kind === 'console') {
      const data = event.data as { level: string; message: string; ts: number }
      get().appendConsole(sessionId, data)
      return
    }
    if (event.kind === 'pick') {
      const data = event.data as {
        selector: string
        text?: string
        props?: Record<string, string>
      }
      get().setInspector(sessionId, {
        selector: data.selector,
        props: data.props ?? {},
        text: data.text,
        ts: Date.now(),
      })
      get().setPickMode(sessionId, false)
      return
    }
    if (event.kind === 'inspect') {
      const data = event.data as {
        selector: string
        props?: Record<string, string>
        error?: string
      }
      if (data.error) return
      get().setInspector(sessionId, {
        selector: data.selector,
        props: data.props ?? {},
        ts: Date.now(),
      })
      return
    }
    if (event.kind === 'zoom') {
      const data = event.data as { factor: number }
      if (typeof data.factor === 'number') get().setZoom(sessionId, data.factor)
    }
  },
}))

export function getBrowserPanelState(sessionId: string | null): BrowserPanelState | null {
  if (!sessionId) return null
  return useBrowserPanelStore.getState().panels[sessionId] ?? null
}
