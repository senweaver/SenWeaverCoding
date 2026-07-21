// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { sessionsApi } from '../api/sessions'
import { useUIStore } from './uiStore'
import { useTerminalPanelStore } from './terminalPanelStore'

const TAB_STORAGE_KEY = 'sen-open-tabs'

export const SCHEDULED_TAB_ID = '__scheduled__'

export type TabType = 'session' | 'scheduled' | 'worker'

export type Tab = {
  sessionId: string
  title: string
  type: TabType
  status: 'idle' | 'running' | 'error'
}

type PersistedTabType = TabType | 'settings'
type TabPersistence = {
  openTabs: Array<{ sessionId: string; title: string; type?: PersistedTabType }>
  activeTabId: string | null
}

let saveTabsTimer: ReturnType<typeof setTimeout> | null = null

function flushTabs(state: { tabs: Tab[]; activeTabId: string | null }) {
  const data: TabPersistence = {
    openTabs: state.tabs.map((t) => ({ sessionId: t.sessionId, title: t.title, type: t.type })),
    activeTabId: state.activeTabId,
  }
  try {
    localStorage.setItem(TAB_STORAGE_KEY, JSON.stringify(data))
  } catch {  }
}

if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', () => {
    if (saveTabsTimer !== null) {
      clearTimeout(saveTabsTimer)
      saveTabsTimer = null
      flushTabs(useTabStore.getState())
    }
  })
}

type TabStore = {
  tabs: Tab[]
  activeTabId: string | null

  openTab: (sessionId: string, title: string, type?: TabType) => void
  openWorkerTab: (workerId: string, title: string) => void
  closeTab: (sessionId: string) => void
  setActiveTab: (sessionId: string) => void
  updateTabTitle: (sessionId: string, title: string) => void
  updateTabStatus: (sessionId: string, status: Tab['status']) => void
  replaceTabSession: (oldSessionId: string, newSessionId: string) => void
  moveTab: (fromIndex: number, toIndex: number) => void

  saveTabs: () => void
  restoreTabs: () => Promise<void>
}

export const useTabStore = create<TabStore>((set, get) => ({
  tabs: [],
  activeTabId: null,

  openTab: (sessionId, title, type = 'session') => {
    useUIStore.getState().dismissChatOverlays()
    const { tabs } = get()
    const existing = tabs.find((t) => t.sessionId === sessionId)
    if (existing) {
      set({ activeTabId: sessionId })
    } else {
      set({
        tabs: [...tabs, { sessionId, title, type, status: 'idle' }],
        activeTabId: sessionId,
      })
    }
    get().saveTabs()
  },

  openWorkerTab: (workerId, title) => {
    get().openTab(workerId, title, 'worker')
  },

  closeTab: (sessionId) => {
    const { tabs, activeTabId } = get()
    const index = tabs.findIndex((t) => t.sessionId === sessionId)
    if (index < 0) return

    const newTabs = tabs.filter((t) => t.sessionId !== sessionId)
    let newActiveId = activeTabId

    if (activeTabId === sessionId) {
      if (newTabs.length === 0) {
        newActiveId = null
      } else if (index >= newTabs.length) {
        newActiveId = newTabs[newTabs.length - 1]!.sessionId
      } else {
        newActiveId = newTabs[index]!.sessionId
      }
    }

    set({ tabs: newTabs, activeTabId: newActiveId })
    get().saveTabs()
    useTerminalPanelStore.getState().removeAgentMirrorForSession(sessionId)
  },

  setActiveTab: (sessionId) => {
    useUIStore.getState().dismissChatOverlays()
    set({ activeTabId: sessionId })
    get().saveTabs()
  },

  updateTabTitle: (sessionId, title) => {
    set((s) => ({
      tabs: s.tabs.map((t) => (t.sessionId === sessionId ? { ...t, title } : t)),
    }))
    get().saveTabs()
  },

  updateTabStatus: (sessionId, status) => {
    set((s) => {
      const target = s.tabs.find((t) => t.sessionId === sessionId)
      // Skip the state update entirely when the status is unchanged: every
      // ProgressTick frame calls this, and rebuilding the tabs array on each
      // no-op forces the TabBar to re-render on every streaming iteration.
      if (!target || target.status === status) return s
      return {
        tabs: s.tabs.map((t) =>
          t.sessionId === sessionId ? { ...t, status } : t,
        ),
      }
    })
  },

  replaceTabSession: (oldSessionId, newSessionId) => {
    const { activeTabId } = get()
    set((s) => ({
      tabs: s.tabs.map((t) =>
        t.sessionId === oldSessionId ? { ...t, sessionId: newSessionId } : t,
      ),
      activeTabId: activeTabId === oldSessionId ? newSessionId : activeTabId,
    }))
    get().saveTabs()
  },

  moveTab: (fromIndex, toIndex) => {
    if (fromIndex === toIndex) return
    const { tabs } = get()
    if (fromIndex < 0 || fromIndex >= tabs.length || toIndex < 0 || toIndex >= tabs.length) return
    const newTabs = [...tabs]
    const [moved] = newTabs.splice(fromIndex, 1)
    newTabs.splice(toIndex, 0, moved!)
    set({ tabs: newTabs })
    get().saveTabs()
  },

  saveTabs: () => {
    if (saveTabsTimer !== null) return
    saveTabsTimer = setTimeout(() => {
      saveTabsTimer = null
      flushTabs(get())
    }, 300)
  },

  restoreTabs: async () => {
    try {
      const raw = localStorage.getItem(TAB_STORAGE_KEY)
      if (!raw) return

      const data = JSON.parse(raw) as TabPersistence
      if (!data.openTabs || data.openTabs.length === 0) return

      const { sessions } = await sessionsApi.list({ limit: 200 })
      const existingIds = new Set(sessions.map((s) => s.id))

      const validTabs: Tab[] = data.openTabs
        .filter((t) => {

          if (t.type === 'settings') return false
          if (t.type === 'scheduled') return true
          if (t.type === 'worker') return true
          return existingIds.has(t.sessionId)
        })
        .map((t) => {
          if (t.type === 'scheduled') {
            return { sessionId: t.sessionId, title: t.title, type: 'scheduled' as const, status: 'idle' as const }
          }
          if (t.type === 'worker') {
            return { sessionId: t.sessionId, title: t.title, type: 'worker' as const, status: 'idle' as const }
          }
          return {
            sessionId: t.sessionId,
            title: sessions.find((s) => s.id === t.sessionId)?.title || t.title,
            type: 'session' as const,
            status: 'idle' as const,
          }
        })

      if (validTabs.length === 0) return

      const activeId = data.activeTabId && validTabs.some((t) => t.sessionId === data.activeTabId)
        ? data.activeTabId
        : validTabs[0]!.sessionId

      set({ tabs: validTabs, activeTabId: activeId })
    } catch {  }
  },
}))
