// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import {
  workspaceFilesApi,
  type WorkspaceCopyEvent,
  type WorkspaceWatchEvent,
} from '../api/workspaceFiles'
import type { FileTreeNode } from '../types/workspaceFile'
import { useFileHistoryStore } from './fileHistoryStore'
import { useGitStatusStore } from './gitStatusStore'
import { useLspStore } from './lspStore'
import { usePythonEnvStore } from './pythonEnvStore'
import { useUIStore } from './uiStore'
import { t } from '../i18n'

const PYTHON_TOAST_DISMISS_KEY = 'sen.pythonEnv.dismissedRoots.v1'
const PYTHON_DETECT_DEBOUNCE_MS = 1200

let pythonDetectTimer: ReturnType<typeof setTimeout> | null = null

function loadDismissedPythonRoots(): Record<string, number> {
  try {
    const raw = localStorage.getItem(PYTHON_TOAST_DISMISS_KEY)
    if (!raw) return {}
    const parsed = JSON.parse(raw)
    if (parsed && typeof parsed === 'object') {
      return parsed as Record<string, number>
    }
  } catch {
  }
  return {}
}

function rememberDismissedPythonRoot(root: string) {
  try {
    const map = loadDismissedPythonRoots()
    map[root] = Date.now()
    localStorage.setItem(PYTHON_TOAST_DISMISS_KEY, JSON.stringify(map))
  } catch {
  }
}

function isPythonRootDismissed(root: string): boolean {
  return Boolean(loadDismissedPythonRoots()[root])
}

function schedulePythonDetectionToast(root: string) {
  if (pythonDetectTimer) {
    clearTimeout(pythonDetectTimer)
    pythonDetectTimer = null
  }
  pythonDetectTimer = setTimeout(() => {
    pythonDetectTimer = null
    const py = usePythonEnvStore.getState()
    if (py.activeRoot !== root) return
    const status = py.statusByRoot[root]
    if (!status) return
    if (!status.isPythonProject) return
    if (status.isIsolated) return
    if (status.interpreterPath) return
    if (isPythonRootDismissed(root)) return
    const ui = useUIStore.getState()
    ui.addToast({
      type: 'info',
      message: t('python.toast.detectedNoVenv'),
      duration: 14000,
      action: {
        label: t('python.toast.createCta'),
        onClick: () => {
          rememberDismissedPythonRoot(root)
          void usePythonEnvStore.getState().createVenv(root)
        },
      },
      onDismiss: () => rememberDismissedPythonRoot(root),
    })
  }, PYTHON_DETECT_DEBOUNCE_MS)
}

type DirState = {
  loaded: boolean
  loading: boolean
  expanded: boolean
  error?: string
  children: FileTreeNode[]
}

export type FileBuffer = {
  original: string
  draft: string
  isDirty: boolean
  isBinary: boolean
  lossy?: boolean
  encoding: 'utf8' | 'base64'
  sizeBytes: number
  modifiedAt?: string
  mimeType?: string
  loading: boolean
  saving: boolean
  error?: string
  saveError?: string
  missing?: boolean
}

export type MonacoEditOperation = {
  range: {
    startLineNumber: number
    startColumn: number
    endLineNumber: number
    endColumn: number
  }
  text: string | null
  forceMoveMarkers?: boolean
}

export type MonacoModelHandle = {
  pushEditOperations(
    beforeCursorState: null,
    editOperations: MonacoEditOperation[],
    cursorStateComputer: (...args: unknown[]) => null,
  ): unknown | null
  getLanguageId(): string
  isDisposed?(): boolean
  dispose?(): void
}

function scheduleModelDispose(models: MonacoModelHandle[]): void {
  if (models.length === 0) return
  setTimeout(() => {
    for (const model of models) {
      try {
        if (model.isDisposed?.()) continue
        model.dispose?.()
      } catch {
      }
    }
  }, 0)
}

type Key = string

const k = (root: string | null, relPath: string) => `${root ?? ''}::${relPath}`

const AI_PENDING_WINDOW_MS = 5_000

const SELF_PENDING_WINDOW_MS = 3_000

const AI_TREE_REFRESH_DEBOUNCE_MS = 400

const aiRefreshBatch = new Set<string>()

let aiRefreshFlushTimer: ReturnType<typeof setTimeout> | null = null

const WATCH_DIR_REFRESH_DEBOUNCE_MS = 300

const watchDirRefreshBatch = new Set<string>()

let watchDirRefreshTimer: ReturnType<typeof setTimeout> | null = null

const dirRefreshInFlight = new Set<string>()

const dirRefreshQueued = new Set<string>()

const dirRefreshedAt: Record<string, number> = {}

const DIR_EXPAND_REVALIDATE_MS = 60_000

const WATCHER_RESUME_STALE_MS = 30_000

let watcherSuspendedAt: number | null = null

export const AI_FRESH_WINDOW_MS = 8_000

export type TabViewState = {
  scrollTop: number
  scrollLeft: number
  selection: {
    startLineNumber: number
    startColumn: number
    endLineNumber: number
    endColumn: number
  } | null
}

export type ClipboardItem = {
  relPath: string
  isDir: boolean
}

export type ClipboardEntry = {
  entries: ClipboardItem[]
  mode: 'copy' | 'cut'
}

function dedupeClipboardItems(items: ClipboardItem[]): ClipboardItem[] {
  const sorted = [...items]
    .filter((item) => item.relPath !== '')
    .sort((a, b) => a.relPath.length - b.relPath.length)
  const out: ClipboardItem[] = []
  for (const item of sorted) {
    const covered = out.some(
      (kept) => kept.isDir && item.relPath.startsWith(`${kept.relPath}/`),
    )
    if (!covered && !out.some((kept) => kept.relPath === item.relPath)) {
      out.push(item)
    }
  }
  return out
}

export type CopyJob = {
  fromName: string
  toDir: string
  bytesDone: number
  bytesTotal: number
  filesDone: number
  filesTotal: number
}

export type WorkspaceFilesState = {
  root: string | null
  rootEntries: FileTreeNode[]
  rootLoaded: boolean
  rootLoading: boolean
  rootError?: string
  truncated: boolean

  showHidden: boolean

  setShowHidden: (show: boolean) => void

  pendingReveal: { relPath: string; ticket: number } | null

  revealInTree: (relPath: string) => void

  consumeReveal: () => void

  dirs: Record<Key, DirState>

  files: Record<Key, FileBuffer>

  openTabs: string[]

  activeTab: string | null

  selectedRelPath: string | null

  aiPendingWrites: Record<string, number>

  selfPendingWrites: Record<string, number>

  aiModifiedAt: Record<string, number>

  externalChanged: Record<string, number>

  lastSeenContent: Record<string, string>

  monacoModels: Record<string, MonacoModelHandle>

  pendingNavigation: { relPath: string; line: number; character: number; ticket: number } | null

  requestNavigation: (relPath: string, line: number, character: number) => Promise<void>

  consumeNavigation: () => void

  tabViewStates: Record<string, TabViewState>

  setTabViewState: (relPath: string, state: TabViewState) => void

  setRoot: (root: string | null) => void
  suspendWatcher: () => void
  resumeWatcher: () => void
  refreshRoot: () => Promise<void>
  refreshAll: () => Promise<void>
  loadDirectory: (relPath: string, opts?: { force?: boolean; silent?: boolean }) => Promise<void>
  retryDirectory: (relPath: string) => Promise<void>
  ensureDirectoryLoaded: (relPath: string) => void
  setExpanded: (relPath: string, expanded: boolean) => void
  toggleExpanded: (relPath: string) => Promise<void>

  selectFile: (relPath: string) => Promise<void>
  closeFile: (relPath: string) => void
  updateDraft: (relPath: string, content: string) => void
  saveFile: (relPath: string) => Promise<void>
  reloadFile: (relPath: string) => Promise<void>

  openTab: (relPath: string) => Promise<void>

  closeTab: (relPath: string) => void

  closeAllTabs: () => void

  closeOtherTabs: (keepRelPath: string) => void

  reorderTab: (relPath: string, toIndex: number) => void

  setActiveTab: (relPath: string | null) => void

  registerAiPendingWrite: (relPath: string) => void

  notifyAiFileChanged: (relPath: string) => void

  registerSelfWrite: (relPath: string) => void

  handleWatchEvent: (event: WorkspaceWatchEvent) => void

  acknowledgeExternalChange: (relPath: string) => void

  snapshotLastSeen: (relPath: string, content: string) => void

  registerMonacoModel: (relPath: string, model: MonacoModelHandle) => void

  unregisterMonacoModel: (relPath: string, model?: MonacoModelHandle) => void

  createFile: (parentRelPath: string, name: string) => Promise<void>
  createDir: (parentRelPath: string, name: string) => Promise<void>
  rename: (relPath: string, nextRelPath: string) => Promise<void>
  remove: (relPath: string, isDir: boolean) => Promise<void>
  uploadFiles: (
    parentRelPath: string,
    files: File[],
    opts?: { overwrite?: boolean },
  ) => Promise<{ uploaded: number; conflicts: File[] }>

  clipboard: ClipboardEntry | null
  copyJob: CopyJob | null
  copyToClipboard: (items: ClipboardItem[]) => void
  cutToClipboard: (items: ClipboardItem[]) => void
  pasteInto: (targetDir: string) => Promise<void>
  cancelCopy: () => void
}

