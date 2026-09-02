// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { useLanShareStore } from './lanShareStore'
import { useReviewPanelStore } from './reviewPanelStore'
import type { ThemeMode } from '../types/settings'

const THEME_STORAGE_KEY = 'sen-theme'
const RIGHT_SIDEBAR_OPEN_KEY = 'sen-right-sidebar-open'
const RIGHT_SIDEBAR_WIDTH_KEY = 'sen-right-sidebar-width'
const RIGHT_SIDEBAR_WIDTH_AUTO_KEY = 'sen-right-sidebar-width-auto'

const RIGHT_SIDEBAR_MIN_WIDTH = 240
const RIGHT_SIDEBAR_DEFAULT_WIDTH = 360
const RIGHT_SIDEBAR_ABSOLUTE_MAX_WIDTH = 4096
const MAIN_AREA_MIN_WIDTH = 360

function measureViewportWidth(): number {
  if (typeof window === 'undefined') return RIGHT_SIDEBAR_ABSOLUTE_MAX_WIDTH
  const inner = window.innerWidth
  if (Number.isFinite(inner) && inner > 0) return inner
  return RIGHT_SIDEBAR_ABSOLUTE_MAX_WIDTH
}

function measureElementWidth(selector: string): number {
  if (typeof document === 'undefined') return 0
  const el = document.querySelector(selector) as HTMLElement | null
  if (!el) return 0
  const rect = el.getBoundingClientRect()
  return Number.isFinite(rect.width) ? rect.width : 0
}

export function getRightSidebarMaxWidth(): number {
  const viewport = measureViewportWidth()
  const leftWidth = measureElementWidth('[data-testid="sidebar-shell"]')
  const browserWidth = measureElementWidth('[data-testid="embedded-browser-panel"]')
  const available = viewport - leftWidth - browserWidth - MAIN_AREA_MIN_WIDTH
  const upper = Math.min(RIGHT_SIDEBAR_ABSOLUTE_MAX_WIDTH, Math.floor(available))
  return Math.max(RIGHT_SIDEBAR_MIN_WIDTH, upper)
}

function getStoredTheme(): ThemeMode {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY)
    if (stored === 'light' || stored === 'dark') return stored
  } catch {  }
  return 'light'
}

function getStoredRightSidebarOpen(): boolean {
  try {
    const stored = localStorage.getItem(RIGHT_SIDEBAR_OPEN_KEY)
    if (stored === 'true') return true
    if (stored === 'false') return false
  } catch {  }
  return false
}

function getStoredRightSidebarWidth(): number {
  try {
    const stored = localStorage.getItem(RIGHT_SIDEBAR_WIDTH_KEY)
    if (stored) {
      const value = Number.parseInt(stored, 10)
      if (Number.isFinite(value)) {
        return clampRightSidebarWidth(value)
      }
    }
  } catch {  }
  return RIGHT_SIDEBAR_DEFAULT_WIDTH
}

function getStoredRightSidebarWidthAuto(): boolean {
  try {
    const stored = localStorage.getItem(RIGHT_SIDEBAR_WIDTH_AUTO_KEY)
    if (stored === 'false') return false
    if (stored === 'true') return true
  } catch {  }
  return true
}

function clampRightSidebarWidth(value: number): number {
  if (!Number.isFinite(value)) return RIGHT_SIDEBAR_DEFAULT_WIDTH
  const dynamicMax = getRightSidebarMaxWidth()
  const rounded = Math.round(value)
  return Math.min(dynamicMax, Math.max(RIGHT_SIDEBAR_MIN_WIDTH, rounded))
}

export const RIGHT_SIDEBAR_BOUNDS = {
  min: RIGHT_SIDEBAR_MIN_WIDTH,
  default: RIGHT_SIDEBAR_DEFAULT_WIDTH,
  mainAreaMin: MAIN_AREA_MIN_WIDTH,
  absoluteMax: RIGHT_SIDEBAR_ABSOLUTE_MAX_WIDTH,
  get max(): number {
    return getRightSidebarMaxWidth()
  },
}

export function applyTheme(theme: ThemeMode) {
  if (typeof document === 'undefined') return
  document.documentElement.setAttribute('data-theme', theme)
  document.documentElement.style.colorScheme = theme
}

export function initializeTheme() {
  applyTheme(getStoredTheme())
}

export type ToastAction = {
  label: string
  onClick: () => void
}

export type Toast = {
  id: string
  type: 'success' | 'error' | 'warning' | 'info'
  message: string
  duration?: number
  action?: ToastAction
  sessionId?: string
  onDismiss?: () => void
}

export type SettingsTab =
  | 'providers'
  | 'agents'
  | 'general'
  | 'adapters'
  | 'custom'
  | 'security'
  | 'usage'
  | 'evolution'
  | 'plugins'
  | 'lsp'
  | 'keyboard'

