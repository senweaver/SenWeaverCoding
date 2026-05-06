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
  dockClear,
  dockClose,
  dockCloseTab,
  dockHide,
  dockInspectSelector,
  dockListTabs,
  dockNavigate,
  dockNewTab,
  dockOpen,
  dockRequestState,
  dockReload,
  dockSetPickMode,
  dockSetRect,
  dockSetZoom,
  type BrowserDockEvent,
  type BrowserDockRect,
  type BrowserDockTabInfo,
} from '../lib/browserDock'

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

export type BrowserPanelState = {

  visible: boolean

  expanded: boolean

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

  lastAgentActionAt: number

  tabs: BrowserDockTabInfo[]

  activeTabId: number | null
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
  setExpanded: (sessionId: string, expanded: boolean) => void
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
}

const DEFAULT_STATE: BrowserPanelState = {
  visible: false,
  expanded: false,
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
  lastAgentActionAt: 0,
  tabs: [],
  activeTabId: null,
}

let consoleSeq = 0
let agentLogSeq = 0
const CONSOLE_RING_MAX = 200
const AGENT_LOG_RING_MAX = 200

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
    const next: BrowserPanelState = { ...DEFAULT_STATE }
    set((state) => ({ panels: { ...state.panels, [sessionId]: next } }))
    return next
  },

  setVisible: (sessionId, visible) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { visible }) })),

  setExpanded: (sessionId, expanded) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { expanded }) })),

  setAnchorRect: (sessionId, rect) => {
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { anchorRect: rect }) }))
    const owns = get().activeSessionId === sessionId
    const expanded = get().panels[sessionId]?.expanded
    if (owns && expanded) {
      void dockSetRect(rect)
    }
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
          expanded: true,
          lastSource: source,
          url: seedUrl ?? prev.url,
        }),
      }
    })
    const rect = get().panels[sessionId]?.anchorRect ?? null
    if (rect) {
      await dockOpen(rect, seedUrl)
    } else {
      await dockOpen({ x: 0, y: 0, w: 1, h: 1 }, seedUrl)
    }
    void dockRequestState()
  },

  toggle: async (sessionId, opts) => {
    const cur = get().panels[sessionId] ?? DEFAULT_STATE
    const ownsActive = get().activeSessionId === sessionId
    const wasExpanded = ownsActive && cur.expanded
    const wantsExpanded = !wasExpanded
    const source: BrowserPanelSource = opts?.source ?? 'manual'
    const seedUrl = opts?.url ?? null
    set((state) => ({
      activeSessionId: sessionId,
      panels: patchPanel(state.panels, sessionId, {
        visible: true,
        expanded: wantsExpanded,
        lastSource: source,
        url: seedUrl ?? cur.url,
      }),
    }))
    if (wantsExpanded) {
      const rect = get().panels[sessionId]?.anchorRect ?? { x: 0, y: 0, w: 1, h: 1 }
      await dockOpen(rect, seedUrl ?? cur.url ?? null)
      void dockRequestState()
    } else {
      await dockHide()
    }
  },

  navigate: async (sessionId, url) => {
    const trimmed = url.trim()
    if (!trimmed) return
    const normalized =
      /^https?:\/\//i.test(trimmed) || trimmed === 'about:blank'
        ? trimmed
        : `https://${trimmed}`
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { url: normalized }) }))
    if (get().activeSessionId !== sessionId) {
      await get().openForTool(sessionId, { source: 'manual', url: normalized })
      return
    }
    await dockNavigate(normalized)
  },

  back: async () => {
    await import('../lib/browserDock').then(({ dockBack }) => dockBack())
  },

  forward: async () => {
    await import('../lib/browserDock').then(({ dockForward }) => dockForward())
  },

  reload: async (_sessionId, hard) => {
    await dockReload(hard ?? false)
  },

  zoom: async (sessionId, delta) => {
    const cur = get().panels[sessionId]?.zoom ?? 1
    const next = delta === 'reset' ? 1 : Math.min(3, Math.max(0.25, cur + delta))
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { zoom: next }) }))
    await dockSetZoom(next)
  },

  togglePick: async (sessionId) => {
    const enabled = !(get().panels[sessionId]?.pickMode ?? false)
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { pickMode: enabled }) }))
    await dockSetPickMode(enabled)
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
    await dockInspectSelector(selector)
  },

  clearStorage: async (_sessionId, opts) => {
    await dockClear(opts)
  },

  closeForSession: async (sessionId) => {
    set((state) => ({
      activeSessionId: state.activeSessionId === sessionId ? null : state.activeSessionId,
      panels: patchPanel(state.panels, sessionId, { visible: false, expanded: false, pickMode: false }),
    }))
    await dockClose()
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
    const tabs = await dockListTabs()
    const active = tabs.find((t) => t.active)?.id ?? null
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { tabs, activeTabId: active }),
    }))
  },

  newTab: async (sessionId, url, activate) => {
    const id = await dockNewTab(url ?? null, activate ?? true)
    if (id != null && (activate ?? true)) {
      set((state) => ({
        panels: patchPanel(state.panels, sessionId, { activeTabId: id }),
      }))
    }
    return id
  },

  closeTab: async (_sessionId, tabId) => {
    await dockCloseTab(tabId)
  },

  activateTab: async (sessionId, tabId) => {
    await dockActivateTab(tabId)
    set((state) => ({
      panels: patchPanel(state.panels, sessionId, { activeTabId: tabId }),
    }))
  },

  ingestEvent: (event) => {

    const fallbackSession = (): string | null => {
      const cur = get().activeSessionId
      if (cur) return cur
      const ids = Object.keys(get().panels)
      return ids[0] ?? null
    }

    if (event.kind === 'visible') {
      const sessionId = fallbackSession()
      if (!sessionId) return
      set((state) => ({
        activeSessionId: sessionId,
        panels: patchPanel(state.panels, sessionId, {
          visible: true,
          expanded: true,
          lastSource: 'agent',
        }),
      }))

      const seedRect = get().panels[sessionId]?.anchorRect ?? {
        x: 0,
        y: 0,
        w: 1,
        h: 1,
      }
      const seedUrl = get().panels[sessionId]?.url || null
      void dockOpen(seedRect, seedUrl)
      void dockRequestState()
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
        return {
          panels: patchPanel(state.panels, sessionId, {
            tabs,
            activeTabId: active,
            url: act?.url ?? prev.url,
            liveUrl: act?.url ?? prev.liveUrl,
            title: act?.title ?? prev.title,
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
      }
      const ts = typeof data.ts === 'number' ? data.ts : Date.now()
      set((state) => {
        const prev = state.panels[sessionId] ?? DEFAULT_STATE
        const id = ++agentLogSeq
        const reqId = typeof data.reqId === 'number' ? data.reqId : id
        const kind = typeof data.kind === 'string' ? data.kind : 'unknown'
        const next = [...prev.agentLog, { id, reqId, kind, args: data.args ?? null, ts }]
        while (next.length > AGENT_LOG_RING_MAX) next.shift()
        return {
          panels: patchPanel(state.panels, sessionId, {
            agentLog: next,
            lastAgentActionAt: ts,
            visible: true,
          }),
        }
      })
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