let watcherDispose: (() => void) | null = null

let cutInProgress = false

let copyAbortController: AbortController | null = null

let copyJobClearTimer: ReturnType<typeof setTimeout> | null = null

const COPY_JOB_CLEAR_DELAY_MS = 700

function joinPath(parent: string, name: string): string {
  if (!parent || parent === '' || parent === '/') return name
  if (parent.endsWith('/')) return `${parent}${name}`
  return `${parent}/${name}`
}

function parentOf(relPath: string): string {
  if (!relPath) return ''
  const idx = relPath.lastIndexOf('/')
  return idx === -1 ? '' : relPath.slice(0, idx)
}

function nameOf(relPath: string): string {
  if (!relPath) return ''
  const idx = relPath.lastIndexOf('/')
  return idx === -1 ? relPath : relPath.slice(idx + 1)
}

function readBlobAsBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onerror = () => reject(reader.error ?? new Error('read failed'))
    reader.onload = () => {
      const result = reader.result
      if (typeof result !== 'string') {
        reject(new Error('unexpected reader result'))
        return
      }
      const idx = result.indexOf(',')
      resolve(idx === -1 ? result : result.slice(idx + 1))
    }
    reader.readAsDataURL(file)
  })
}

const emptyDir: DirState = {
  loaded: false,
  loading: false,
  expanded: false,
  children: [],
}

const dirLoadEpoch: Record<string, number> = {}

const PERSIST_VERSION = 1
const PERSIST_DEBOUNCE_MS = 500

const SHOW_HIDDEN_STORAGE_KEY = 'sen-workspace-tree-show-hidden'

function loadShowHidden(): boolean {
  if (typeof window === 'undefined' || !window.localStorage) return false
  try {
    return window.localStorage.getItem(SHOW_HIDDEN_STORAGE_KEY) === 'true'
  } catch {
    return false
  }
}

const persistTimers: Record<string, ReturnType<typeof setTimeout>> = {}

type PersistedExpanded = {
  version: number
  expanded: string[]
}

function persistKeyForRoot(root: string): string {
  let encoded: string
  try {
    encoded = window.btoa(unescape(encodeURIComponent(root)))
  } catch {
    encoded = root
  }
  const safe = encoded.replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
  return `sen-workspace-tree-expanded:${safe}`
}

function loadExpandedFromLocalStorage(root: string): Record<Key, DirState> {
  if (typeof window === 'undefined' || !window.localStorage) return {}
  try {
    const raw = window.localStorage.getItem(persistKeyForRoot(root))
    if (!raw) return {}
    const parsed = JSON.parse(raw) as PersistedExpanded
    if (!parsed || parsed.version !== PERSIST_VERSION) return {}
    if (!Array.isArray(parsed.expanded)) return {}
    const dirs: Record<Key, DirState> = {}
    for (const rel of parsed.expanded) {
      if (typeof rel !== 'string') continue
      dirs[k(root, rel)] = {
        loaded: false,
        loading: false,
        expanded: true,
        children: [],
      }
    }
    return dirs
  } catch {
    return {}
  }
}

function persistExpandedToLocalStorage(
  root: string,
  dirs: Record<Key, DirState>,
) {
  if (typeof window === 'undefined' || !window.localStorage) return
  const prefix = `${root}::`
  const expanded: string[] = []
  for (const key of Object.keys(dirs)) {
    if (!key.startsWith(prefix)) continue
    const entry = dirs[key]
    if (!entry?.expanded) continue
    expanded.push(key.slice(prefix.length))
  }
  expanded.sort()
  const payload: PersistedExpanded = {
    version: PERSIST_VERSION,
    expanded,
  }
  try {
    window.localStorage.setItem(persistKeyForRoot(root), JSON.stringify(payload))
  } catch {

  }
}

function schedulePersistExpanded(
  root: string | null,
  dirs: Record<Key, DirState>,
) {
  if (!root) return
  const existing = persistTimers[root]
  if (existing) clearTimeout(existing)
  persistTimers[root] = setTimeout(() => {
    delete persistTimers[root]
    persistExpandedToLocalStorage(root, dirs)
  }, PERSIST_DEBOUNCE_MS)
}

type PersistedTabs = {
  version: number
  openTabs: string[]
  activeTab: string | null
  tabViewStates: Record<string, TabViewState>
}

function tabsPersistKeyForRoot(root: string): string {
  let encoded: string
  try {
    encoded = window.btoa(unescape(encodeURIComponent(root)))
  } catch {
    encoded = root
  }
  const safe = encoded.replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
  return `sen-workspace-tabs:${safe}`
}

function loadTabsFromLocalStorage(root: string): PersistedTabs | null {
  if (typeof window === 'undefined' || !window.localStorage) return null
  try {
    const raw = window.localStorage.getItem(tabsPersistKeyForRoot(root))
    if (!raw) return null
    const parsed = JSON.parse(raw) as PersistedTabs
    if (!parsed || parsed.version !== PERSIST_VERSION) return null
    if (!Array.isArray(parsed.openTabs)) return null
    const openTabs = parsed.openTabs.filter(
      (t): t is string => typeof t === 'string' && t.length > 0,
    )
    const activeTab =
      typeof parsed.activeTab === 'string' && openTabs.includes(parsed.activeTab)
        ? parsed.activeTab
        : null
    const tabViewStates =
      parsed.tabViewStates && typeof parsed.tabViewStates === 'object'
        ? parsed.tabViewStates
        : {}
    return { version: PERSIST_VERSION, openTabs, activeTab, tabViewStates }
  } catch {
    return null
  }
}

function persistTabsToLocalStorage(
  root: string,
  openTabs: string[],
  activeTab: string | null,
  tabViewStates: Record<string, TabViewState>,
) {
  if (typeof window === 'undefined' || !window.localStorage) return
  const scopedViewStates: Record<string, TabViewState> = {}
  for (const tab of openTabs) {
    const view = tabViewStates[tab]
    if (view) scopedViewStates[tab] = view
  }
  const payload: PersistedTabs = {
    version: PERSIST_VERSION,
    openTabs,
    activeTab,
    tabViewStates: scopedViewStates,
  }
  try {
    window.localStorage.setItem(
      tabsPersistKeyForRoot(root),
      JSON.stringify(payload),
    )
  } catch {

  }
}

let tabsPersistTimer: ReturnType<typeof setTimeout> | null = null

function scheduleTabsPersist(root: string) {
  if (tabsPersistTimer) clearTimeout(tabsPersistTimer)
  tabsPersistTimer = setTimeout(() => {
    tabsPersistTimer = null
    const s = useWorkspaceFilesStore.getState()
    if (s.root !== root) return
    persistTabsToLocalStorage(root, s.openTabs, s.activeTab, s.tabViewStates)
  }, PERSIST_DEBOUNCE_MS)
}

function flushTabsPersist(root: string, state: WorkspaceFilesState) {
  if (tabsPersistTimer) {
    clearTimeout(tabsPersistTimer)
    tabsPersistTimer = null
  }
  persistTabsToLocalStorage(root, state.openTabs, state.activeTab, state.tabViewStates)
}

function startWatcherForRoot(
  get: () => WorkspaceFilesState,
  root: string,
): () => void {
  return workspaceFilesApi.watch(
    root,
    (event) => {

      if (get().root === root) {
        get().handleWatchEvent(event)
        useGitStatusStore.getState().scheduleRefresh(root)
        useFileHistoryStore.getState().scheduleRefresh(root)
      }
    },
    () => {

    },
    () => {

      if (get().root === root) {
        void get().refreshAll()
        useGitStatusStore.getState().scheduleRefresh(root)
        useFileHistoryStore.getState().scheduleRefresh(root)
      }
    },
  )
}