export type CustomSettingsSubTab =
  | 'tools'
  | 'guardrails'
  | 'web'
  | 'mcps'
  | 'rules'
  | 'skills'

type ActiveView = 'code'

export type AppMode = 'code' | 'computer'

export type WorkspaceFinderMode = 'quick-open' | 'search-in-files' | 'workspace-symbol'

export type EditorCursor = {
  relPath: string
  line: number
  column: number
  selection?: { startLine: number; startColumn: number; endLine: number; endColumn: number } | null
  selectedCharCount?: number
}

type UIStore = {
  theme: ThemeMode
  sidebarOpen: boolean
  rightSidebarOpen: boolean
  rightSidebarWidth: number
  rightSidebarWidthAuto: boolean
  activeView: ActiveView
  appMode: AppMode

  settingsOverlayOpen: boolean
  pendingSettingsTab: SettingsTab | null
  pendingCustomSubTab: CustomSettingsSubTab | null
  templateLibraryOpen: boolean
  activeModal: string | null
  workspaceFinderMode: WorkspaceFinderMode | null
  workspaceFinderScopeDir: string | null
  closePromptOpen: boolean
  safeExiting: boolean
  editorCursor: EditorCursor | null
  editorCloseRequest: { relPath: string; nonce: number } | null
  toasts: Toast[]

  setTheme: (theme: ThemeMode) => void
  toggleTheme: () => void
  toggleSidebar: () => void
  setSidebarOpen: (open: boolean) => void
  toggleRightSidebar: () => void
  setRightSidebarOpen: (open: boolean) => void
  setRightSidebarWidth: (px: number) => void
  setRightSidebarWidthAuto: (auto: boolean) => void
  setActiveView: (view: ActiveView) => void
  setAppMode: (mode: AppMode) => void
  toggleAppMode: () => void
  openSettingsOverlay: (tab?: SettingsTab) => void
  closeSettingsOverlay: () => void
  toggleSettingsOverlay: (tab?: SettingsTab) => void
  setPendingSettingsTab: (tab: SettingsTab | null) => void
  setPendingCustomSubTab: (subTab: CustomSettingsSubTab | null) => void
  openTemplateLibrary: () => void
  closeTemplateLibrary: () => void
  toggleTemplateLibrary: () => void
  openLanSharePanel: () => void
  toggleLanSharePanel: () => void
  openReviewPanel: (sessionId: string) => void
  openModal: (id: string) => void
  closeModal: () => void
  openWorkspaceFinder: (
    mode: WorkspaceFinderMode,
    opts?: { scopeDir?: string },
  ) => void
  closeWorkspaceFinder: () => void
  dismissChatOverlays: () => void
  setClosePromptOpen: (open: boolean) => void
  setSafeExiting: (active: boolean) => void
  setEditorCursor: (cursor: EditorCursor | null) => void
  requestEditorTabClose: (relPath: string) => void
  clearEditorCloseRequest: () => void
  addToast: (toast: Omit<Toast, 'id'>) => void
  removeToast: (id: string) => void
}

let toastCounter = 0
const MAX_TOASTS = 5
const toastTimers = new Map<string, ReturnType<typeof setTimeout>>()

const SIDEBAR_WIDTH_PERSIST_DEBOUNCE_MS = 300
let sidebarWidthPersistTimer: ReturnType<typeof setTimeout> | null = null

function schedulePersistRightSidebarWidth(get: () => UIStore) {
  if (sidebarWidthPersistTimer) clearTimeout(sidebarWidthPersistTimer)
  sidebarWidthPersistTimer = setTimeout(() => {
    sidebarWidthPersistTimer = null
    const state = get()
    try {
      localStorage.setItem(RIGHT_SIDEBAR_WIDTH_KEY, String(state.rightSidebarWidth))
    } catch {  }
    try {
      localStorage.setItem(
        RIGHT_SIDEBAR_WIDTH_AUTO_KEY,
        state.rightSidebarWidthAuto ? 'true' : 'false',
      )
    } catch {  }
  }, SIDEBAR_WIDTH_PERSIST_DEBOUNCE_MS)
}

function editorCursorEquals(a: EditorCursor | null, b: EditorCursor | null): boolean {
  if (a === b) return true
  if (!a || !b) return false
  if (
    a.relPath !== b.relPath ||
    a.line !== b.line ||
    a.column !== b.column ||
    (a.selectedCharCount ?? 0) !== (b.selectedCharCount ?? 0)
  ) {
    return false
  }
  const sa = a.selection ?? null
  const sb = b.selection ?? null
  if (sa === sb) return true
  if (!sa || !sb) return false
  return (
    sa.startLine === sb.startLine &&
    sa.startColumn === sb.startColumn &&
    sa.endLine === sb.endLine &&
    sa.endColumn === sb.endColumn
  )
}

