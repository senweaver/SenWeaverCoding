// SPDX-License-Identifier: MIT
//
// Bottom Terminal Panel store.
// Holds visibility, height, and an ordered list of tabs (interactive
// PTY tabs + a single read-only "Agent" mirror tab fed by the
// /api/background-shell/stream broadcast).
//
// xterm.js Terminal instances themselves are never put inside the
// store (they are stateful, non-serialisable, and have their own
// lifetime). Instead the store keeps lightweight tab descriptors and
// exposes a small imperative ring buffer + writer registry so the
// agent-mirror tab can replay history when its <XtermView> mounts.

import { create } from 'zustand'

const STORAGE_KEY_OPEN = 'sen-terminal-panel-open'
const STORAGE_KEY_HEIGHT = 'sen-terminal-panel-height'

export const TERMINAL_AGENT_MIRROR_TAB_ID = 'agent-mirror'
const MIRROR_BUFFER_MAX = 4096
const HEIGHT_MIN = 120
const HEIGHT_MAX = 800
const HEIGHT_DEFAULT = 260

export type TerminalTabKind = 'pty' | 'agent-mirror'
export type TerminalTabStatus = 'starting' | 'running' | 'exited' | 'error'

export type TerminalTab = {
  id: string
  kind: TerminalTabKind
  title: string
  status: TerminalTabStatus
  cwd?: string
  sessionId?: number
}

export type AgentMirrorEvent =
  | { type: 'spawned'; id: string; command: string }
  | { type: 'chunk'; id: string; stream: 'stdout' | 'stderr'; line: string }
  | { type: 'heartbeat'; id: string; elapsedSecs: number }
  | { type: 'exited'; id: string; elapsedSecs: number; exitCode: number | null }

const mirrorBuffer: string[] = []
const mirrorWriters = new Set<(chunk: string) => void>()

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
    /* ignore quota errors */
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
  ensureAgentMirrorTab: () => string
  appendAgentMirrorEvent: (event: AgentMirrorEvent) => void
  clearAgentMirror: () => void
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

  ensureAgentMirrorTab: () => {
    const existing = get().tabs.find((t) => t.id === TERMINAL_AGENT_MIRROR_TAB_ID)
    if (existing) return existing.id
    const tab: TerminalTab = {
      id: TERMINAL_AGENT_MIRROR_TAB_ID,
      kind: 'agent-mirror',
      title: '',
      status: 'running',
    }
    set((s) => ({
      tabs: [tab, ...s.tabs],
      activeTabId: s.activeTabId ?? TERMINAL_AGENT_MIRROR_TAB_ID,
    }))
    return TERMINAL_AGENT_MIRROR_TAB_ID
  },

  appendAgentMirrorEvent: (event) => {
    get().ensureAgentMirrorTab()
    const text = formatMirrorEvent(event)
    if (!text) return
    pushMirrorChunk(text)
  },

  clearAgentMirror: () => {
    mirrorBuffer.length = 0
    mirrorWriters.forEach((w) => {
      try {
        w('\x1b[2J\x1b[H')
      } catch {
        /* writer may have been disposed */
      }
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

function pushMirrorChunk(text: string) {
  mirrorBuffer.push(text)
  while (mirrorBuffer.length > MIRROR_BUFFER_MAX) mirrorBuffer.shift()
  mirrorWriters.forEach((w) => {
    try {
      w(text)
    } catch {
      /* swallow writer errors so one stale ref does not block others */
    }
  })
}

export function readMirrorBuffer(): string[] {
  return mirrorBuffer.slice()
}

export function registerMirrorWriter(write: (chunk: string) => void): () => void {
  mirrorWriters.add(write)
  return () => {
    mirrorWriters.delete(write)
  }
}
