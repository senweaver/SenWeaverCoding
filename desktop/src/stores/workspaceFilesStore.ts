import { create } from 'zustand'
import { workspaceFilesApi, type WorkspaceWatchEvent } from '../api/workspaceFiles'
import type { FileTreeNode } from '../types/workspaceFile'

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
  encoding: 'utf8' | 'base64'
  sizeBytes: number
  modifiedAt?: string
  mimeType?: string
  loading: boolean
  saving: boolean
  error?: string
  saveError?: string
}

type Key = string

const k = (root: string | null, relPath: string) => `${root ?? ''}::${relPath}`

const AI_PENDING_WINDOW_MS = 5_000

const SELF_PENDING_WINDOW_MS = 3_000

export const AI_FRESH_WINDOW_MS = 8_000

export type WorkspaceFilesState = {
  root: string | null
  rootEntries: FileTreeNode[]
  rootLoaded: boolean
  rootLoading: boolean
  rootError?: string
  truncated: boolean

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

  setRoot: (root: string | null) => void
  refreshRoot: () => Promise<void>
  loadDirectory: (relPath: string) => Promise<void>
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

  registerSelfWrite: (relPath: string) => void

  handleWatchEvent: (event: WorkspaceWatchEvent) => void

  acknowledgeExternalChange: (relPath: string) => void

  snapshotLastSeen: (relPath: string, content: string) => void

  createFile: (parentRelPath: string, name: string) => Promise<void>
  createDir: (parentRelPath: string, name: string) => Promise<void>
  rename: (relPath: string, nextRelPath: string) => Promise<void>
  remove: (relPath: string, isDir: boolean) => Promise<void>
  uploadFiles: (parentRelPath: string, files: File[]) => Promise<number>
}

let watcherDispose: (() => void) | null = null

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