export const useWorkspaceFilesStore = create<WorkspaceFilesState>((set, get) => ({
  root: null,
  rootEntries: [],
  rootLoaded: false,
  rootLoading: false,
  rootError: undefined,
  truncated: false,
  showHidden: loadShowHidden(),
  pendingReveal: null,
  dirs: {},
  files: {},
  openTabs: [],
  activeTab: null,
  selectedRelPath: null,
  aiPendingWrites: {},
  selfPendingWrites: {},
  aiModifiedAt: {},
  externalChanged: {},
  lastSeenContent: {},
  monacoModels: {},
  pendingNavigation: null,
  tabViewStates: {},
  clipboard: null,
  copyJob: null,

  setTabViewState: (relPath, state) => {
    if (!relPath) return
    set((s) => ({
      tabViewStates: { ...s.tabViewStates, [relPath]: state },
    }))
  },

  requestNavigation: async (relPath, line, character) => {
    if (!relPath) return
    const state = get()
    if (state.activeTab !== relPath) {
      try {
        await state.selectFile(relPath)
      } catch {
        return
      }
    }
    const ticket = Date.now() + Math.random()
    set({
      pendingNavigation: { relPath, line, character, ticket },
    })
  },

  consumeNavigation: () => {
    set({ pendingNavigation: null })
  },

  setShowHidden: (show: boolean) => {
    if (get().showHidden === show) return
    set({ showHidden: show })
    try {
      window.localStorage.setItem(SHOW_HIDDEN_STORAGE_KEY, show ? 'true' : 'false')
    } catch {
    }
    void get().refreshAll()
  },

  revealInTree: (relPath: string) => {
    const root = get().root
    if (!root || !relPath) return
    const parents: string[] = []
    let parent = parentOf(relPath)
    while (parent) {
      parents.unshift(parent)
      parent = parentOf(parent)
    }
    for (const dir of parents) {
      get().setExpanded(dir, true)
    }
    set({ pendingReveal: { relPath, ticket: Date.now() + Math.random() } })
  },

  consumeReveal: () => {
    set({ pendingReveal: null })
  },

  setRoot: (root) => {
    const current = get().root
    if (current === root) return
    if (current) {
      flushTabsPersist(current, get())
      const oldPrefix = `${current}::`
      for (const key of Object.keys(dirLoadEpoch)) {
        if (key.startsWith(oldPrefix)) delete dirLoadEpoch[key]
      }
    }
    if (watcherDispose) {
      watcherDispose()
      watcherDispose = null
    }
    if (aiRefreshFlushTimer) {
      clearTimeout(aiRefreshFlushTimer)
      aiRefreshFlushTimer = null
    }
    aiRefreshBatch.clear()
    if (watchDirRefreshTimer) {
      clearTimeout(watchDirRefreshTimer)
      watchDirRefreshTimer = null
    }
    watchDirRefreshBatch.clear()
    dirRefreshInFlight.clear()
    dirRefreshQueued.clear()
    watcherSuspendedAt = null
    if (current) {
      const oldPrefix = `${current}::`
      for (const key of Object.keys(dirRefreshedAt)) {
        if (key.startsWith(oldPrefix)) delete dirRefreshedAt[key]
      }
    }
    if (copyAbortController) {
      copyAbortController.abort()
      copyAbortController = null
    }
    if (copyJobClearTimer) {
      clearTimeout(copyJobClearTimer)
      copyJobClearTimer = null
    }
    try {
      useLspStore.getState().clearDiagnostics()
    } catch (err) {
      console.warn('[workspaceFiles] clearDiagnostics failed on setRoot', err)
    }
    scheduleModelDispose(Object.values(get().monacoModels))
    const restoredDirs = root ? loadExpandedFromLocalStorage(root) : {}
    const restoredTabs = root ? loadTabsFromLocalStorage(root) : null
    set({
      root,
      rootEntries: [],
      rootLoaded: false,
      rootLoading: false,
      rootError: undefined,
      truncated: false,
      dirs: restoredDirs,
      files: {},
      openTabs: restoredTabs?.openTabs ?? [],
      activeTab: null,
      selectedRelPath: null,
      aiPendingWrites: {},
      selfPendingWrites: {},
      aiModifiedAt: {},
      externalChanged: {},
      lastSeenContent: {},
      monacoModels: {},
      pendingNavigation: null,
      pendingReveal: null,
      tabViewStates: restoredTabs?.tabViewStates ?? {},
      clipboard: null,
      copyJob: null,
    })
    if (root) {
      void get().refreshRoot()
      void useGitStatusStore.getState().fetchStatus(root, { forceRefresh: true })
      void useFileHistoryStore.getState().fetchFiles(root)
      const py = usePythonEnvStore.getState()
      py.setActiveRoot(root)
      void py.refresh(root).catch((err) => {
        console.warn('[workspaceFiles] python refresh failed', err)
      })
      void py.discover(root)
      schedulePythonDetectionToast(root)
      const restoredRels: string[] = []
      const restoredPrefix = `${root}::`
      for (const key of Object.keys(restoredDirs)) {
        if (!key.startsWith(restoredPrefix)) continue
        restoredRels.push(key.slice(restoredPrefix.length))
      }
      restoredRels.sort((a, b) => a.length - b.length)
      for (const rel of restoredRels) {
        void get().loadDirectory(rel)
      }
      const restoredActive = restoredTabs?.activeTab
      if (restoredActive && (restoredTabs?.openTabs ?? []).includes(restoredActive)) {
        void get().selectFile(restoredActive).catch(() => {})
      }
      watcherDispose = startWatcherForRoot(get, root)
    }
  },

  suspendWatcher: () => {
    if (watcherDispose) {
      watcherDispose()
      watcherDispose = null
      watcherSuspendedAt = Date.now()
    }
  },

  resumeWatcher: () => {
    const root = get().root
    if (!root || watcherDispose) return
    watcherDispose = startWatcherForRoot(get, root)
    const suspendedAt = watcherSuspendedAt
    watcherSuspendedAt = null
    if (suspendedAt !== null && Date.now() - suspendedAt > WATCHER_RESUME_STALE_MS) {
      void get().refreshAll()
    }
    useGitStatusStore.getState().scheduleRefresh(root)
    useFileHistoryStore.getState().scheduleRefresh(root)
  },

  refreshRoot: async () => {
    const root = get().root
    if (!root) return
    set({ rootLoading: true, rootError: undefined })
    try {
      const tree = await workspaceFilesApi.tree({
        root,
        depth: 1,
        showHidden: get().showHidden,
      })
      if (get().root !== root) return
      dirRefreshedAt[k(root, '')] = Date.now()
      set((s) => ({
        rootEntries: reconcileChildren(s.rootEntries, tree.entries),
        rootLoaded: true,
        rootLoading: false,
        truncated: tree.truncated,
        rootError: undefined,
      }))
    } catch (err) {
      if (get().root !== root) return
      set({
        rootLoading: false,
        rootError: err instanceof Error ? err.message : String(err),
      })
    }
  },

  refreshAll: async () => {
    const root = get().root
    if (!root) return
    await get().refreshRoot()
    if (get().root !== root) return
    const prefix = `${root}::`
    const dirs = get().dirs
    const rels: string[] = []
    for (const key of Object.keys(dirs)) {
      if (!key.startsWith(prefix)) continue
      const rel = key.slice(prefix.length)
      if (!rel) continue
      const dir = dirs[key]
      if (dir?.loading) continue
      if (dir?.loaded || dir?.expanded) rels.push(rel)
    }
    rels.sort((a, b) => a.length - b.length)
    const queue = [...rels]
    const workerCount = Math.min(6, queue.length)
    if (workerCount === 0) return
    const workers = Array.from({ length: workerCount }, async () => {
      for (;;) {
        const rel = queue.shift()
        if (rel === undefined) return
        if (get().root !== root) return
        await reloadLoadedDir(get, set, root, rel)
      }
    })
    await Promise.all(workers)
  },

  loadDirectory: async (relPath: string, opts) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    const existing = get().dirs[key] ?? emptyDir
    if (!opts?.force && (existing.loaded || existing.loading)) return
    const epoch = (dirLoadEpoch[key] ?? 0) + 1
    dirLoadEpoch[key] = epoch
    const silent = opts?.silent === true
    set((s) => {
      const current = s.dirs[key] ?? emptyDir
      return {
        dirs: {
          ...s.dirs,
          [key]: {
            ...current,
            loading: true,
            expanded: silent ? current.expanded : true,
            error: undefined,
            ...(opts?.force ? { loaded: false } : {}),
          },
        },
      }
    })
    try {
      const tree = await workspaceFilesApi.tree({
        root,
        path: relPath,
        depth: 1,
        showHidden: get().showHidden,
      })
      if (get().root !== root) return
      if (dirLoadEpoch[key] !== epoch) return
      dirRefreshedAt[key] = Date.now()
      set((s) => {
        const current = s.dirs[key] ?? emptyDir
        const nextDirs = {
          ...s.dirs,
          [key]: {
            loaded: true,
            loading: false,
            expanded: current.expanded,
            children: reconcileChildren(current.children, tree.entries),
            error: undefined,
          },
        }
        if (!silent) {
          schedulePersistExpanded(root, nextDirs)
        }
        return { dirs: nextDirs }
      })
    } catch (err) {
      if (get().root !== root) return
      if (dirLoadEpoch[key] !== epoch) return
      set((s) => {
        const current = s.dirs[key] ?? emptyDir
        return {
          dirs: {
            ...s.dirs,
            [key]: {
              ...current,
              loading: false,
              loaded: false,
              error: err instanceof Error ? err.message : String(err),
            },
          },
        }
      })
    }
  },

  retryDirectory: async (relPath: string) => {
    await get().loadDirectory(relPath, { force: true })
  },

  ensureDirectoryLoaded: (relPath: string) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    const dir = get().dirs[key]
    if (!dir) return
    if (dir.error) return
    if (dir.expanded && !dir.loaded && !dir.loading) {
      void get().loadDirectory(relPath)
    }
  },

  setExpanded: (relPath: string, expanded: boolean) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    const prev = get().dirs[key]
    set((s) => {
      const current = s.dirs[key] ?? emptyDir
      const nextDirs = {
        ...s.dirs,
        [key]: {
          ...current,
          expanded,
        },
      }
      schedulePersistExpanded(root, nextDirs)
      return { dirs: nextDirs }
    })
    if (expanded && prev?.loaded && !prev.loading) {
      if (Date.now() - (dirRefreshedAt[key] ?? 0) > DIR_EXPAND_REVALIDATE_MS) {
        void reloadLoadedDir(get, set, root, relPath)
      }
    } else if (expanded && !prev?.loaded && !prev?.loading) {
      void get().loadDirectory(relPath)
    }
  },

  toggleExpanded: async (relPath: string) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    const dir = get().dirs[key] ?? emptyDir
    if (dir.loading) return
    if (!dir.loaded) {
      if (dir.expanded) {
        set((s) => {
          const current = s.dirs[key] ?? emptyDir
          const nextDirs = {
            ...s.dirs,
            [key]: { ...current, expanded: false },
          }
          schedulePersistExpanded(root, nextDirs)
          return { dirs: nextDirs }
        })
        return
      }
      await get().loadDirectory(relPath)
      return
    }
    const willExpand = !dir.expanded
    set((s) => {
      const current = s.dirs[key] ?? emptyDir
      if (current.loading) return {}
      const nextDirs = {
        ...s.dirs,
        [key]: { ...current, expanded: willExpand },
      }
      schedulePersistExpanded(root, nextDirs)
      return { dirs: nextDirs }
    })
    if (willExpand) {
      const after = get().dirs[key]
      if (
        after?.loaded &&
        !after.loading &&
        Date.now() - (dirRefreshedAt[key] ?? 0) > DIR_EXPAND_REVALIDATE_MS
      ) {
        void reloadLoadedDir(get, set, root, relPath)
      }
    }
  },

  selectFile: async (relPath: string) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    set((s) => ({
      activeTab: relPath,
      selectedRelPath: relPath,
      openTabs: s.openTabs.includes(relPath) ? s.openTabs : [...s.openTabs, relPath],

      externalChanged: stripKey(s.externalChanged, relPath),
    }))
    const existing = get().files[key]
    if (
      existing &&
      existing.original === existing.draft &&
      !existing.isDirty &&
      !existing.missing &&
      !existing.error
    ) {

      get().snapshotLastSeen(relPath, existing.original)
      return
    }
    if (existing?.loading) return
    if (existing?.isDirty) {

      get().snapshotLastSeen(relPath, existing.original)
      return
    }
    set((s) => ({
      files: {
        ...s.files,
        [key]: {
          ...(s.files[key] ?? emptyBuffer()),
          loading: true,
          error: undefined,
        },
      },
    }))
    try {
      const file = await workspaceFilesApi.readFile({ root, path: relPath })
      if (get().root !== root) return
      set((s) => {
        const prev = s.files[key]
        const draft = prev?.isDirty ? prev.draft : file.content
        return {
          files: {
            ...s.files,
            [key]: {
              original: file.content,
              draft,
              isDirty: prev?.isDirty ?? false,
              isBinary: file.isBinary,
              lossy: file.lossy,
              encoding: file.encoding,
              sizeBytes: file.sizeBytes,
              modifiedAt: file.modifiedAt,
              mimeType: file.mimeType,
              loading: false,
              saving: false,
              error: undefined,
              saveError: undefined,
              missing: false,
            },
          },

          lastSeenContent: { ...s.lastSeenContent, [relPath]: file.content },
        }
      })
    } catch (err) {
      if (get().root !== root) return
      set((s) => ({
        files: {
          ...s.files,
          [key]: {
            ...(s.files[key] ?? emptyBuffer()),
            loading: false,
            error: err instanceof Error ? err.message : String(err),
          },
        },
      }))
    }
  },

  closeFile: (relPath: string) => {

    get().closeTab(relPath)
  },

  openTab: async (relPath: string) => {
    const { openTabs } = get()
    if (!openTabs.includes(relPath)) {
      set((s) => ({ openTabs: [...s.openTabs, relPath] }))
    }
    await get().selectFile(relPath)
  },

  closeTab: (relPath: string) => {
    const root = get().root
    const key = k(root, relPath)
    const model = get().monacoModels[relPath]
    set((s) => {
      const tabs = s.openTabs.filter((t) => t !== relPath)
      let nextActive = s.activeTab
      if (s.activeTab === relPath) {
        if (tabs.length === 0) {
          nextActive = null
        } else {

          const oldIdx = s.openTabs.indexOf(relPath)
          const fallbackIdx = Math.min(oldIdx, tabs.length - 1)
          nextActive = tabs[fallbackIdx] ?? null
        }
      }
      const files = { ...s.files }
      delete files[key]
      return {
        files,
        openTabs: tabs,
        activeTab: nextActive,
        selectedRelPath: nextActive,
        aiModifiedAt: stripKey(s.aiModifiedAt, relPath),
        externalChanged: stripKey(s.externalChanged, relPath),
        lastSeenContent: stripKey(s.lastSeenContent, relPath),
        monacoModels: stripKey(s.monacoModels, relPath),
        tabViewStates: stripKey(s.tabViewStates, relPath),
      }
    })
    if (model) scheduleModelDispose([model])
  },

  closeAllTabs: () => {
    const root = get().root
    const models = Object.values(get().monacoModels)
    set((s) => {
      const files = { ...s.files }
      for (const relPath of s.openTabs) {
        delete files[k(root, relPath)]
      }
      return {
        files,
        openTabs: [],
        activeTab: null,
        selectedRelPath: null,
        aiModifiedAt: {},
        externalChanged: {},
        lastSeenContent: {},
        monacoModels: {},
        tabViewStates: {},
      }
    })
    scheduleModelDispose(models)
  },

  closeOtherTabs: (keepRelPath: string) => {
    const root = get().root
    const modelsToDispose: MonacoModelHandle[] = []
    set((s) => {
      if (!s.openTabs.includes(keepRelPath)) return {}
      const files = { ...s.files }
      let aiModifiedAt = { ...s.aiModifiedAt }
      let externalChanged = { ...s.externalChanged }
      let lastSeenContent = { ...s.lastSeenContent }
      let monacoModels = { ...s.monacoModels }
      let tabViewStates = { ...s.tabViewStates }
      for (const relPath of s.openTabs) {
        if (relPath === keepRelPath) continue
        delete files[k(root, relPath)]
        aiModifiedAt = stripKey(aiModifiedAt, relPath)
        externalChanged = stripKey(externalChanged, relPath)
        lastSeenContent = stripKey(lastSeenContent, relPath)
        const model = monacoModels[relPath]
        if (model) modelsToDispose.push(model)
        monacoModels = stripKey(monacoModels, relPath)
        tabViewStates = stripKey(tabViewStates, relPath)
      }
      return {
        files,
        openTabs: [keepRelPath],
        activeTab: keepRelPath,
        selectedRelPath: keepRelPath,
        aiModifiedAt,
        externalChanged,
        lastSeenContent,
        monacoModels,
        tabViewStates,
      }
    })
    scheduleModelDispose(modelsToDispose)
  },

  reorderTab: (relPath: string, toIndex: number) => {
    set((s) => {
      const idx = s.openTabs.indexOf(relPath)
      if (idx === -1) return {}
      const next = [...s.openTabs]
      next.splice(idx, 1)
      const clamped = Math.max(0, Math.min(toIndex, next.length))
      next.splice(clamped, 0, relPath)
      return { openTabs: next }
    })
  },

  setActiveTab: (relPath: string | null) => {
    if (relPath === null) {
      set({ activeTab: null, selectedRelPath: null })
      return
    }
    void get().selectFile(relPath)
  },

  registerAiPendingWrite: (relPath: string) => {
    if (!relPath) return
    const now = Date.now()
    set((s) => {

      const externalTs = s.externalChanged[relPath]
      if (externalTs !== undefined && now - externalTs <= AI_PENDING_WINDOW_MS) {
        return {
          externalChanged: stripKey(s.externalChanged, relPath),
          aiModifiedAt: { ...s.aiModifiedAt, [relPath]: now },
        }
      }
      return {
        aiPendingWrites: {
          ...prunePendingMap(s.aiPendingWrites, now, AI_PENDING_WINDOW_MS * 4),
          [relPath]: now,
        },
      }
    })
  },

  notifyAiFileChanged: (relPath: string) => {
    if (!relPath) return
    get().registerAiPendingWrite(relPath)
    scheduleAiFileRefresh(relPath, get, set)
  },

  registerSelfWrite: (relPath: string) => {
    if (!relPath) return
    const now = Date.now()
    set((s) => ({
      selfPendingWrites: {
        ...prunePendingMap(s.selfPendingWrites, now, SELF_PENDING_WINDOW_MS * 4),
        [relPath]: now,
      },
    }))
  },

  handleWatchEvent: (event: WorkspaceWatchEvent) => {
    const root = get().root
    if (!root) return
    if (event.kind === 'resync') {
      void get().refreshAll()
      useGitStatusStore.getState().scheduleRefresh(root)
      useFileHistoryStore.getState().scheduleRefresh(root)
      return
    }
    const relPath = event.relPath
    if (!relPath) return
    const now = Date.now()

    if (event.kind === 'renamed' && event.fromRelPath && event.fromRelPath !== relPath) {
      migrateRename(get, set, root, event.fromRelPath, relPath)
      scheduleWatchDirRefresh(parentOf(event.fromRelPath), get, set)
      const newParent = parentOf(relPath)
      if (newParent !== parentOf(event.fromRelPath)) {
        scheduleWatchDirRefresh(newParent, get, set)
      }
      useGitStatusStore.getState().scheduleRefresh(root)
      return
    }

    const aiTs = get().aiPendingWrites[relPath]
    const isAi = aiTs !== undefined && now - aiTs <= AI_PENDING_WINDOW_MS
    const selfTs = get().selfPendingWrites[relPath]
    const isSelf =
      !isAi && selfTs !== undefined && now - selfTs <= SELF_PENDING_WINDOW_MS

    const isOpen = get().openTabs.includes(relPath)
    const key = k(root, relPath)
    const buf = get().files[key]

    const willReload = isAi && isOpen && buf && !buf.isDirty && event.kind !== 'removed'

    set((s) => {
      const nextAiPending = { ...s.aiPendingWrites }
      if (isAi) delete nextAiPending[relPath]
      const nextSelfPending = isSelf
        ? stripKey(s.selfPendingWrites, relPath)
        : s.selfPendingWrites
      const nextAiModified =
        isAi && !willReload
          ? { ...s.aiModifiedAt, [relPath]: now }
          : s.aiModifiedAt
      const nextExternal =
        !isAi && !isSelf && s.openTabs.includes(relPath)
          ? { ...s.externalChanged, [relPath]: now }
          : s.externalChanged
      return {
        aiPendingWrites: nextAiPending,
        selfPendingWrites: nextSelfPending,
        aiModifiedAt: nextAiModified,
        externalChanged: nextExternal,
      }
    })

    if (event.kind === 'removed') {
      const removedKey = key
      const removedKeyPrefix = `${removedKey}/`
      set((s) => {
        let hasDirEntry = false
        for (const dk of Object.keys(s.dirs)) {
          if (dk === removedKey || dk.startsWith(removedKeyPrefix)) {
            hasDirEntry = true
            break
          }
        }
        if (!hasDirEntry) return {}
        const dirs: Record<Key, DirState> = {}
        for (const [dk, dv] of Object.entries(s.dirs)) {
          if (dk === removedKey || dk.startsWith(removedKeyPrefix)) continue
          dirs[dk] = dv
        }
        schedulePersistExpanded(root, dirs)
        return { dirs }
      })
    }

    if (event.kind === 'created' || event.kind === 'removed' || event.kind === 'renamed') {
      const parent = parentOf(relPath)
      const parentKey = k(root, parent)
      const parentDir = get().dirs[parentKey]
      if (parent === '' || parentDir?.loaded) {
        scheduleWatchDirRefresh(parent, get, set)
      }
    }

    if (isOpen && !isSelf) {
      if (event.kind === 'removed') {
        if (buf) {
          set((s) => ({
            externalChanged: { ...s.externalChanged, [relPath]: Date.now() },
            files: {
              ...s.files,
              [k(root, relPath)]: {
                ...buf,
                missing: true,
              },
            },
          }))
        }
        return
      }
      if (buf && !buf.isDirty) {
        if (isAi) {
          aiRefreshBatch.delete(relPath)
        }
        void reloadBuffer(get, set, relPath).then(() => {

          if (isAi) {
            set((s) => ({
              aiModifiedAt: { ...s.aiModifiedAt, [relPath]: Date.now() },
            }))
          }
        })
      } else if (buf?.missing && (event.kind === 'created' || event.kind === 'modified')) {
        set((s) => {
          const current = s.files[key]
          if (!current) return {}
          return {
            files: {
              ...s.files,
              [key]: { ...current, missing: false },
            },
          }
        })
      } else if (isAi) {

        set((s) => ({
          aiModifiedAt: { ...s.aiModifiedAt, [relPath]: now },
        }))
      }
    }
  },

  acknowledgeExternalChange: (relPath: string) => {
    set((s) => ({ externalChanged: stripKey(s.externalChanged, relPath) }))
  },

  snapshotLastSeen: (relPath: string, content: string) => {
    set((s) => ({ lastSeenContent: { ...s.lastSeenContent, [relPath]: content } }))
  },

  registerMonacoModel: (relPath: string, model: MonacoModelHandle) => {
    if (!relPath || !model) return
    set((s) => {
      if (s.monacoModels[relPath] === model) return {}
      return { monacoModels: { ...s.monacoModels, [relPath]: model } }
    })
  },

  unregisterMonacoModel: (relPath: string, model?: MonacoModelHandle) => {
    if (!relPath) return
    set((s) => {
      const current = s.monacoModels[relPath]
      if (!current) return {}
      if (model && current !== model) return {}
      const next = { ...s.monacoModels }
      delete next[relPath]
      return { monacoModels: next }
    })
  },

  updateDraft: (relPath: string, content: string) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    set((s) => {
      const buf = s.files[key]
      if (!buf) return {}
      if (buf.draft === content) return {}
      return {
        files: {
          ...s.files,
          [key]: {
            ...buf,
            draft: content,
            isDirty: content !== buf.original,
          },
        },
      }
    })
  },

  saveFile: async (relPath: string) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    const buf = get().files[key]
    if (!buf || buf.isBinary || buf.lossy) return

    get().registerSelfWrite(relPath)
    set((s) => ({
      files: {
        ...s.files,
        [key]: { ...buf, saving: true, saveError: undefined },
      },
    }))
    try {
      const res = await workspaceFilesApi.writeFile({
        root,
        path: relPath,
        content: buf.draft,
        ifMatchMtime: buf.missing ? undefined : buf.modifiedAt,
        encoding: 'utf8',
      })
      if (get().root !== root) return

      get().registerSelfWrite(relPath)
      set((s) => ({
        files: {
          ...s.files,
          [key]: {
            ...(s.files[key] ?? buf),
            original: buf.draft,
            isDirty: false,
            saving: false,
            modifiedAt: res.modifiedAt ?? buf.modifiedAt,
            sizeBytes: res.sizeBytes ?? buf.sizeBytes,
            missing: false,
          },
        },

        aiModifiedAt: stripKey(s.aiModifiedAt, relPath),
        externalChanged: stripKey(s.externalChanged, relPath),

        lastSeenContent: { ...s.lastSeenContent, [relPath]: buf.draft },
      }))

      const parent = parentOf(relPath)
      const parentKey = k(root, parent)
      const parentDir = get().dirs[parentKey]
      if (parentDir?.loaded) {
        try {
          const tree = await workspaceFilesApi.tree({
            root,
            path: parent,
            depth: 1,
            showHidden: get().showHidden,
          })
          if (get().root !== root) return
          dirRefreshedAt[parentKey] = Date.now()
          set((s) => ({
            dirs: {
              ...s.dirs,
              [parentKey]: {
                ...(s.dirs[parentKey] ?? emptyDir),
                children: reconcileChildren(
                  s.dirs[parentKey]?.children,
                  tree.entries,
                ),
                loaded: true,
                loading: false,
              },
            },
          }))
        } catch {

        }
      } else if (parent === '') {
        await get().refreshRoot()
      }
    } catch (err) {
      if (get().root === root) {
        set((s) => ({
          files: {
            ...s.files,
            [key]: {
              ...(s.files[key] ?? buf),
              saving: false,
              saveError: err instanceof Error ? err.message : String(err),
            },
          },
        }))
      }
      throw err
    }
  },

  reloadFile: async (relPath: string) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    set((s) => {
      const buf = s.files[key]
      if (!buf) return {}
      return {
        files: {
          ...s.files,
          [key]: { ...buf, loading: true, error: undefined },
        },
      }
    })
    try {
      const file = await workspaceFilesApi.readFile({ root, path: relPath })
      if (get().root !== root) return
      set((s) => {
        const buf = s.files[key]
        if (!buf) return {}
        return {
          files: {
            ...s.files,
            [key]: {
              original: file.content,
              draft: file.content,
              isDirty: false,
              isBinary: file.isBinary,
              lossy: file.lossy,
              encoding: file.encoding,
              sizeBytes: file.sizeBytes,
              modifiedAt: file.modifiedAt,
              mimeType: file.mimeType,
              loading: false,
              saving: false,
              missing: false,
            },
          },
          lastSeenContent: { ...s.lastSeenContent, [relPath]: file.content },
          externalChanged: stripKey(s.externalChanged, relPath),
        }
      })
    } catch (err) {
      if (get().root !== root) return
      set((s) => {
        const buf = s.files[key]
        if (!buf) return {}
        return {
          files: {
            ...s.files,
            [key]: {
              ...buf,
              loading: false,
              error: err instanceof Error ? err.message : String(err),
            },
          },
        }
      })
    }
  },

  createFile: async (parentRelPath: string, name: string) => {
    const root = get().root
    if (!root) return
    const target = joinPath(parentRelPath, name)
    get().registerSelfWrite(target)
    await workspaceFilesApi.createFile({ root, path: target })
    get().registerSelfWrite(target)
    await refreshDir(get, set, parentRelPath)
  },

  createDir: async (parentRelPath: string, name: string) => {
    const root = get().root
    if (!root) return
    const target = joinPath(parentRelPath, name)
    get().registerSelfWrite(target)
    await workspaceFilesApi.createDir({ root, path: target })
    get().registerSelfWrite(target)
    await refreshDir(get, set, parentRelPath)
  },

  rename: async (relPath: string, nextRelPath: string) => {
    const root = get().root
    if (!root) return
    if (relPath === nextRelPath) return

    get().registerSelfWrite(relPath)
    get().registerSelfWrite(nextRelPath)
    await workspaceFilesApi.move({
      root,
      fromPath: relPath,
      toPath: nextRelPath,
    })
    if (get().root !== root) return
    get().registerSelfWrite(relPath)
    get().registerSelfWrite(nextRelPath)
    migrateRename(get, set, root, relPath, nextRelPath)
    const oldParent = parentOf(relPath)
    const newParent = parentOf(nextRelPath)
    await refreshDir(get, set, oldParent)
    if (oldParent !== newParent) {
      await refreshDir(get, set, newParent)
    }
  },

  remove: async (relPath: string, isDir: boolean) => {
    const root = get().root
    if (!root) return
    get().registerSelfWrite(relPath)
    await workspaceFilesApi.remove({
      root,
      path: relPath,
      recursive: isDir,
    })
    if (get().root !== root) return
    get().registerSelfWrite(relPath)
    const key = k(root, relPath)
    const keyPrefix = `${key}/`
    const relPrefix = `${relPath}/`
    const matchesRel = (rel: string) => rel === relPath || rel.startsWith(relPrefix)
    const modelsToDispose: MonacoModelHandle[] = []
    set((s) => {
      const files: Record<Key, FileBuffer> = {}
      for (const [fk, fv] of Object.entries(s.files)) {
        if (fk === key || fk.startsWith(keyPrefix)) continue
        files[fk] = fv
      }
      const dirs: Record<Key, DirState> = {}
      for (const [dk, dv] of Object.entries(s.dirs)) {
        if (dk === key || dk.startsWith(keyPrefix)) continue
        dirs[dk] = dv
      }
      const openTabs = s.openTabs.filter((t) => !matchesRel(t))
      let activeTab = s.activeTab
      if (s.activeTab && matchesRel(s.activeTab)) {
        if (openTabs.length === 0) {
          activeTab = null
        } else {
          const oldIdx = s.openTabs.indexOf(s.activeTab)
          const fallbackIdx = Math.max(
            0,
            Math.min(oldIdx, openTabs.length - 1),
          )
          activeTab = openTabs[fallbackIdx] ?? null
        }
      }
      const stripMatching = <V,>(map: Record<string, V>): Record<string, V> => {
        const out: Record<string, V> = {}
        for (const [mk, mv] of Object.entries(map)) {
          if (matchesRel(mk)) continue
          out[mk] = mv
        }
        return out
      }
      for (const [mk, model] of Object.entries(s.monacoModels)) {
        if (matchesRel(mk)) modelsToDispose.push(model)
      }
      schedulePersistExpanded(root, dirs)
      return {
        files,
        dirs,
        openTabs,
        activeTab,
        selectedRelPath: activeTab,
        aiPendingWrites: stripMatching(s.aiPendingWrites),
        aiModifiedAt: stripMatching(s.aiModifiedAt),
        externalChanged: stripMatching(s.externalChanged),
        lastSeenContent: stripMatching(s.lastSeenContent),
        monacoModels: stripMatching(s.monacoModels),
        tabViewStates: stripMatching(s.tabViewStates),
      }
    })
    scheduleModelDispose(modelsToDispose)
    await refreshDir(get, set, parentOf(relPath))
  },

  uploadFiles: async (parentRelPath: string, files: File[], opts) => {
    const root = get().root
    if (!root || files.length === 0) return { uploaded: 0, conflicts: [] }
    const overwrite = opts?.overwrite === true
    let uploaded = 0
    const conflicts: File[] = []
    for (const file of files) {
      const base64 = await readBlobAsBase64(file)
      if (get().root !== root) break
      const relTarget = joinPath(parentRelPath, file.name)
      get().registerSelfWrite(relTarget)
      try {
        await workspaceFilesApi.upload({
          root,
          path: relTarget,
          contentBase64: base64,
          overwrite,
        })
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err)
        if (!overwrite && /exist/i.test(message)) {
          conflicts.push(file)
          continue
        }
        throw err
      }
      get().registerSelfWrite(relTarget)
      uploaded += 1
    }
    if (get().root === root && uploaded > 0) {
      await refreshDir(get, set, parentRelPath)
    }
    return { uploaded, conflicts }
  },

  copyToClipboard: (items: ClipboardItem[]) => {
    const entries = dedupeClipboardItems(items)
    if (entries.length === 0) return
    set({ clipboard: { entries, mode: 'copy' } })
    try {
      useUIStore.getState().addToast({
        type: 'success',
        message:
          entries.length === 1
            ? t('workspace.copied', { name: nameOf(entries[0]!.relPath) })
            : t('workspace.copiedMulti', { count: entries.length }),
        duration: 2200,
      })
    } catch {
    }
  },

  cutToClipboard: (items: ClipboardItem[]) => {
    const entries = dedupeClipboardItems(items)
    if (entries.length === 0) return
    set({ clipboard: { entries, mode: 'cut' } })
    try {
      useUIStore.getState().addToast({
        type: 'success',
        message:
          entries.length === 1
            ? t('workspace.cutToast', { name: nameOf(entries[0]!.relPath) })
            : t('workspace.cutToastMulti', { count: entries.length }),
        duration: 2200,
      })
    } catch {
    }
  },

  pasteInto: async (targetDir: string) => {
    const root = get().root
    const clipboard = get().clipboard
    if (!root || !clipboard || clipboard.entries.length === 0) return
    if (get().copyJob || cutInProgress) return

    const normalizedTarget = targetDir ?? ''
    const targetLabel = normalizedTarget === '' ? '/' : normalizedTarget

    if (clipboard.mode === 'cut') {
      cutInProgress = true
      try {
        const sourceParents = new Set<string>()
        const errors: string[] = []
        const succeeded = new Set<string>()
        for (const item of clipboard.entries) {
          const fromName = nameOf(item.relPath)
          const toPath =
            normalizedTarget === '' ? fromName : `${normalizedTarget}/${fromName}`
          if (toPath === item.relPath) {
            succeeded.add(item.relPath)
            continue
          }
          if (
            item.isDir &&
            (normalizedTarget === item.relPath ||
              normalizedTarget.startsWith(`${item.relPath}/`))
          ) {
            errors.push(t('workspace.moveIntoSelf'))
            continue
          }
          try {
            get().registerSelfWrite(item.relPath)
            get().registerSelfWrite(toPath)
            await workspaceFilesApi.move({ root, fromPath: item.relPath, toPath })
            if (get().root !== root) return
            get().registerSelfWrite(item.relPath)
            get().registerSelfWrite(toPath)
            migrateRename(get, set, root, item.relPath, toPath)
            sourceParents.add(parentOf(item.relPath))
            succeeded.add(item.relPath)
          } catch (err) {
            errors.push(err instanceof Error ? err.message : String(err))
          }
        }
        if (get().root !== root) return
        if (succeeded.size > 0) {
          set((s) => {
            const cb = s.clipboard
            if (!cb || cb.mode !== 'cut') return {}
            const remaining = cb.entries.filter(
              (entry) => !succeeded.has(entry.relPath),
            )
            return {
              clipboard:
                remaining.length > 0
                  ? { entries: remaining, mode: 'cut' }
                  : null,
            }
          })
          for (const parent of sourceParents) {
            if (parent !== normalizedTarget) {
              await refreshDir(get, set, parent)
            }
          }
          await refreshDir(get, set, normalizedTarget)
        }
        if (errors.length > 0) {
          try {
            useUIStore.getState().addToast({
              type: 'error',
              message: t('workspace.moveFailed', { message: errors[0] ?? '' }),
              duration: 6000,
            })
          } catch {
          }
        }
      } finally {
        cutInProgress = false
      }
      return
    }

    if (copyJobClearTimer) {
      clearTimeout(copyJobClearTimer)
      copyJobClearTimer = null
    }
    if (copyAbortController) {
      copyAbortController.abort()
    }
    const controller = new AbortController()
    copyAbortController = controller

    const entries = clipboard.entries
    const totalEntries = entries.length

    const jobName = (index: number): string => {
      const name = nameOf(entries[index]?.relPath ?? '')
      return totalEntries > 1 ? `${name} (${index + 1}/${totalEntries})` : name
    }

    set({
      copyJob: {
        fromName: jobName(0),
        toDir: targetLabel,
        bytesDone: 0,
        bytesTotal: 0,
        filesDone: 0,
        filesTotal: 0,
      },
    })

    const finishClear = () => {
      copyJobClearTimer = setTimeout(() => {
        copyJobClearTimer = null
        set({ copyJob: null })
      }, COPY_JOB_CLEAR_DELAY_MS)
    }

    try {
      let firstError: string | null = null
      let anyDone = false
      for (let i = 0; i < entries.length; i += 1) {
        const item = entries[i]
        if (!item) continue
        if (controller.signal.aborted) break
        set((s) =>
          s.copyJob
            ? {
                copyJob: {
                  ...s.copyJob,
                  fromName: jobName(i),
                  bytesDone: 0,
                  bytesTotal: 0,
                  filesDone: 0,
                  filesTotal: 0,
                },
              }
            : {},
        )
        let errorMessage: string | null = null
        let done = false
        await workspaceFilesApi.copyStream(
          {
            root,
            fromPath: item.relPath,
            toDir: normalizedTarget,
            signal: controller.signal,
          },
          (event: WorkspaceCopyEvent) => {
            if (copyAbortController !== controller) return
            if (event.type === 'progress') {
              set((s) =>
                s.copyJob
                  ? {
                      copyJob: {
                        ...s.copyJob,
                        bytesDone: event.bytesDone,
                        bytesTotal: event.bytesTotal,
                        filesDone: event.filesDone,
                        filesTotal: event.filesTotal,
                      },
                    }
                  : {},
              )
            } else if (event.type === 'done') {
              done = true
            } else if (event.type === 'error') {
              errorMessage = event.message
            }
          },
        )
        if (copyAbortController !== controller) return
        if (done && !errorMessage) {
          anyDone = true
        } else if (errorMessage && errorMessage !== 'cancelled' && !firstError) {
          firstError = errorMessage
        }
      }
      if (copyAbortController !== controller) return
      copyAbortController = null
      if (anyDone) {
        await refreshDir(get, set, normalizedTarget)
      }
      if (firstError) {
        try {
          useUIStore.getState().addToast({
            type: 'error',
            message: t('workspace.copyFailed', { message: firstError }),
            duration: 6000,
          })
        } catch {
        }
        set({ copyJob: null })
      } else if (anyDone) {
        finishClear()
      } else {
        set({ copyJob: null })
      }
    } catch (err) {
      if (copyAbortController !== controller) {
        return
      }
      copyAbortController = null
      const aborted =
        err instanceof DOMException
          ? err.name === 'AbortError'
          : err instanceof Error && err.name === 'AbortError'
      if (!aborted) {
        try {
          useUIStore.getState().addToast({
            type: 'error',
            message: t('workspace.copyFailed', {
              message: err instanceof Error ? err.message : String(err),
            }),
            duration: 6000,
          })
        } catch {
        }
      }
      set({ copyJob: null })
    }
  },

  cancelCopy: () => {
    if (copyAbortController) {
      copyAbortController.abort()
      copyAbortController = null
    }
    if (copyJobClearTimer) {
      clearTimeout(copyJobClearTimer)
      copyJobClearTimer = null
    }
    set({ copyJob: null })
  },
}))