function clearToastTimer(id: string) {
  const timer = toastTimers.get(id)
  if (timer) {
    clearTimeout(timer)
    toastTimers.delete(id)
  }
}

type FullscreenOverlay = 'settings' | 'templateLibrary' | 'lanShare' | 'review'

function closeSiblingOverlays(keep: FullscreenOverlay): Partial<UIStore> {
  const next: Partial<UIStore> = {}
  if (keep !== 'settings') {
    next.settingsOverlayOpen = false
    next.pendingSettingsTab = null
    next.pendingCustomSubTab = null
  }
  if (keep !== 'templateLibrary') {
    next.templateLibraryOpen = false
  }
  if (keep !== 'lanShare') {
    const lanShare = useLanShareStore.getState()
    if (lanShare.panelOpen) lanShare.closePanel()
  }
  if (keep !== 'review') {
    const review = useReviewPanelStore.getState()
    if (review.open) review.closePanel()
  }
  return next
}

export const useUIStore = create<UIStore>((set, get) => ({
  theme: getStoredTheme(),
  sidebarOpen: true,
  rightSidebarOpen: getStoredRightSidebarOpen(),
  rightSidebarWidth: getStoredRightSidebarWidth(),
  rightSidebarWidthAuto: getStoredRightSidebarWidthAuto(),
  activeView: 'code',
  appMode: 'code',
  settingsOverlayOpen: false,
  pendingSettingsTab: null,
  pendingCustomSubTab: null,
  templateLibraryOpen: false,
  activeModal: null,
  workspaceFinderMode: null,
  workspaceFinderScopeDir: null,
  closePromptOpen: false,
  safeExiting: false,
  editorCursor: null,
  editorCloseRequest: null,
  toasts: [],

  setTheme: (theme) => {
    applyTheme(theme)
    try { localStorage.setItem(THEME_STORAGE_KEY, theme) } catch {  }
    set({ theme })
  },

  toggleTheme: () => {
    set((state) => {
      const next = state.theme === 'light' ? 'dark' : 'light'
      applyTheme(next)
      try { localStorage.setItem(THEME_STORAGE_KEY, next) } catch {  }
      return { theme: next }
    })
  },

  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  setSidebarOpen: (open) => set({ sidebarOpen: open }),

  toggleRightSidebar: () => set((s) => {
    const next = !s.rightSidebarOpen
    try { localStorage.setItem(RIGHT_SIDEBAR_OPEN_KEY, next ? 'true' : 'false') } catch {  }
    return { rightSidebarOpen: next }
  }),
  setRightSidebarOpen: (open) => {
    try { localStorage.setItem(RIGHT_SIDEBAR_OPEN_KEY, open ? 'true' : 'false') } catch {  }
    set({ rightSidebarOpen: open })
  },
  setRightSidebarWidth: (px) => {
    const clamped = clampRightSidebarWidth(px)
    set({ rightSidebarWidth: clamped, rightSidebarWidthAuto: false })
    schedulePersistRightSidebarWidth(get)
  },

  setRightSidebarWidthAuto: (auto) => {
    set({ rightSidebarWidthAuto: auto })
    schedulePersistRightSidebarWidth(get)
  },

  setActiveView: (view) => set({ activeView: view }),
  setAppMode: (mode) => set({ appMode: mode }),
  toggleAppMode: () => set((s) => ({ appMode: s.appMode === 'computer' ? 'code' : 'computer' })),
  openSettingsOverlay: (tab) => {
    const siblings = closeSiblingOverlays('settings')
    set((state) => ({
      ...siblings,
      settingsOverlayOpen: true,
      pendingSettingsTab: tab ?? state.pendingSettingsTab,
    }))
  },
  closeSettingsOverlay: () =>
    set({ settingsOverlayOpen: false, pendingSettingsTab: null, pendingCustomSubTab: null }),
  toggleSettingsOverlay: (tab) => {
    if (get().settingsOverlayOpen) {
      set({
        settingsOverlayOpen: false,
        pendingSettingsTab: null,
        pendingCustomSubTab: null,
      })
      return
    }
    const siblings = closeSiblingOverlays('settings')
    set((state) => ({
      ...siblings,
      settingsOverlayOpen: true,
      pendingSettingsTab: tab ?? state.pendingSettingsTab,
    }))
  },
  setPendingSettingsTab: (tab) => set({ pendingSettingsTab: tab }),
  setPendingCustomSubTab: (subTab) => set({ pendingCustomSubTab: subTab }),
  openTemplateLibrary: () => {
    const siblings = closeSiblingOverlays('templateLibrary')
    set({ ...siblings, templateLibraryOpen: true })
  },
  closeTemplateLibrary: () => set({ templateLibraryOpen: false }),
  toggleTemplateLibrary: () => {
    if (get().templateLibraryOpen) {
      set({ templateLibraryOpen: false })
      return
    }
    const siblings = closeSiblingOverlays('templateLibrary')
    set({ ...siblings, templateLibraryOpen: true })
  },
  openLanSharePanel: () => {
    const siblings = closeSiblingOverlays('lanShare')
    set(siblings)
    useLanShareStore.getState().openPanel()
  },
  toggleLanSharePanel: () => {
    const lanShare = useLanShareStore.getState()
    if (lanShare.panelOpen) {
      lanShare.closePanel()
      return
    }
    const siblings = closeSiblingOverlays('lanShare')
    set(siblings)
    lanShare.openPanel()
  },
  openReviewPanel: (sessionId) => {
    const siblings = closeSiblingOverlays('review')
    set(siblings)
    useReviewPanelStore.getState().openPanel(sessionId)
  },
  openModal: (id) => set({ activeModal: id }),
  closeModal: () => set({ activeModal: null }),

  openWorkspaceFinder: (mode, opts) => {
    try {
      localStorage.setItem(RIGHT_SIDEBAR_OPEN_KEY, 'true')
    } catch {
    }
    set({
      workspaceFinderMode: mode,
      workspaceFinderScopeDir: opts?.scopeDir?.trim() ? opts.scopeDir : null,
      rightSidebarOpen: true,
    })
  },
  closeWorkspaceFinder: () =>
    set({ workspaceFinderMode: null, workspaceFinderScopeDir: null }),

  dismissChatOverlays: () => {
    const lanShare = useLanShareStore.getState()
    if (lanShare.panelOpen) lanShare.closePanel()
    const review = useReviewPanelStore.getState()
    if (review.open) review.closePanel()
    set((s) => {
      if (
        !s.settingsOverlayOpen &&
        !s.templateLibraryOpen &&
        s.workspaceFinderMode === null &&
        s.activeModal === null
      ) {
        return s
      }
      const next: Partial<UIStore> = {}
      if (s.settingsOverlayOpen) {
        next.settingsOverlayOpen = false
        next.pendingSettingsTab = null
        next.pendingCustomSubTab = null
      }
      if (s.templateLibraryOpen) {
        next.templateLibraryOpen = false
      }
      if (s.workspaceFinderMode !== null) {
        next.workspaceFinderMode = null
      }
      if (s.activeModal !== null) {
        next.activeModal = null
      }
      return next
    })
  },

  setClosePromptOpen: (open) => set({ closePromptOpen: open }),
  setSafeExiting: (active) => set({ safeExiting: active }),

  setEditorCursor: (cursor) =>
    set((s) => (editorCursorEquals(s.editorCursor, cursor) ? s : { editorCursor: cursor })),

  requestEditorTabClose: (relPath) =>
    set((s) => ({
      editorCloseRequest: {
        relPath,
        nonce: (s.editorCloseRequest?.nonce ?? 0) + 1,
      },
    })),
  clearEditorCloseRequest: () => set({ editorCloseRequest: null }),

  addToast: (toast) => {
    let toastId = ''
    set((s) => {
      const duplicate = s.toasts.find(
        (t) =>
          t.type === toast.type &&
          t.message === toast.message &&
          t.sessionId === toast.sessionId,
      )
      if (duplicate) {
        toastId = duplicate.id
        return {
          toasts: s.toasts.map((t) =>
            t.id === duplicate.id ? { ...toast, id: duplicate.id } : t,
          ),
        }
      }
      const id = `toast-${++toastCounter}`
      toastId = id
      const next = [...s.toasts, { ...toast, id }]
      while (next.length > MAX_TOASTS) {
        const evicted = next.shift()
        if (evicted) {
          clearToastTimer(evicted.id)
          try {
            evicted.onDismiss?.()
          } catch {
          }
        }
      }
      return { toasts: next }
    })

    clearToastTimer(toastId)
    const duration = toast.duration ?? 4000
    if (duration > 0) {
      const id = toastId
      toastTimers.set(
        id,
        setTimeout(() => {
          toastTimers.delete(id)
          const existing = get().toasts.find((t) => t.id === id)
          if (existing) {
            try {
              existing.onDismiss?.()
            } catch {
            }
            set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }))
          }
        }, duration),
      )
    }
  },

  removeToast: (id) =>
    set((s) => {
      clearToastTimer(id)
      const target = s.toasts.find((t) => t.id === id)
      if (target?.onDismiss) {
        try {
          target.onDismiss()
        } catch {
        }
      }
      return { toasts: s.toasts.filter((t) => t.id !== id) }
    }),
}))
