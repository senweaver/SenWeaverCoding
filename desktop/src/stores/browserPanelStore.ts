// SPDX-License-Identifier: MIT

import { create } from 'zustand'

import {
  dockActivateTab,
  dockBack,
  dockClear,
  dockClearTestTarget,
  dockCloseTab,
  dockForward,
  dockHide,
  dockInspectSelector,
  dockGetTestTarget,
  dockListTabs,
  dockNavigate,
  dockNewTab,
  dockOpen,
  dockPinTestTarget,
  dockPresentSession,
  dockRequestState,
  dockReload,
  dockSetPickMode,
  dockSetRect,
  dockSetZoom,
  type BrowserDockEvent,
  type BrowserDockRect,
  type BrowserDockTabInfo,
  normalizeBrowserSessionId,
} from '../lib/browserDock'
import { useTabStore } from './tabStore'
import { useUIStore } from './uiStore'
import { t } from '../i18n'
import type { TranslationKey } from '../i18n'

async function measureViewportRect(): Promise<{
  x: number
  y: number
  w: number
  h: number
} | null> {
  if (typeof document === 'undefined') return null
  for (let attempt = 0; attempt < 10; attempt++) {
    await new Promise((resolve) => requestAnimationFrame(() => resolve(null)))
    const el = document.querySelector('[data-browser-viewport="true"]')
    if (!el) continue
    const rect = el.getBoundingClientRect()
    if (rect.width > 1 && rect.height > 1) {
      return {
        x: rect.left,
        y: rect.top,
        w: Math.max(1, rect.width - 1),
        h: Math.max(1, rect.height - 1),
      }
    }
  }
  return null
}

function notifyDockActionFailed(actionLabelKey: TranslationKey, err: unknown): void {
  console.warn(`[browserDock] ${actionLabelKey} failed`, err)
  useUIStore.getState().addToast({
    type: 'error',
    message: t('browser.panel.toast.actionFailed', { action: t(actionLabelKey) }),
    duration: 5000,
  })
}

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
    const withScheme = trimmed.includes('://') ? trimmed : `http://${trimmed}`
    try {
      const parsed = new URL(withScheme)
      const host = parsed.hostname.toLowerCase()
      if (host === 'localhost' || host === '::1') {
        parsed.hostname = '127.0.0.1'
      }
      return parsed.toString()
    } catch {
      return `http://${trimmed}`
    }
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

  pendingShow: boolean

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

  prototypeRefTabId: number | null

  prototypeRefFigmaUrl: string | null

  columnWidth: number

  columnWidthAuto: boolean

  drawerHeightRatio: number
}

type ToggleOptions = {
  source?: BrowserPanelSource
  url?: string | null

  presentOnly?: boolean
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
  appendConsoleBatch: (
    sessionId: string,
    entries: Array<{ level: string; message: string; ts: number }>,
  ) => void
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

  setPrototypeRefTab: (sessionId: string, tabId: number) => void

  clearPrototypeRefTab: (sessionId: string) => void

  setPrototypeRefFigma: (sessionId: string, url: string) => void

  clearPrototypeRefFigma: (sessionId: string) => void

  setColumnWidth: (sessionId: string, px: number) => void

  setColumnWidthAuto: (sessionId: string, auto: boolean) => void

  setDrawerHeightRatio: (sessionId: string, ratio: number) => void

  appendUserAction: (sessionId: string, entry: { kind: string; detail: string; ts?: number }) => void

  clearUserLog: (sessionId: string) => void
}

const COLUMN_WIDTH_STORAGE_PREFIX = 'sen-browser-column-width:'
const COLUMN_WIDTH_AUTO_STORAGE_PREFIX = 'sen-browser-column-width-auto:'
const DRAWER_RATIO_STORAGE_PREFIX = 'sen-browser-drawer-ratio:'
const DEFAULT_WORKSPACE_KEY = '__default__'