function emptyBuffer(): FileBuffer {
  return {
    original: '',
    draft: '',
    isDirty: false,
    isBinary: false,
    encoding: 'utf8',
    sizeBytes: 0,
    loading: true,
    saving: false,
  }
}

function stripKey<V>(map: Record<string, V>, key: string): Record<string, V> {
  if (!(key in map)) return map
  const next = { ...map }
  delete next[key]
  return next
}

function prunePendingMap(
  map: Record<string, number>,
  now: number,
  maxAgeMs: number,
): Record<string, number> {
  let hasStale = false
  for (const ts of Object.values(map)) {
    if (now - ts > maxAgeMs) {
      hasStale = true
      break
    }
  }
  if (!hasStale) return map
  const next: Record<string, number> = {}
  for (const [key, ts] of Object.entries(map)) {
    if (now - ts <= maxAgeMs) next[key] = ts
  }
  return next
}

function reconcileChildren(
  prev: FileTreeNode[] | undefined,
  next: FileTreeNode[],
): FileTreeNode[] {
  if (!prev || prev.length === 0) return next
  const byPath = new Map<string, FileTreeNode>()
  for (const node of prev) byPath.set(node.relPath, node)
  let reusedAll = prev.length === next.length
  const out = next.map((node, idx) => {
    const old = byPath.get(node.relPath)
    if (
      old &&
      old.name === node.name &&
      old.isDir === node.isDir &&
      old.sizeBytes === node.sizeBytes &&
      old.modifiedAt === node.modifiedAt
    ) {
      if (reusedAll && prev[idx] !== old) reusedAll = false
      return old
    }
    reusedAll = false
    return node
  })
  return reusedAll ? prev : out
}