export const useWorkspaceFilesStore = create<WorkspaceFilesState>((set, get) => ({
  root: null,
  rootEntries: [],
  rootLoaded: false,
  rootLoading: false,
  rootError: undefined,
  truncated: false,
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

  setRoot: (root) => {
    const current = get().root
    if (current === root) return
    if (watcherDispose) {
      watcherDispose()
      watcherDispose = null
    }
    set({
      root,
      rootEntries: [],
      rootLoaded: false,
      rootLoading: false,
      rootError: undefined,
      truncated: false,
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
    })
    if (root) {
      void get().refreshRoot()
      watcherDispose = workspaceFilesApi.watch(
        root,
        (event) => {

          if (get().root === root) {
            get().handleWatchEvent(event)
          }
        },
        () => {

        },
      )
    }
  },

  refreshRoot: async () => {
    const root = get().root
    if (!root) return
    set({ rootLoading: true, rootError: undefined })
    try {
      const tree = await workspaceFilesApi.tree({ root, depth: 1 })
      set({
        rootEntries: tree.entries,
        rootLoaded: true,
        rootLoading: false,
        truncated: tree.truncated,
      })
    } catch (err) {
      set({
        rootLoading: false,
        rootError: err instanceof Error ? err.message : String(err),
      })
    }
  },

  loadDirectory: async (relPath: string) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    const existing = get().dirs[key] ?? emptyDir
    if (existing.loaded || existing.loading) return
    set((s) => ({
      dirs: { ...s.dirs, [key]: { ...existing, loading: true, error: undefined } },
    }))
    try {
      const tree = await workspaceFilesApi.tree({
        root,
        path: relPath,
        depth: 1,
      })
      set((s) => ({
        dirs: {
          ...s.dirs,
          [key]: {
            loaded: true,
            loading: false,
            expanded: true,
            children: tree.entries,
          },
        },
      }))
    } catch (err) {
      set((s) => ({
        dirs: {
          ...s.dirs,
          [key]: {
            ...(s.dirs[key] ?? emptyDir),
            loading: false,
            error: err instanceof Error ? err.message : String(err),
          },
        },
      }))
    }
  },

  setExpanded: (relPath: string, expanded: boolean) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    set((s) => ({
      dirs: {
        ...s.dirs,
        [key]: {
          ...(s.dirs[key] ?? emptyDir),
          expanded,
        },
      },
    }))
  },

  toggleExpanded: async (relPath: string) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    const dir = get().dirs[key] ?? emptyDir
    if (!dir.loaded && !dir.loading) {
      await get().loadDirectory(relPath)
      return
    }
    set((s) => ({
      dirs: {
        ...s.dirs,
        [key]: { ...dir, expanded: !dir.expanded },
      },
    }))
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
    if (existing && existing.original === existing.draft && !existing.isDirty) {

      get().snapshotLastSeen(relPath, existing.original)
      return
    }
    if (existing?.loading) return
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
              encoding: file.encoding,
              sizeBytes: file.sizeBytes,
              modifiedAt: file.modifiedAt,
              mimeType: file.mimeType,
              loading: false,
              saving: false,
              error: undefined,
              saveError: undefined,
            },
          },

          lastSeenContent: { ...s.lastSeenContent, [relPath]: file.content },
        }
      })
    } catch (err) {
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
      }
    })
  },

  closeAllTabs: () => {
    const root = get().root
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
      }
    })
  },

  closeOtherTabs: (keepRelPath: string) => {
    const root = get().root
    set((s) => {
      if (!s.openTabs.includes(keepRelPath)) return {}
      const files = { ...s.files }
      let aiModifiedAt = { ...s.aiModifiedAt }
      let externalChanged = { ...s.externalChanged }
      let lastSeenContent = { ...s.lastSeenContent }
      for (const relPath of s.openTabs) {
        if (relPath === keepRelPath) continue
        delete files[k(root, relPath)]
        aiModifiedAt = stripKey(aiModifiedAt, relPath)
        externalChanged = stripKey(externalChanged, relPath)
        lastSeenContent = stripKey(lastSeenContent, relPath)
      }
      return {
        files,
        openTabs: [keepRelPath],
        activeTab: keepRelPath,
        selectedRelPath: keepRelPath,
        aiModifiedAt,
        externalChanged,
        lastSeenContent,
      }
    })
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
      return { aiPendingWrites: { ...s.aiPendingWrites, [relPath]: now } }
    })
  },

  registerSelfWrite: (relPath: string) => {
    if (!relPath) return
    set((s) => ({
      selfPendingWrites: { ...s.selfPendingWrites, [relPath]: Date.now() },
    }))
  },

  handleWatchEvent: (event: WorkspaceWatchEvent) => {
    const root = get().root
    if (!root) return
    const relPath = event.relPath
    if (!relPath) return
    const now = Date.now()

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

    if (event.kind === 'created' || event.kind === 'removed' || event.kind === 'renamed') {
      const parent = parentOf(relPath)
      const parentKey = k(root, parent)
      const parentDir = get().dirs[parentKey]
      if (parent === '' || parentDir?.loaded) {

        void refreshDir(get, set, parent)
      }
    }

    if (isOpen && !isSelf) {
      if (event.kind === 'removed') {

        return
      }
      if (buf && !buf.isDirty) {
        void reloadBuffer(get, set, relPath).then(() => {

          if (isAi) {
            set((s) => ({
              aiModifiedAt: { ...s.aiModifiedAt, [relPath]: Date.now() },
            }))
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

  updateDraft: (relPath: string, content: string) => {
    const root = get().root
    if (!root) return
    const key = k(root, relPath)
    set((s) => {
      const buf = s.files[key]
      if (!buf) return {}
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
    if (!buf || buf.isBinary) return

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
        ifMatchMtime: buf.modifiedAt,
        encoding: 'utf8',
      })

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
          },
        },

        aiModifiedAt: stripKey(s.aiModifiedAt, relPath),

        lastSeenContent: { ...s.lastSeenContent, [relPath]: buf.draft },
      }))

      const parent = parentOf(relPath)
      const parentKey = k(root, parent)
      const parentDir = get().dirs[parentKey]
      if (parentDir?.loaded) {
        try {
          const tree = await workspaceFilesApi.tree({ root, path: parent, depth: 1 })
          set((s) => ({
            dirs: {
              ...s.dirs,
              [parentKey]: {
                ...(s.dirs[parentKey] ?? emptyDir),
                children: tree.entries,
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
          [key]: { ...buf, isDirty: false, draft: buf.original },
        },
      }
    })
    await get().selectFile(relPath)
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
    get().registerSelfWrite(relPath)
    get().registerSelfWrite(nextRelPath)
    const oldKey = k(root, relPath)
    const newKey = k(root, nextRelPath)
    set((s) => {
      const files = { ...s.files }
      const buf = files[oldKey]
      if (buf) {
        files[newKey] = buf
        delete files[oldKey]
      }
      const openTabs = s.openTabs.map((t) => (t === relPath ? nextRelPath : t))
      const activeTab = s.activeTab === relPath ? nextRelPath : s.activeTab
      return {
        files,
        openTabs,
        activeTab,
        selectedRelPath:
          s.selectedRelPath === relPath ? nextRelPath : s.selectedRelPath,
      }
    })
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
    get().registerSelfWrite(relPath)
    const key = k(root, relPath)
    set((s) => {
      const files = { ...s.files }
      delete files[key]
      const dirs = { ...s.dirs }
      delete dirs[key]

      const openTabs = s.openTabs.filter((t) => t !== relPath)
      let activeTab = s.activeTab
      if (s.activeTab === relPath) {
        if (openTabs.length === 0) {
          activeTab = null
        } else {
          const oldIdx = s.openTabs.indexOf(relPath)
          const fallbackIdx = Math.min(oldIdx, openTabs.length - 1)
          activeTab = openTabs[fallbackIdx] ?? null
        }
      }
      return {
        files,
        dirs,
        openTabs,
        activeTab,
        selectedRelPath: activeTab,
        aiModifiedAt: stripKey(s.aiModifiedAt, relPath),
        externalChanged: stripKey(s.externalChanged, relPath),
        lastSeenContent: stripKey(s.lastSeenContent, relPath),
      }
    })
    await refreshDir(get, set, parentOf(relPath))
  },

  uploadFiles: async (parentRelPath: string, files: File[]) => {
    const root = get().root
    if (!root || files.length === 0) return 0
    let uploaded = 0
    for (const file of files) {
      const base64 = await readBlobAsBase64(file)
      const relTarget = joinPath(parentRelPath, file.name)
      get().registerSelfWrite(relTarget)
      await workspaceFilesApi.upload({
        root,
        path: relTarget,
        contentBase64: base64,
        overwrite: true,
      })
      get().registerSelfWrite(relTarget)
      uploaded += 1
    }
    await refreshDir(get, set, parentRelPath)
    return uploaded
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
            encoding: file.encoding,
            sizeBytes: file.sizeBytes,
            modifiedAt: file.modifiedAt,
            mimeType: file.mimeType,
          },
        },
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
  if (relPath === '') {
    await get().refreshRoot()
    return
  }
  const key = k(root, relPath)
  try {
    const tree = await workspaceFilesApi.tree({ root, path: relPath, depth: 1 })
    set((s) => ({
      dirs: {
        ...s.dirs,
        [key]: {
          ...(s.dirs[key] ?? emptyDir),
          children: tree.entries,
          loaded: true,
          loading: false,
          expanded: true,
        },
      },
    }))
  } catch (err) {
    set((s) => ({
      dirs: {
        ...s.dirs,
        [key]: {
          ...(s.dirs[key] ?? emptyDir),
          error: err instanceof Error ? err.message : String(err),
          loading: false,
        },
      },
    }))
  }
}

export { joinPath, nameOf, parentOf }