function normalizeWorkspaceKey(key: string | null | undefined): string {
  if (!key) return DEFAULT_WORKSPACE_KEY
  const trimmed = key.trim()
  return trimmed.length ? trimmed : DEFAULT_WORKSPACE_KEY
}

function currentWorkspaceKey(): string {
  if (typeof window === 'undefined') return DEFAULT_WORKSPACE_KEY
  try {
    const w = window as unknown as {
      __sen_active_workspace_key__?: string | null
    }
    return normalizeWorkspaceKey(w.__sen_active_workspace_key__)
  } catch {
    return DEFAULT_WORKSPACE_KEY
  }
}

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

function readStoredColumnWidth(workspaceKey: string): number {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') {
    return BROWSER_COLUMN_WIDTH_BOUNDS.default
  }
  try {
    const raw = localStorage.getItem(
      `${COLUMN_WIDTH_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}`,
    )
    if (!raw) return BROWSER_COLUMN_WIDTH_BOUNDS.default
    const value = Number.parseInt(raw, 10)
    if (Number.isFinite(value)) return clampColumnWidth(value)
  } catch {

  }
  return BROWSER_COLUMN_WIDTH_BOUNDS.default
}

function writeStoredColumnWidth(workspaceKey: string, value: number) {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(
      `${COLUMN_WIDTH_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}`,
      String(clampColumnWidth(value)),
    )
  } catch {

  }
}

function readStoredColumnWidthAuto(workspaceKey: string): boolean {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return true
  try {
    const raw = localStorage.getItem(
      `${COLUMN_WIDTH_AUTO_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}`,
    )
    if (raw === 'false') return false
    if (raw === 'true') return true
  } catch {

  }
  return true
}

function writeStoredColumnWidthAuto(workspaceKey: string, value: boolean) {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(
      `${COLUMN_WIDTH_AUTO_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}`,
      value ? 'true' : 'false',
    )
  } catch {

  }
}

function readStoredDrawerRatio(workspaceKey: string): number {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return 0.35
  try {
    const raw = localStorage.getItem(
      `${DRAWER_RATIO_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}`,
    )
    if (!raw) return 0.35
    const value = Number.parseFloat(raw)
    return clampRatio(value, 0.15, 0.6, 0.35)
  } catch {
    return 0.35
  }
}

function writeStoredDrawerRatio(workspaceKey: string, value: number) {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(
      `${DRAWER_RATIO_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}`,
      String(value),
    )
  } catch {

  }
}