function scheduleWatchDirRefresh(
  parentRelPath: string,
  get: () => WorkspaceFilesState,
  set: (
    partial:
      | Partial<WorkspaceFilesState>
      | ((s: WorkspaceFilesState) => Partial<WorkspaceFilesState>),
  ) => void,
) {
  watchDirRefreshBatch.add(parentRelPath)
  if (watchDirRefreshTimer) return
  watchDirRefreshTimer = setTimeout(() => {
    watchDirRefreshTimer = null
    const parents = [...watchDirRefreshBatch]
    watchDirRefreshBatch.clear()
    for (const parent of parents) {
      void refreshDir(get, set, parent)
    }
  }, WATCH_DIR_REFRESH_DEBOUNCE_MS)
}

function scheduleAiFileRefresh(
  relPath: string,
  get: () => WorkspaceFilesState,
  set: (
    partial:
      | Partial<WorkspaceFilesState>
      | ((s: WorkspaceFilesState) => Partial<WorkspaceFilesState>),
  ) => void,
) {
  aiRefreshBatch.add(relPath)
  if (aiRefreshFlushTimer) clearTimeout(aiRefreshFlushTimer)
  aiRefreshFlushTimer = setTimeout(() => {
    aiRefreshFlushTimer = null
    void flushAiFileRefresh(get, set)
  }, AI_TREE_REFRESH_DEBOUNCE_MS)
}

