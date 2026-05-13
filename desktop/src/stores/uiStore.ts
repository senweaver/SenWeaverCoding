import { create } from 'zustand'
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
  onDismiss?: () => void
}

export type SettingsTab =
  | 'providers'
  | 'agents'
  | 'codingMode'
  | 'general'
  | 'adapters'
  | 'mcp'
  | 'skills'
  | 'hooks'
  | 'usage'
  | 'evolution'
  | 'plugins'
  | 'lsp'
  | 'keyboard'
  | 'credentials'

type ActiveView = 'code' | 'scheduled' | 'terminal' | 'history' | 'settings'

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

  settingsOverlayOpen: boolean
  pendingSettingsTab: SettingsTab | null
  activeModal: string | null
  workspaceFinderMode: WorkspaceFinderMode | null
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
  openSettingsOverlay: (tab?: SettingsTab) => void
  closeSettingsOverlay: () => void
  toggleSettingsOverlay: (tab?: SettingsTab) => void
  setPendingSettingsTab: (tab: SettingsTab | null) => void
  openModal: (id: string) => void
  closeModal: () => void
  openWorkspaceFinder: (mode: WorkspaceFinderMode) => void
  closeWorkspaceFinder: () => void
  setEditorCursor: (cursor: EditorCursor | null) => void
  requestEditorTabClose: (relPath: string) => void
  clearEditorCloseRequest: () => void
  addToast: (toast: Omit<Toast, 'id'>) => void
  removeToast: (id: string) => void
}

let toastCounter = 0

export const useUIStore = create<UIStore>((set, get) => ({
  theme: getStoredTheme(),
  sidebarOpen: true,
  rightSidebarOpen: getStoredRightSidebarOpen(),
  rightSidebarWidth: getStoredRightSidebarWidth(),
  rightSidebarWidthAuto: getStoredRightSidebarWidthAuto(),
  activeView: 'code',
  settingsOverlayOpen: false,
  pendingSettingsTab: null,
  activeModal: null,
  workspaceFinderMode: null,
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
    try { localStorage.setItem(RIGHT_SIDEBAR_WIDTH_KEY, String(clamped)) } catch {  }
    try { localStorage.setItem(RIGHT_SIDEBAR_WIDTH_AUTO_KEY, 'false') } catch {  }
    set({ rightSidebarWidth: clamped, rightSidebarWidthAuto: false })
  },

  setRightSidebarWidthAuto: (auto) => {
    try { localStorage.setItem(RIGHT_SIDEBAR_WIDTH_AUTO_KEY, auto ? 'true' : 'false') } catch {  }
    set({ rightSidebarWidthAuto: auto })
  },

  setActiveView: (view) => set({ activeView: view }),
  openSettingsOverlay: (tab) =>
    set((state) => ({
      settingsOverlayOpen: true,
      pendingSettingsTab: tab ?? state.pendingSettingsTab,
    })),
  closeSettingsOverlay: () => set({ settingsOverlayOpen: false, pendingSettingsTab: null }),
  toggleSettingsOverlay: (tab) =>
    set((state) => {
      if (state.settingsOverlayOpen) {
        return { settingsOverlayOpen: false, pendingSettingsTab: null }
      }
      return {
        settingsOverlayOpen: true,
        pendingSettingsTab: tab ?? state.pendingSettingsTab,
      }
    }),
  setPendingSettingsTab: (tab) => set({ pendingSettingsTab: tab }),
  openModal: (id) => set({ activeModal: id }),
  closeModal: () => set({ activeModal: null }),

  openWorkspaceFinder: (mode) => {
    try {
      localStorage.setItem(RIGHT_SIDEBAR_OPEN_KEY, 'true')
    } catch {
      /* ignore */
    }
    set({ workspaceFinderMode: mode, rightSidebarOpen: true })
  },
  closeWorkspaceFinder: () => set({ workspaceFinderMode: null }),

  setEditorCursor: (cursor) => set({ editorCursor: cursor }),

  requestEditorTabClose: (relPath) =>
    set((s) => ({
      editorCloseRequest: {
        relPath,
        nonce: (s.editorCloseRequest?.nonce ?? 0) + 1,
      },
    })),
  clearEditorCloseRequest: () => set({ editorCloseRequest: null }),

  addToast: (toast) => {
    const id = `toast-${++toastCounter}`
    set((s) => ({ toasts: [...s.toasts, { ...toast, id }] }))

    const duration = toast.duration ?? 4000
    if (duration > 0) {
      setTimeout(() => {
        const existing = get().toasts.find((t) => t.id === id)
        if (existing) {
          existing.onDismiss?.()
          set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }))
        }
      }, duration)
    }
  },

  removeToast: (id) =>
    set((s) => {
      const target = s.toasts.find((t) => t.id === id)
      if (target?.onDismiss) {
        try {
          target.onDismiss()
        } catch {
          /* noop */
        }
      }
      return { toasts: s.toasts.filter((t) => t.id !== id) }
    }),
}))
