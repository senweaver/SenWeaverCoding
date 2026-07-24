// SPDX-License-Identifier: MIT

import { create } from 'zustand'

const STORAGE_KEY_OPEN = 'sen-terminal-panel-open'
const STORAGE_KEY_HEIGHT = 'sen-terminal-panel-height'

export const TERMINAL_AGENT_MIRROR_TAB_ID = 'agent-mirror'
export const TERMINAL_AGENT_MIRROR_GLOBAL_BUCKET = '__global__'
const MIRROR_BUFFER_MAX = 4096
const HEIGHT_MIN = 120
const HEIGHT_MAX = 800
const HEIGHT_DEFAULT = 260

export function mirrorTabIdForSession(sessionId: string | null | undefined): string {
  if (!sessionId) return TERMINAL_AGENT_MIRROR_TAB_ID
  return `${TERMINAL_AGENT_MIRROR_TAB_ID}:${sessionId}`
}

export function sessionIdFromMirrorTabId(tabId: string): string | null {
  if (tabId === TERMINAL_AGENT_MIRROR_TAB_ID) return null
  if (tabId.startsWith(`${TERMINAL_AGENT_MIRROR_TAB_ID}:`)) {
    return tabId.slice(TERMINAL_AGENT_MIRROR_TAB_ID.length + 1)
  }
  return null
}

export type TerminalTabKind = 'pty' | 'agent-mirror'
export type TerminalTabStatus = 'starting' | 'running' | 'exited' | 'error'

export type TerminalTab = {
  id: string
  kind: TerminalTabKind
  title: string
  status: TerminalTabStatus
  cwd?: string
  sessionId?: number
  interacted?: boolean
}

export type AgentMirrorEvent =
  | { type: 'spawned'; id: string; command: string; sessionId?: string | null }
  | { type: 'chunk'; id: string; stream: 'stdout' | 'stderr'; line: string; sessionId?: string | null }
  | { type: 'heartbeat'; id: string; elapsedSecs: number; sessionId?: string | null }
  | { type: 'exited'; id: string; elapsedSecs: number; exitCode: number | null; sessionId?: string | null }

const mirrorBuffersBySession = new Map<string, string[]>()
const mirrorWritersBySession = new Map<string, Set<(chunk: string) => void>>()

function bucketKey(sessionId: string | null | undefined): string {
  return sessionId ?? TERMINAL_AGENT_MIRROR_GLOBAL_BUCKET
}

function getOrCreateMirrorBuffer(sessionId: string | null | undefined): string[] {
  const key = bucketKey(sessionId)
  let buf = mirrorBuffersBySession.get(key)
  if (!buf) {
    buf = []
    mirrorBuffersBySession.set(key, buf)
  }
  return buf
}

function getOrCreateMirrorWriters(
  sessionId: string | null | undefined,
): Set<(chunk: string) => void> {
  const key = bucketKey(sessionId)
  let writers = mirrorWritersBySession.get(key)
  if (!writers) {
    writers = new Set()
    mirrorWritersBySession.set(key, writers)
  }
  return writers
}

function safeRead(key: string): string | null {
  if (typeof window === 'undefined') return null
  try {
    return window.localStorage.getItem(key)
  } catch {
    return null
  }
}

function safeWrite(key: string, value: string) {
  if (typeof window === 'undefined') return
  try {
    window.localStorage.setItem(key, value)
  } catch {
  }
}

const initialOpen = safeRead(STORAGE_KEY_OPEN) === '1'
const initialHeightRaw = parseInt(safeRead(STORAGE_KEY_HEIGHT) ?? '', 10)
const initialHeight =
  Number.isFinite(initialHeightRaw) && initialHeightRaw >= HEIGHT_MIN
    ? Math.min(HEIGHT_MAX, initialHeightRaw)
    : HEIGHT_DEFAULT

let ptyTabSeq = 1
function nextPtyTabId(): string {
  return `pty-${ptyTabSeq++}`
}

type State = {
  open: boolean
  heightPx: number
  tabs: TerminalTab[]
  activeTabId: string | null

  togglePanel: () => void
  setOpen: (open: boolean) => void
  setHeight: (px: number) => void
  openNewTab: (opts?: { cwd?: string }) => string
  closeTab: (id: string) => void
  setActiveTab: (id: string) => void
  setTabSession: (id: string, sessionId: number) => void
  setTabStatus: (id: string, status: TerminalTabStatus) => void
  setTabTitle: (id: string, title: string) => void
  setTabCwd: (id: string, cwd: string) => void
  markTabInteracted: (id: string) => void
  ensureAgentMirrorTab: (sessionId?: string | null) => string
  appendAgentMirrorEvent: (event: AgentMirrorEvent) => void
  clearAgentMirror: (sessionId?: string | null) => void
  syncAgentMirrorForChatSession: (sessionId: string | null | undefined) => void
  removeAgentMirrorForSession: (sessionId: string) => void
}

