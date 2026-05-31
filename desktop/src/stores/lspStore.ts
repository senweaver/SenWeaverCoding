// SPDX-License-Identifier: MIT

import { create } from 'zustand'
import { lspApi, type LspPreferences } from '../api/lsp'
import { useSessionStore } from './sessionStore'
import type {
  LspBroadcastEvent,
  LspDiagnostic,
  LspInstallState,
  LspServerLifecycleStatus,
  LspServerRecord,
  LspUpsertPayload,
} from '../types/lsp'

type DiagnosticsByUri = Record<string, { serverId: string; version: number | null; diagnostics: LspDiagnostic[] }>
type DiagnosticsByWorkspace = Record<string, DiagnosticsByUri>

const DIAGNOSTICS_UNKNOWN_WORKSPACE = '__unknown__'

function normaliseFsPathForCompare(value: string): string {
  return value.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
}

function uriToFsPath(uri: string): string | null {
  if (uri.startsWith('file://')) {
    try {
      const decoded = decodeURIComponent(uri.slice(7))
      if (decoded.startsWith('/') && /^\/[A-Za-z]:/.test(decoded)) {
        return decoded.slice(1)
      }
      return decoded
    } catch {
      return uri.slice(7)
    }
  }
  return uri
}

export function resolveWorkspaceRootForUri(
  uri: string,
  workspaceRoots: readonly string[],
): string {
  const fsPath = uriToFsPath(uri)
  if (!fsPath) return DIAGNOSTICS_UNKNOWN_WORKSPACE
  const cmp = normaliseFsPathForCompare(fsPath)
  let best: string | null = null
  let bestLen = -1
  for (const root of workspaceRoots) {
    if (!root) continue
    const rootCmp = normaliseFsPathForCompare(root)
    if (cmp === rootCmp || cmp.startsWith(`${rootCmp}/`)) {
      if (rootCmp.length > bestLen) {
        best = root
        bestLen = rootCmp.length
      }
    }
  }
  return best ?? DIAGNOSTICS_UNKNOWN_WORKSPACE
}

function collectWorkspaceRoots(): string[] {
  const roots = new Set<string>()
  for (const session of useSessionStore.getState().sessions) {
    if (session.workDir && session.workDir.trim().length > 0) {
      roots.add(session.workDir)
    }
  }
  return Array.from(roots)
}

export { DIAGNOSTICS_UNKNOWN_WORKSPACE }

type InstallProgressMap = Record<
  string,
  {
    phase: string
    percent?: number | null
    message?: string
    bytesDownloaded?: number
    bytesTotal?: number | null
  }
>

type ServerStatusMap = Record<string, { status: LspServerLifecycleStatus; reason?: string | null }>

export type LspDisplayStatus =
  | 'disabled'
  | 'stopped'
  | 'install'
  | 'installing'
  | 'ready'
  | 'starting'
  | 'failed'

export function resolveLspServerDisplayStatus(
  globalEnabled: boolean,
  server: LspServerRecord,
  lifecycle?: LspServerLifecycleStatus | null,
  installProgress?: { phase: string } | null,
): LspDisplayStatus {
  if (!globalEnabled) {
    return 'disabled'
  }
  if (!server.enabled) {
    return 'stopped'
  }

  const phase = installProgress?.phase
  const installing =
    phase === 'resolving' ||
    phase === 'downloading' ||
    phase === 'extracting' ||
    phase === 'verifying' ||
    server.installState.status === 'installing'
  if (installing) {
    return 'installing'
  }
  if (server.installState.status === 'failed' || phase === 'failed') {
    return 'failed'
  }

  const hasCommand = Boolean(server.command?.trim())
  if (server.managed && server.installState.status === 'not_installed') {
    return 'install'
  }
  if (server.managed && !hasCommand) {
    return 'install'
  }
  if (!server.managed && !hasCommand) {
    return 'stopped'
  }

  if (lifecycle === 'ready') {
    return 'ready'
  }
  if (lifecycle === 'starting') {
    return 'starting'
  }
  if (lifecycle === 'failed') {
    return 'failed'
  }
  if (server.installState.status === 'installed' || hasCommand) {
    return 'starting'
  }
  return 'install'
}