async function flushAiFileRefresh(
  get: () => WorkspaceFilesState,
  set: (
    partial:
      | Partial<WorkspaceFilesState>
      | ((s: WorkspaceFilesState) => Partial<WorkspaceFilesState>),
  ) => void,
) {
  const batch = [...aiRefreshBatch]
  aiRefreshBatch.clear()
  const root = get().root
  if (!root || batch.length === 0) return

  const parents = new Set<string>()
  for (const rel of batch) {
    parents.add(parentOf(rel))
  }

  for (const parent of parents) {
    const parentKey = k(root, parent)
    const parentDir = get().dirs[parentKey]
    if (parent === '' || parentDir?.loaded) {
      await refreshDir(get, set, parent)
    }
  }

  const now = Date.now()
  for (const relPath of batch) {
    if (!get().openTabs.includes(relPath)) continue
    const key = k(root, relPath)
    const buf = get().files[key]
    if (!buf || buf.isDirty) continue
    await reloadBuffer(get, set, relPath)
    set((s) => ({
      aiModifiedAt: { ...s.aiModifiedAt, [relPath]: now },
    }))
  }
}

async function reloadBuffer(
  get: () => WorkspaceFilesState,
  set: (
    partial:
      | Partial<WorkspaceFilesState>
      | ((s: WorkspaceFilesState) => Partial<WorkspaceFilesState>),
  ) => void,
  relPath: string,
) {
  const root = get().root
  if (!root) return
  const key = k(root, relPath)
  try {
    const file = await workspaceFilesApi.readFile({ root, path: relPath })
    if (get().root !== root) return
    set((s) => {
      const buf = s.files[key]
      if (!buf) return {}

      if (buf.isDirty) return {}
      return {
        files: {
          ...s.files,
          [key]: {
            ...buf,
            original: file.content,
            draft: file.content,
            isBinary: file.isBinary,
            lossy: file.lossy,
            encoding: file.encoding,
            sizeBytes: file.sizeBytes,
            modifiedAt: file.modifiedAt,
            mimeType: file.mimeType,
            missing: false,
          },
        },
        externalChanged: stripKey(s.externalChanged, relPath),
      }
    })
  } catch {

  }
}