export const useTerminalPanelStore = create<State>((set, get) => ({
  open: initialOpen,
  heightPx: initialHeight,
  tabs: [],
  activeTabId: null,

  togglePanel: () => {
    const next = !get().open
    safeWrite(STORAGE_KEY_OPEN, next ? '1' : '0')
    set({ open: next })
  },

  setOpen: (open) => {
    safeWrite(STORAGE_KEY_OPEN, open ? '1' : '0')
    set({ open })
  },

  setHeight: (px) => {
    const clamped = Math.max(HEIGHT_MIN, Math.min(HEIGHT_MAX, Math.round(px)))
    safeWrite(STORAGE_KEY_HEIGHT, String(clamped))
    set({ heightPx: clamped })
  },

  openNewTab: (opts) => {
    const id = nextPtyTabId()
    const tab: TerminalTab = {
      id,
      kind: 'pty',
      title: '',
      status: 'starting',
      cwd: opts?.cwd,
    }
    set((s) => ({
      tabs: [...s.tabs, tab],
      activeTabId: id,
    }))
    return id
  },

  closeTab: (id) => {
    set((s) => {
      const tabs = s.tabs.filter((t) => t.id !== id)
      let activeTabId = s.activeTabId
      if (activeTabId === id) {
        activeTabId = tabs[tabs.length - 1]?.id ?? null
      }
      return { tabs, activeTabId }
    })
  },

  setActiveTab: (id) => {
    set({ activeTabId: id })
  },

  setTabSession: (id, sessionId) => {
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.id === id ? { ...t, sessionId, status: 'running' } : t,
      ),
    }))
  },

  setTabStatus: (id, status) => {
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === id ? { ...t, status } : t)),
    }))
  },

  setTabTitle: (id, title) => {
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === id ? { ...t, title } : t)),
    }))
  },

  setTabCwd: (id, cwd) => {
    set((s) => ({
      tabs: s.tabs.map((t) => (t.id === id ? { ...t, cwd } : t)),
    }))
  },

  markTabInteracted: (id) => {
    set((s) => {
      const tab = s.tabs.find((t) => t.id === id)
      if (!tab || tab.interacted) return s
      return {
        tabs: s.tabs.map((t) => (t.id === id ? { ...t, interacted: true } : t)),
      }
    })
  },

  ensureAgentMirrorTab: (sessionId) => {
    const tabId = mirrorTabIdForSession(sessionId)
    const existing = get().tabs.find((t) => t.id === tabId)
    if (existing) return existing.id
    const tab: TerminalTab = {
      id: tabId,
      kind: 'agent-mirror',
      title: '',
      status: 'running',
    }
    set((s) => ({
      tabs: [tab, ...s.tabs],
      activeTabId: s.activeTabId ?? tabId,
    }))
    return tabId
  },

  appendAgentMirrorEvent: (event) => {
    get().ensureAgentMirrorTab(event.sessionId ?? null)
    const text = formatMirrorEvent(event)
    if (!text) return
    pushMirrorChunk(event.sessionId ?? null, text)
  },

  clearAgentMirror: (sessionId) => {
    const buf = getOrCreateMirrorBuffer(sessionId ?? null)
    buf.length = 0
    const writers = getOrCreateMirrorWriters(sessionId ?? null)
    writers.forEach((w) => {
      try {
        w('\x1b[2J\x1b[H')
      } catch {
      }
    })
  },

  syncAgentMirrorForChatSession: (sessionId) => {
    if (!sessionId) return
    const targetTabId = mirrorTabIdForSession(sessionId)
    get().ensureAgentMirrorTab(sessionId)
    const current = get().activeTabId
    if (current && sessionIdFromMirrorTabId(current) === null && current !== TERMINAL_AGENT_MIRROR_TAB_ID) {
      return
    }
    if (current === targetTabId) return
    set({ activeTabId: targetTabId })
  },

  removeAgentMirrorForSession: (sessionId) => {
    const tabId = mirrorTabIdForSession(sessionId)
    const key = bucketKey(sessionId)
    mirrorBuffersBySession.delete(key)
    mirrorWritersBySession.delete(key)
    set((s) => {
      if (!s.tabs.some((t) => t.id === tabId)) return s
      const tabs = s.tabs.filter((t) => t.id !== tabId)
      let activeTabId = s.activeTabId
      if (activeTabId === tabId) {
        activeTabId = tabs[tabs.length - 1]?.id ?? null
      }
      return { tabs, activeTabId }
    })
  },
}))

function formatMirrorEvent(event: AgentMirrorEvent): string | null {
  switch (event.type) {
    case 'spawned':
      return `\x1b[1;36m[${event.id}] $\x1b[0m ${event.command}\r\n`
    case 'chunk': {
      const stripped = event.line.replace(/\r?\n$/, '')
      const colored =
        event.stream === 'stderr'
          ? `\x1b[31m${stripped}\x1b[0m`
          : stripped
      return `${colored}\r\n`
    }
    case 'heartbeat':
      return null
    case 'exited':
      return `\x1b[2m[${event.id}] exit ${event.exitCode ?? '?'} in ${event.elapsedSecs}s\x1b[0m\r\n`
  }
}

function pushMirrorChunk(sessionId: string | null | undefined, text: string) {
  const buf = getOrCreateMirrorBuffer(sessionId)
  buf.push(text)
  while (buf.length > MIRROR_BUFFER_MAX) buf.shift()
  const writers = getOrCreateMirrorWriters(sessionId)
  writers.forEach((w) => {
    try {
      w(text)
    } catch {
    }
  })
}

export function readMirrorBuffer(sessionId?: string | null): string[] {
  const buf = mirrorBuffersBySession.get(bucketKey(sessionId ?? null))
  return buf ? buf.slice() : []
}

export function registerMirrorWriter(
  write: (chunk: string) => void,
  sessionId?: string | null,
): () => void {
  const writers = getOrCreateMirrorWriters(sessionId ?? null)
  writers.add(write)
  return () => {
    writers.delete(write)
  }
}