type LspStore = {
  enabled: boolean
  servers: LspServerRecord[]
  selectedId: string | null
  isLoading: boolean
  error: string | null

  diagnosticsByUri: DiagnosticsByUri
  diagnosticsByWorkspace: DiagnosticsByWorkspace
  installProgress: InstallProgressMap
  serverStatus: ServerStatusMap

  preferences: LspPreferences
  preferencesLoaded: boolean

  fetch: () => Promise<void>
  setGlobalEnabled: (enabled: boolean) => Promise<void>
  createServer: (payload: LspUpsertPayload) => Promise<LspServerRecord>
  updateServer: (id: string, payload: LspUpsertPayload) => Promise<LspServerRecord>
  deleteServer: (id: string) => Promise<void>
  toggleServer: (id: string) => Promise<LspServerRecord>
  installServer: (id: string) => Promise<void>
  restartServer: (id: string) => Promise<void>
  selectServer: (id: string | null) => void

  fetchPreferences: () => Promise<void>
  setPreferences: (payload: Partial<LspPreferences>) => Promise<void>

  clearDiagnostics: () => void

  handleBroadcastEvent: (event: LspBroadcastEvent) => void
}

const TEMPLATE_PROTOTYPES: Record<string, LspUpsertPayload> = {
  'rust-analyzer': {
    id: 'rust-analyzer',
    languageId: 'rust',
    displayName: 'rust-analyzer',
    enabled: false,
    managed: true,
    command: null,
    args: [],
    env: {},
    fileExtensions: ['rs'],
    initializationOptions: null,
  },
  'typescript-language-server': {
    id: 'typescript-language-server',
    languageId: 'typescript',
    displayName: 'typescript-language-server',
    enabled: false,
    managed: true,
    command: null,
    args: ['--stdio'],
    env: {},
    fileExtensions: ['ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs'],
    initializationOptions: null,
  },
  pyright: {
    id: 'pyright',
    languageId: 'python',
    displayName: 'Pyright',
    enabled: false,
    managed: true,
    command: null,
    args: ['--stdio'],
    env: {},
    fileExtensions: ['py', 'pyi'],
    initializationOptions: null,
  },
  gopls: {
    id: 'gopls',
    languageId: 'go',
    displayName: 'gopls',
    enabled: false,
    managed: true,
    command: null,
    args: [],
    env: {},
    fileExtensions: ['go'],
    initializationOptions: null,
  },
  clangd: {
    id: 'clangd',
    languageId: 'cpp',
    displayName: 'clangd',
    enabled: false,
    managed: true,
    command: null,
    args: [],
    env: {},
    fileExtensions: ['c', 'h', 'cc', 'cpp', 'cxx', 'hpp', 'hh', 'hxx'],
    initializationOptions: null,
  },
  'bash-language-server': {
    id: 'bash-language-server',
    languageId: 'shell',
    displayName: 'bash-language-server',
    enabled: false,
    managed: true,
    command: null,
    args: ['start'],
    env: {},
    fileExtensions: ['sh', 'bash', 'zsh'],
    initializationOptions: null,
  },
  'yaml-language-server': {
    id: 'yaml-language-server',
    languageId: 'yaml',
    displayName: 'yaml-language-server',
    enabled: false,
    managed: true,
    command: null,
    args: ['--stdio'],
    env: {},
    fileExtensions: ['yaml', 'yml'],
    initializationOptions: null,
  },
  'vscode-html-language-server': {
    id: 'vscode-html-language-server',
    languageId: 'html',
    displayName: 'vscode-html-language-server',
    enabled: false,
    managed: true,
    command: null,
    args: ['--stdio'],
    env: {},
    fileExtensions: ['html', 'htm'],
    initializationOptions: null,
  },
  'vscode-css-language-server': {
    id: 'vscode-css-language-server',
    languageId: 'css',
    displayName: 'vscode-css-language-server',
    enabled: false,
    managed: true,
    command: null,
    args: ['--stdio'],
    env: {},
    fileExtensions: ['css', 'scss', 'less'],
    initializationOptions: null,
  },
  'vscode-json-language-server': {
    id: 'vscode-json-language-server',
    languageId: 'json',
    displayName: 'vscode-json-language-server',
    enabled: false,
    managed: true,
    command: null,
    args: ['--stdio'],
    env: {},
    fileExtensions: ['json', 'jsonc'],
    initializationOptions: null,
  },
  'lua-language-server': {
    id: 'lua-language-server',
    languageId: 'lua',
    displayName: 'lua-language-server',
    enabled: false,
    managed: false,
    command: null,
    args: [],
    env: {},
    fileExtensions: ['lua'],
    initializationOptions: null,
  },
  jdtls: {
    id: 'jdtls',
    languageId: 'java',
    displayName: 'Eclipse JDT Language Server',
    enabled: false,
    managed: false,
    command: null,
    args: [],
    env: {},
    fileExtensions: ['java'],
    initializationOptions: null,
  },
  omnisharp: {
    id: 'omnisharp',
    languageId: 'csharp',
    displayName: 'OmniSharp',
    enabled: false,
    managed: false,
    command: null,
    args: ['-lsp'],
    env: {},
    fileExtensions: ['cs'],
    initializationOptions: null,
  },
}