async function refreshDir(
  get: () => WorkspaceFilesState,
  set: (
    partial:
      | Partial<WorkspaceFilesState>
      | ((s: WorkspaceFilesState) => Partial<WorkspaceFilesState>),
  ) => void,
  relPath: string,
) {
  const root = get().root
  if (!root) return
  const key = k(root, relPath)
  if (dirRefreshInFlight.has(key)) {
    dirRefreshQueued.add(key)
    return
  }
  dirRefreshInFlight.add(key)
  try {
    if (relPath === '') {
      await get().refreshRoot()
      if (get().root === root) {
        dirRefreshedAt[key] = Date.now()
      }
      return
    }
    const existing = get().dirs[key]
    if (existing?.loading) return
    const epoch = dirLoadEpoch[key] ?? 0
    try {
      const tree = await workspaceFilesApi.tree({
        root,
        path: relPath,
        depth: 1,
        showHidden: get().showHidden,
      })
      if (get().root !== root) return
      if ((dirLoadEpoch[key] ?? 0) !== epoch) return
      dirRefreshedAt[key] = Date.now()
      set((s) => {
        const current = s.dirs[key]
        if (current?.loading) return {}
        const children = reconcileChildren(current?.children, tree.entries)
        if (
          current &&
          children === current.children &&
          current.loaded &&
          current.error === undefined
        ) {
          return {}
        }
        return {
          dirs: {
            ...s.dirs,
            [key]: {
              ...(current ?? emptyDir),
              children,
              loaded: true,
              loading: false,
              expanded: current?.expanded ?? true,
              error: undefined,
            },
          },
        }
      })
    } catch (err) {
      if (get().root !== root) return
      if ((dirLoadEpoch[key] ?? 0) !== epoch) return
      set((s) => {
        const current = s.dirs[key]
        if (!current || current.loading) return {}
        return {
          dirs: {
            ...s.dirs,
            [key]: {
              ...current,
              error: err instanceof Error ? err.message : String(err),
              loading: false,
            },
          },
        }
      })
    }
  } finally {
    dirRefreshInFlight.delete(key)
    if (dirRefreshQueued.delete(key) && get().root === root) {
      void refreshDir(get, set, relPath)
    }
  }
}