const DEFAULT_STATE: BrowserPanelState = {
  visible: false,
  pendingShow: false,
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
  prototypeRefTabId: null,
  prototypeRefFigmaUrl: null,
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

function filterTabsForSession(
  allTabs: BrowserDockTabInfo[],
  sessionId: string,
): BrowserDockTabInfo[] {
  const canon = normalizeBrowserSessionId(sessionId)
  if (!canon) return []
  return allTabs.filter((tab) => normalizeBrowserSessionId(tab.sessionId) === canon)
}

function resolveSessionActiveTabId(
  sessionTabs: BrowserDockTabInfo[],
  globalActive: number | null,
  previous: number | null,
): number | null {
  if (
    globalActive !== null &&
    sessionTabs.some((tab) => tab.id === globalActive)
  ) {
    return globalActive
  }
  if (
    previous !== null &&
    sessionTabs.some((tab) => tab.id === previous)
  ) {
    return previous
  }
  const sessionMarked = sessionTabs.find((tab) => tab.active)
  if (sessionMarked) return sessionMarked.id
  return sessionTabs.length > 0 ? sessionTabs[sessionTabs.length - 1]!.id : null
}

function liveFieldsFromSessionTabs(
  sessionTabs: BrowserDockTabInfo[],
  activeTabId: number | null,
): Pick<BrowserPanelState, 'url' | 'liveUrl' | 'title'> {
  if (sessionTabs.length === 0) {
    return { url: '', liveUrl: '', title: '' }
  }
  const activeTab = sessionTabs.find((tab) => tab.id === activeTabId)
  const tabUrl = activeTab?.url ?? ''
  return {
    url: tabUrl,
    liveUrl: tabUrl,
    title: activeTab?.title ?? '',
  }
}

function resolvePreferredTestTabId(
  sessionTabs: BrowserDockTabInfo[],
  previous: number | null,
  backendPinned: number | null | undefined,
): number | null {
  const candidate =
    backendPinned !== undefined ? backendPinned : previous
  if (candidate === null) return null
  return sessionTabs.some((tab) => tab.id === candidate) ? candidate : null
}

function buildSessionTabPatch(
  sessionId: string,
  allTabs: BrowserDockTabInfo[],
  globalActive: number | null,
  prev: BrowserPanelState,
  backendPinned?: number | null,
): Partial<BrowserPanelState> {
  const tabs = filterTabsForSession(allTabs, sessionId)
  const activeTabId = resolveSessionActiveTabId(
    tabs,
    globalActive,
    prev.activeTabId,
  )
  const stalePin =
    prev.preferredTestTabId !== null &&
    !tabs.some((tab) => tab.id === prev.preferredTestTabId)
  if (stalePin) {
    dockClearTestTarget(sessionId).catch((err) => {
      console.warn('[browserDock] auto-clear stale pin failed', err)
    })
  }
  const preferredTestTabId = resolvePreferredTestTabId(
    tabs,
    stalePin ? null : prev.preferredTestTabId,
    stalePin ? null : backendPinned,
  )
  const staleProto =
    prev.prototypeRefTabId !== null &&
    !tabs.some((tab) => tab.id === prev.prototypeRefTabId)
  return {
    tabs,
    activeTabId: tabs.some((tab) => tab.id === activeTabId) ? activeTabId : null,
    ...liveFieldsFromSessionTabs(tabs, activeTabId),
    preferredTestTabId,
    prototypeRefTabId: staleProto ? null : prev.prototypeRefTabId,
  }
}

export const useBrowserPanelStore = create<StoreState>((set, get) => ({
  activeSessionId: null,
  panels: {},

  ensure: (sessionId) => {
    const existing = get().panels[sessionId]
    if (existing) return existing
    const workspaceKey = currentWorkspaceKey()
    const next: BrowserPanelState = {
      ...DEFAULT_STATE,
      columnWidth: readStoredColumnWidth(workspaceKey),
      columnWidthAuto: readStoredColumnWidthAuto(workspaceKey),
      drawerHeightRatio: readStoredDrawerRatio(workspaceKey),
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

  appendConsoleBatch: (sessionId, entries) => {
    if (entries.length === 0) return
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const appended = entries.map((entry) => ({ id: ++consoleSeq, ...entry }))
      const next = [...prev.consoleLog, ...appended]
      while (next.length > CONSOLE_RING_MAX) next.shift()
      return { panels: patchPanel(state.panels, sessionId, { consoleLog: next }) }
    })
  },

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
    const prevActiveSessionId = get().activeSessionId
    const prevVisible = get().panels[sessionId]?.visible ?? false
    const activeTabId = useTabStore.getState().activeTabId
    const isForegroundSession = activeTabId === sessionId
    if (!isForegroundSession) {
      set((state) => {
        const prev = state.panels[sessionId] ?? DEFAULT_STATE
        return {
          panels: patchPanel(state.panels, sessionId, {
            pendingShow: true,
            lastSource: source,
            url: seedUrl ?? (prev.tabs.length > 0 ? prev.url : ''),
          }),
        }
      })
      useUIStore.getState().addToast({
        type: 'info',
        message: t('browser.backgroundSessionWantsOpen'),
        duration: 5000,
        sessionId,
      })
      return
    }
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      return {
        activeSessionId: sessionId,
        panels: patchPanel(state.panels, sessionId, {
          visible: true,
          pendingShow: false,
          lastSource: source,
          url: seedUrl ?? (prev.tabs.length > 0 ? prev.url : ''),
        }),
      }
    })
    let rect = get().panels[sessionId]?.anchorRect ?? null
    if (!rect) {
      rect = await measureViewportRect()
    }
    try {
      if (opts?.presentOnly) {
        if (rect) {
          await dockSetRect(rect)
        }
        await dockPresentSession(sessionId)
      } else {
        await dockPresentSession(sessionId)
        await dockOpen(rect ?? { x: 0, y: 0, w: 1, h: 1 }, seedUrl, sessionId)
        if (typeof document !== 'undefined') {
          document.dispatchEvent(new CustomEvent('browser-panel-remeasure'))
        }
      }
    } catch (err) {
      set((state) => ({
        panels: patchPanel(state.panels, sessionId, { visible: prevVisible }),
        activeSessionId:
          state.activeSessionId === sessionId ? prevActiveSessionId : state.activeSessionId,
      }))
      notifyDockActionFailed('browser.panel.action.open', err)
      return
    }
    dockRequestState().catch((err) => {
      console.warn('[browserDock] dockRequestState failed', err)
    })
  },

  toggle: async (sessionId, opts) => {
    const cur = get().panels[sessionId] ?? DEFAULT_STATE
    const prevActiveSessionId = get().activeSessionId
    const ownsActive = prevActiveSessionId === sessionId
    const wasVisible = ownsActive && cur.visible
    const wantsVisible = !wasVisible
    const source: BrowserPanelSource = opts?.source ?? 'manual'
    const seedUrl = opts?.url ?? null
    set((state) => ({
      activeSessionId: wantsVisible ? sessionId : state.activeSessionId,
      panels: patchPanel(state.panels, sessionId, {
        visible: wantsVisible,
        pendingShow: wantsVisible ? false : cur.pendingShow,
        lastSource: source,
        url: seedUrl ?? (cur.tabs.length > 0 ? cur.url : ''),
      }),
    }))
    try {
      if (wantsVisible) {
        await dockPresentSession(sessionId)
        const rect = get().panels[sessionId]?.anchorRect ?? { x: 0, y: 0, w: 1, h: 1 }
        await dockOpen(rect, seedUrl ?? cur.url ?? null, sessionId)
        dockRequestState().catch((err) => {
          console.warn('[browserDock] dockRequestState failed', err)
        })
      } else {
        await dockHide()
      }
    } catch (err) {
      set((state) => ({
        activeSessionId: wantsVisible
          ? (state.activeSessionId === sessionId ? prevActiveSessionId : state.activeSessionId)
          : state.activeSessionId,
        panels: patchPanel(state.panels, sessionId, { visible: wasVisible }),
      }))
      notifyDockActionFailed('browser.panel.action.toggle', err)
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
        notifyDockActionFailed('browser.panel.action.navigate', err)
      }
      return
    }
    try {
      await dockNavigate(normalized)
    } catch (err) {
      notifyDockActionFailed('browser.panel.action.navigate', err)
    }
  },

  back: async (sessionId) => {
    try {
      await dockBack()
      if (sessionId) get().appendUserAction(sessionId, { kind: 'back', detail: '' })
    } catch (err) {
      notifyDockActionFailed('browser.panel.back', err)
    }
  },

  forward: async (sessionId) => {
    try {
      await dockForward()
      if (sessionId) get().appendUserAction(sessionId, { kind: 'forward', detail: '' })
    } catch (err) {
      notifyDockActionFailed('browser.panel.forward', err)
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
      notifyDockActionFailed('browser.panel.reload', err)
    }
  },

  zoom: async (sessionId, delta) => {
    const cur = get().panels[sessionId]?.zoom ?? 1
    const next = delta === 'reset' ? 1 : Math.min(3, Math.max(0.25, cur + delta))
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { zoom: next }) }))
    try {
      await dockSetZoom(next)
    } catch (err) {
      notifyDockActionFailed('browser.panel.menu.zoom', err)
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
    const wasOwner = get().activeSessionId === sessionId
    set((state) => ({
      activeSessionId: state.activeSessionId === sessionId ? null : state.activeSessionId,
      panels: patchPanel(state.panels, sessionId, { visible: false, pickMode: false }),
    }))
    if (!wasOwner) {
      return
    }
    try {
      await dockHide()
    } catch (err) {
      console.warn('[browserDock] closeForSession dockHide failed', err)
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
      const allTabs = await dockListTabs()
      const globalActive = allTabs.find((tab) => tab.active)?.id ?? null
      const prev = get().panels[sessionId] ?? DEFAULT_STATE
      let backendPinned: number | null | undefined
      try {
        backendPinned = await dockGetTestTarget(sessionId)
      } catch (err) {
        console.warn('[browserDock] refreshTabs get test target failed', err)
      }
      const patch = buildSessionTabPatch(
        sessionId,
        allTabs,
        globalActive,
        prev,
        backendPinned,
      )
      set((state) => ({
        panels: patchPanel(state.panels, sessionId, patch),
      }))
    } catch (err) {
      console.warn('[browserDock] refreshTabs failed', err)
    }
  },

  newTab: async (sessionId, url, activate) => {
    try {
      const id = await dockNewTab(url ?? null, activate ?? true, sessionId)
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
      notifyDockActionFailed('browser.panel.tabs.new', err)
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
      notifyDockActionFailed('browser.panel.tabs.close', err)
    }
  },

  activateTab: async (sessionId, tabId) => {
    const sid = normalizeBrowserSessionId(sessionId)
    if (!sid) return
    const panelTabs = get().panels[sid]?.tabs ?? []
    if (!panelTabs.some((tab) => tab.id === tabId)) {
      const scoped = await dockListTabs(sid)
      if (!scoped.some((tab) => tab.id === tabId)) {
        await get().newTab(sid, null, true)
        return
      }
    }
    try {
      await dockActivateTab(tabId, sid)
      set((state) => ({
        panels: patchPanel(state.panels, sid, { activeTabId: tabId }),
      }))
      get().appendUserAction(sid, {
        kind: 'activate_tab',
        detail: String(tabId),
      })
    } catch (err) {
      notifyDockActionFailed('browser.panel.tabs.activate', err)
    }
  },

  setPreferredTestTab: async (sessionId, tabId) => {
    const panelTabs = get().panels[sessionId]?.tabs ?? []
    const belongs =
      panelTabs.some((tab) => tab.id === tabId) ||
      (await dockListTabs(sessionId)).some((tab) => tab.id === tabId)
    if (!belongs) {
      console.warn('[browserDock] pinTestTarget rejected foreign tab', tabId, sessionId)
      return
    }
    try {
      await dockPinTestTarget(sessionId, tabId)
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
      await dockClearTestTarget(sessionId)
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

  setPrototypeRefTab: (sessionId, tabId) => {
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { prototypeRefTabId: tabId }),
    }))
    get().appendUserAction(sessionId, {
      kind: 'set_prototype_ref',
      detail: String(tabId),
    })
  },

  clearPrototypeRefTab: (sessionId) => {
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { prototypeRefTabId: null }),
    }))
    get().appendUserAction(sessionId, {
      kind: 'clear_prototype_ref',
      detail: '',
    })
  },

  setPrototypeRefFigma: (sessionId, url) => {
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { prototypeRefFigmaUrl: url }),
    }))
    get().appendUserAction(sessionId, {
      kind: 'set_prototype_ref_figma',
      detail: url,
    })
  },

  clearPrototypeRefFigma: (sessionId) => {
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { prototypeRefFigmaUrl: null }),
    }))
    get().appendUserAction(sessionId, {
      kind: 'clear_prototype_ref_figma',
      detail: '',
    })
  },

  setColumnWidth: (sessionId, px) => {
    const next = clampColumnWidth(px)
    const workspaceKey = currentWorkspaceKey()
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, {
        columnWidth: next,
        columnWidthAuto: false,
      }),
    }))
    writeStoredColumnWidth(workspaceKey, next)
    writeStoredColumnWidthAuto(workspaceKey, false)
  },

  setColumnWidthAuto: (sessionId, auto) => {
    const workspaceKey = currentWorkspaceKey()
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { columnWidthAuto: auto }),
    }))
    writeStoredColumnWidthAuto(workspaceKey, auto)
  },

  setDrawerHeightRatio: (sessionId, ratio) => {
    const next = clampRatio(ratio, 0.15, 0.6, 0.35)
    const workspaceKey = currentWorkspaceKey()
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { drawerHeightRatio: next }),
    }))
    writeStoredDrawerRatio(workspaceKey, next)
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
    const eventSessionId =
      typeof (event as { sessionId?: unknown }).sessionId === 'string'
        ? ((event as { sessionId?: string }).sessionId as string)
        : null

    if (event.kind === 'tabs') {
      const data = event.data as {
        tabs?: BrowserDockTabInfo[]
        active?: number | null
        activeSessionId?: string | null
      }
      const allTabs = Array.isArray(data.tabs) ? data.tabs : []
      const globalActive =
        typeof data.active === 'number'
          ? data.active
          : allTabs.find((t) => t.active)?.id ?? null

      const knownSessions = new Set<string>()
      for (const tab of allTabs) {
        const sid = normalizeBrowserSessionId(tab.sessionId)
        if (sid) knownSessions.add(sid)
      }
      for (const key of Object.keys(get().panels)) {
        const sid = normalizeBrowserSessionId(key)
        if (sid) knownSessions.add(sid)
      }
      if (knownSessions.size === 0) return

      set((state) => {
        const nextPanels: Record<string, BrowserPanelState> = { ...state.panels }
        for (const sid of knownSessions) {
          const prev = state.panels[sid] ?? DEFAULT_STATE
          const patch = buildSessionTabPatch(sid, allTabs, globalActive, prev)
          nextPanels[sid] = { ...prev, ...patch }
        }
        const nextActiveSession =
          normalizeBrowserSessionId(
            typeof data.activeSessionId === 'string' ? data.activeSessionId : null,
          ) ?? state.activeSessionId
        return { panels: nextPanels, activeSessionId: nextActiveSession }
      })
      return
    }

    if (event.kind === 'visible') {
      const data = event.data as
        | { session?: string | null; source?: string }
        | null
      const sessionId =
        normalizeBrowserSessionId(
          eventSessionId ??
            (typeof data?.session === 'string' ? data.session : null),
        ) ?? normalizeBrowserSessionId(useTabStore.getState().activeTabId)
      if (!sessionId) {
        console.warn('[browserDock] dropping visible event without sessionId', event)
        return
      }
      set((state) => ({
        activeSessionId: sessionId,
        panels: patchPanel(state.panels, sessionId, {
          visible: true,
          lastSource: 'agent',
        }),
      }))
      dockRequestState().catch((err) => {
        console.warn('[browserDock] ingestEvent dockRequestState failed', err)
      })
      return
    }

    if (!eventSessionId) {
      if (event.kind !== 'dock_takeover' && event.kind !== 'dock_takeover_end') {
        console.warn(
          `[browserDock] dropping ${event.kind} event without sessionId`,
          event,
        )
      }
      return
    }
    const sessionId = eventSessionId

    if (event.kind === 'agent_action') {
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
    if (event.kind === 'console_batch') {
      const data = event.data as {
        entries?: Array<{ level: string; message: string; ts: number }>
      }
      if (Array.isArray(data.entries) && data.entries.length > 0) {
        get().appendConsoleBatch(sessionId, data.entries)
      }
      return
    }
    if (event.kind === 'network_error_batch') {
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