export function lspTemplate(id: string): LspUpsertPayload | null {
  return TEMPLATE_PROTOTYPES[id] ?? null
}

export function listLspTemplates(): LspUpsertPayload[] {
  return Object.values(TEMPLATE_PROTOTYPES)
}

function applyServerPatch(
  servers: LspServerRecord[],
  next: LspServerRecord,
): LspServerRecord[] {
  const idx = servers.findIndex((s) => s.id === next.id)
  if (idx === -1) return [...servers, next]
  const copy = servers.slice()
  copy[idx] = next
  return copy
}

function mergeInstallStateFromProgress(
  state: LspInstallState | undefined,
  payload: LspBroadcastEvent & { type: 'lsp_install_progress' },
): LspInstallState | null {
  switch (payload.phase) {
    case 'done':
      return { status: 'installed', version: payload.version, path: payload.path }
    case 'failed':
      return { status: 'failed', reason: payload.reason }
    case 'resolving':
    case 'downloading':
    case 'extracting':
    case 'verifying':
      return { status: 'installing' }
    default:
      return state ?? null
  }
}

export const useLspStore = create<LspStore>((set, get) => ({
  enabled: false,
  servers: [],
  selectedId: null,
  isLoading: false,
  error: null,

  diagnosticsByUri: {},
  diagnosticsByWorkspace: {},
  installProgress: {},
  serverStatus: {},

  preferences: { inlayHintsEnabled: true, formatOnSave: false, hoverDelayMs: 250 },
  preferencesLoaded: false,

  fetch: async () => {
    set({ isLoading: true, error: null })
    try {
      const response = await lspApi.list()
      const snapshot: ServerStatusMap = {}
      for (const entry of response.servers) {
        if (entry.lifecycleStatus) {
          snapshot[entry.id] = { status: entry.lifecycleStatus, reason: null }
        }
      }
      set({
        enabled: response.enabled,
        servers: response.servers,
        isLoading: false,
        serverStatus: snapshot,
      })
    } catch (error) {
      set({
        isLoading: false,
        error: error instanceof Error ? error.message : 'Failed to load LSP servers',
      })
    }
  },

  setGlobalEnabled: async (enabled) => {
    const result = await lspApi.setGlobalEnabled(enabled)
    if (enabled) {
      set((state) => {
        const nextStatus: ServerStatusMap = { ...state.serverStatus }
        for (const server of state.servers) {
          if (!server.enabled) continue
          const hasCommand = Boolean(server.command?.trim())
          const needsInstall =
            (server.managed && server.installState.status === 'not_installed') ||
            (server.managed && !hasCommand) ||
            (!server.managed && !hasCommand)
          if (!needsInstall) {
            nextStatus[server.id] = { status: 'starting', reason: null }
          }
        }
        return { enabled: result.enabled, serverStatus: nextStatus }
      })
    } else {
      set({ enabled: result.enabled })
    }
    await get().fetch()
  },

  createServer: async (payload) => {
    const { server } = await lspApi.create(payload)
    set((state) => ({ servers: applyServerPatch(state.servers, server), selectedId: server.id }))
    return server
  },

  updateServer: async (id, payload) => {
    const { server } = await lspApi.update(id, payload)
    set((state) => ({ servers: applyServerPatch(state.servers, server) }))
    return server
  },

  deleteServer: async (id) => {
    await lspApi.remove(id)
    set((state) => ({
      servers: state.servers.filter((s) => s.id !== id),
      selectedId: state.selectedId === id ? null : state.selectedId,
    }))
  },

  toggleServer: async (id) => {
    const { server } = await lspApi.toggle(id)
    set((state) => ({ servers: applyServerPatch(state.servers, server) }))
    await get().fetch()
    return server
  },

  installServer: async (id) => {
    set((state) => ({
      installProgress: {
        ...state.installProgress,
        [id]: { phase: 'resolving', message: 'starting' },
      },
    }))
    try {
      await lspApi.install(id)
    } catch (err) {
      const reason = err instanceof Error ? err.message : 'install failed'
      set((state) => ({
        installProgress: { ...state.installProgress, [id]: { phase: 'failed', message: reason } },
      }))
      throw err
    }
    await get().fetch()
  },

  restartServer: async (id) => {
    await lspApi.restart(id)
    await get().fetch()
  },

  selectServer: (id) => set({ selectedId: id }),

  fetchPreferences: async () => {
    try {
      const prefs = await lspApi.getPreferences()
      set({ preferences: prefs, preferencesLoaded: true })
    } catch (err) {
      set({
        preferencesLoaded: true,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },

  setPreferences: async (payload) => {
    const result = await lspApi.setPreferences(payload)
    set({
      preferences: {
        inlayHintsEnabled: result.inlayHintsEnabled,
        formatOnSave: result.formatOnSave,
        hoverDelayMs: result.hoverDelayMs,
      },
      preferencesLoaded: true,
    })
  },

  clearDiagnostics: () => {
    set({ diagnosticsByUri: {}, diagnosticsByWorkspace: {} })
  },

  handleBroadcastEvent: (event) => {
    switch (event.type) {
      case 'lsp_diagnostics': {
        const next: DiagnosticsByUri = { ...get().diagnosticsByUri }
        if (!event.diagnostics || event.diagnostics.length === 0) {
          delete next[event.uri]
        } else {
          next[event.uri] = {
            serverId: event.serverId,
            version: event.version ?? null,
            diagnostics: event.diagnostics,
          }
        }
        const workspaceRoots = collectWorkspaceRoots()
        const workspaceRoot = resolveWorkspaceRootForUri(event.uri, workspaceRoots)
        const prevWorkspaceMap = get().diagnosticsByWorkspace
        const wsCopy: DiagnosticsByWorkspace = { ...prevWorkspaceMap }
        const bucket: DiagnosticsByUri = { ...(wsCopy[workspaceRoot] ?? {}) }
        if (!event.diagnostics || event.diagnostics.length === 0) {
          delete bucket[event.uri]
        } else {
          bucket[event.uri] = {
            serverId: event.serverId,
            version: event.version ?? null,
            diagnostics: event.diagnostics,
          }
        }
        if (Object.keys(bucket).length === 0) {
          delete wsCopy[workspaceRoot]
        } else {
          wsCopy[workspaceRoot] = bucket
        }
        set({ diagnosticsByUri: next, diagnosticsByWorkspace: wsCopy })
        break
      }
      case 'lsp_install_progress': {
        const id = event.serverId
        const merged = mergeInstallStateFromProgress(
          get().servers.find((s) => s.id === id)?.installState,
          event,
        )
        set((state) => ({
          installProgress: {
            ...state.installProgress,
            [id]: {
              phase: event.phase,
              percent: 'percent' in event ? event.percent : undefined,
              message: 'message' in event ? event.message : undefined,
              bytesDownloaded: 'bytesDownloaded' in event ? event.bytesDownloaded : undefined,
              bytesTotal: 'bytesTotal' in event ? event.bytesTotal : undefined,
            },
          },
          servers:
            merged && id
              ? state.servers.map((s) => (s.id === id ? { ...s, installState: merged } : s))
              : state.servers,
        }))
        break
      }
      case 'lsp_server_status': {
        set((state) => ({
          serverStatus: {
            ...state.serverStatus,
            [event.serverId]: {
              status: event.status,
              reason: event.reason ?? null,
            },
          },
        }))
        break
      }
    }
  },
}))