async function reloadLoadedDir(
  get: () => WorkspaceFilesState,
  set: (
    partial:
      | Partial<WorkspaceFilesState>
      | ((s: WorkspaceFilesState) => Partial<WorkspaceFilesState>),
  ) => void,
  root: string,
  relPath: string,
) {
  const key = k(root, relPath)
  const existing = get().dirs[key]
  if (!existing || existing.loading) return
  if (dirRefreshInFlight.has(key)) return
  dirRefreshInFlight.add(key)
  const epoch = dirLoadEpoch[key] ?? 0
  try {
    const tree = await workspaceFilesApi.tree({
      root,
      path: relPath,
      depth: 1,
      showHidden: get().showHidden,
    })
    if (get().root !== root) return
    if ((dirLoadEpoch[key] ?? 0) !== epoch) return
    dirRefreshedAt[key] = Date.now()
    set((s) => {
      const current = s.dirs[key]
      if (!current || current.loading) return {}
      const children = reconcileChildren(current.children, tree.entries)
      if (children === current.children && current.loaded && current.error === undefined) {
        return {}
      }
      return {
        dirs: {
          ...s.dirs,
          [key]: {
            ...current,
            children,
            loaded: true,
            loading: false,
            error: undefined,
          },
        },
      }
    })
  } catch (err) {
    if (get().root !== root) return
    if ((dirLoadEpoch[key] ?? 0) !== epoch) return
    set((s) => {
      const current = s.dirs[key]
      if (!current || current.loading) return {}
      return {
        dirs: {
          ...s.dirs,
          [key]: {
            ...current,
            loading: false,
            error: err instanceof Error ? err.message : String(err),
          },
        },
      }
    })
  } finally {
    dirRefreshInFlight.delete(key)
  }
}

function migrateRename(
  _get: () => WorkspaceFilesState,
  set: (
    partial:
      | Partial<WorkspaceFilesState>
      | ((s: WorkspaceFilesState) => Partial<WorkspaceFilesState>),
  ) => void,
  root: string,
  from: string,
  to: string,
) {
  if (!from || !to || from === to) return
  set((s) => {
    const fromPrefix = from.endsWith('/') ? from : `${from}/`
    const fromKey = k(root, from)
    const toKey = k(root, to)

    const nextDirs: Record<Key, DirState> = {}
    for (const [key, value] of Object.entries(s.dirs)) {
      if (key === fromKey) {
        nextDirs[toKey] = value
        continue
      }
      const prefix = `${fromKey}/`
      if (key.startsWith(prefix)) {
        const suffix = key.slice(prefix.length)
        nextDirs[`${toKey}/${suffix}`] = value
        continue
      }
      nextDirs[key] = value
    }

    const nextFiles: Record<Key, FileBuffer> = {}
    for (const [key, value] of Object.entries(s.files)) {
      if (key === fromKey) {
        nextFiles[toKey] = value
        continue
      }
      const prefix = `${fromKey}/`
      if (key.startsWith(prefix)) {
        const suffix = key.slice(prefix.length)
        nextFiles[`${toKey}/${suffix}`] = value
        continue
      }
      nextFiles[key] = value
    }

    const renameRel = (rel: string): string => {
      if (rel === from) return to
      if (rel.startsWith(fromPrefix)) return `${to}/${rel.slice(fromPrefix.length)}`
      return rel
    }

    const nextOpenTabs = s.openTabs.map(renameRel)
    const nextActive =
      s.activeTab && (s.activeTab === from || s.activeTab.startsWith(fromPrefix))
        ? renameRel(s.activeTab)
        : s.activeTab
    const nextSelected =
      s.selectedRelPath &&
      (s.selectedRelPath === from || s.selectedRelPath.startsWith(fromPrefix))
        ? renameRel(s.selectedRelPath)
        : s.selectedRelPath

    const remap = <V,>(map: Record<string, V>): Record<string, V> => {
      const out: Record<string, V> = {}
      for (const [key, value] of Object.entries(map)) {
        out[renameRel(key)] = value
      }
      return out
    }

    schedulePersistExpanded(root, nextDirs)

    return {
      dirs: nextDirs,
      files: nextFiles,
      openTabs: nextOpenTabs,
      activeTab: nextActive,
      selectedRelPath: nextSelected,
      aiPendingWrites: remap(s.aiPendingWrites),
      selfPendingWrites: remap(s.selfPendingWrites),
      aiModifiedAt: remap(s.aiModifiedAt),
      externalChanged: remap(s.externalChanged),
      lastSeenContent: remap(s.lastSeenContent),
      monacoModels: remap(s.monacoModels),
      tabViewStates: remap(s.tabViewStates),
    }
  })
}

let editorDraftFlusher: (() => void) | null = null

export function registerEditorDraftFlusher(fn: (() => void) | null) {
  editorDraftFlusher = fn
}

export function flushEditorDraft() {
  try {
    editorDraftFlusher?.()
  } catch {
  }
}

useWorkspaceFilesStore.subscribe((state, prev) => {
  const root = state.root
  if (!root) return
  if (root !== prev.root) return
  if (
    state.openTabs === prev.openTabs &&
    state.activeTab === prev.activeTab &&
    state.tabViewStates === prev.tabViewStates
  ) {
    return
  }
  scheduleTabsPersist(root)
})

if (typeof window !== 'undefined') {
  window.addEventListener('beforeunload', () => {
    const state = useWorkspaceFilesStore.getState()
    if (state.root) {
      flushTabsPersist(state.root, state)
    }
  })
}

export { joinPath, nameOf, parentOf }
