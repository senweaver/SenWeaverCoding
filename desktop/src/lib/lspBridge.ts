// SPDX-License-Identifier: MIT

import { lspApi } from '../api/lsp'
import { wsManager } from '../api/websocket'
import { useLspStore } from '../stores/lspStore'
import { useWorkspaceFilesStore } from '../stores/workspaceFilesStore'
import { inferLanguageFromPath } from './extLanguage'
import type {
  LspBroadcastEvent,
  LspDiagnostic,
  LspInstallProgressPhase,
} from '../types/lsp'
import type { ServerMessage } from '../types/chat'

function normalizeUri(input: string): string {
  if (input.startsWith('file://')) return input

  let path = input.replace(/\\/g, '/')
  if (!path.startsWith('/')) path = '/' + path
  return `file://${path}`
}

const openCounts = new Map<string, number>()
const changeTimers = new Map<string, ReturnType<typeof setTimeout>>()
const DEBOUNCE_MS = 300

const langSupportCache = new Map<string, boolean>()
let langSupportCacheVersion = 0

function refreshLangSupportCache() {
  const { enabled, servers } = useLspStore.getState()
  langSupportCache.clear()
  if (!enabled) return
  for (const server of servers) {
    if (!server.enabled) continue
    if (!server.command || server.command.trim() === '') continue
    if (server.languageId) {
      langSupportCache.set(server.languageId.toLowerCase(), true)
    }
    for (const ext of server.fileExtensions ?? []) {
      const norm = ext.trim().toLowerCase().replace(/^\./, '')
      if (norm) langSupportCache.set(norm, true)
    }
  }
}

useLspStore.subscribe((state, prev) => {
  if (
    state.servers !== prev.servers ||
    state.enabled !== prev.enabled
  ) {
    langSupportCacheVersion += 1
    refreshLangSupportCache()
  }
})

export function hasServerForLanguage(languageId: string | undefined | null): boolean {
  if (!languageId) return false
  if (langSupportCache.size === 0 && langSupportCacheVersion === 0) {
    refreshLangSupportCache()
    langSupportCacheVersion = 1
  }
  return langSupportCache.get(languageId.toLowerCase()) === true
}

export type DocumentParams = {
  uri: string
  languageId?: string
  text: string
}

export const lspBridge = {

  async didOpen(params: DocumentParams) {
    const uri = normalizeUri(params.uri)
    const next = (openCounts.get(uri) ?? 0) + 1
    openCounts.set(uri, next)
    if (next === 1) {
      try {
        await lspApi.notify({
          method: 'didOpen',
          uri,
          languageId: params.languageId,
          text: params.text,
        })
      } catch (err) {
        console.warn('[lsp] didOpen failed', err)
      }
    }
  },

  didChange(params: DocumentParams) {
    const uri = normalizeUri(params.uri)
    const previous = changeTimers.get(uri)
    if (previous) clearTimeout(previous)
    const timer = setTimeout(async () => {
      changeTimers.delete(uri)
      try {
        await lspApi.notify({
          method: 'didChange',
          uri,
          languageId: params.languageId,
          text: params.text,
        })
      } catch (err) {
        console.warn('[lsp] didChange failed', err)
      }
    }, DEBOUNCE_MS)
    changeTimers.set(uri, timer)
  },

  async didSave(params: DocumentParams) {
    const uri = normalizeUri(params.uri)

    const pending = changeTimers.get(uri)
    if (pending) {
      clearTimeout(pending)
      changeTimers.delete(uri)
    }
    try {
      await lspApi.notify({
        method: 'didSave',
        uri,
        languageId: params.languageId,
        text: params.text,
      })
    } catch (err) {
      console.warn('[lsp] didSave failed', err)
    }
  },

  async didClose(uri: string) {
    const normalized = normalizeUri(uri)
    const remaining = (openCounts.get(normalized) ?? 0) - 1
    if (remaining > 0) {
      openCounts.set(normalized, remaining)
      return
    }
    openCounts.delete(normalized)
    const pending = changeTimers.get(normalized)
    if (pending) {
      clearTimeout(pending)
      changeTimers.delete(normalized)
    }
    try {
      await lspApi.notify({ method: 'didClose', uri: normalized })
    } catch (err) {
      console.warn('[lsp] didClose failed', err)
    }
  },

  async hover(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const { signal, ...rest } = params
    const response = await lspApi.request<{
      contents?: unknown
      range?: unknown
    }>(
      {
        ...rest,
        method: 'hover',
        uri: normalizeUri(params.uri),
      },
      signal ? { signal } : undefined,
    )
    return response.result ?? null
  },

  async completion(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
    triggerKind?: number
    triggerCharacter?: string
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const { signal, ...rest } = params
    const response = await lspApi.request<unknown>(
      {
        ...rest,
        method: 'completion',
        uri: normalizeUri(params.uri),
      },
      signal ? { signal } : undefined,
    )
    return response.result ?? null
  },

  async completionResolve(params: {
    item: unknown
    languageId?: string
    uri?: string
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      method: 'completionItem/resolve',
      uri: params.uri ? normalizeUri(params.uri) : '',
      languageId: params.languageId,
      item: params.item,
    })
    return response.result ?? null
  },

  async definition(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const { signal, ...rest } = params
    const response = await lspApi.request<unknown>(
      {
        ...rest,
        method: 'definition',
        uri: normalizeUri(params.uri),
      },
      signal ? { signal } : undefined,
    )
    return response.result ?? null
  },

  async declaration(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const { signal, ...rest } = params
    const response = await lspApi.request<unknown>(
      {
        ...rest,
        method: 'declaration',
        uri: normalizeUri(params.uri),
      },
      signal ? { signal } : undefined,
    )
    return response.result ?? null
  },

  async documentLink(params: {
    uri: string
    languageId?: string
    text?: string
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const { signal, ...rest } = params
    const response = await lspApi.request<unknown>(
      {
        ...rest,
        method: 'documentLink',
        uri: normalizeUri(params.uri),
      },
      signal ? { signal } : undefined,
    )
    return response.result ?? null
  },

  async typeDefinition(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      ...params,
      method: 'typeDefinition',
      uri: normalizeUri(params.uri),
    })
    return response.result ?? null
  },

  async implementation(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      ...params,
      method: 'implementation',
      uri: normalizeUri(params.uri),
    })
    return response.result ?? null
  },

  async references(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const { signal, ...rest } = params
    const response = await lspApi.request<unknown>(
      {
        ...rest,
        method: 'references',
        uri: normalizeUri(params.uri),
      },
      signal ? { signal } : undefined,
    )
    return response.result ?? null
  },

  async documentHighlight(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      ...params,
      method: 'documentHighlight',
      uri: normalizeUri(params.uri),
    })
    return response.result ?? null
  },

  async inlayHint(params: {
    uri: string
    languageId?: string
    text?: string
    range: {
      start: { line: number; character: number }
      end: { line: number; character: number }
    }
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>(
      {
        method: 'inlayHint',
        uri: normalizeUri(params.uri),
        languageId: params.languageId,
        text: params.text,
        range: params.range,
      },
      params.signal ? { signal: params.signal } : undefined,
    )
    return response.result ?? null
  },

  async signatureHelp(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
    triggerCharacter?: string
    triggerKind?: number
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const { signal, ...rest } = params
    const response = await lspApi.request<unknown>(
      {
        ...rest,
        method: 'signatureHelp',
        uri: normalizeUri(params.uri),
      },
      signal ? { signal } : undefined,
    )
    return response.result ?? null
  },

  async documentSymbol(params: {
    uri: string
    languageId?: string
    text?: string
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>(
      {
        method: 'documentSymbol',
        uri: normalizeUri(params.uri),
        languageId: params.languageId,
        text: params.text,
      },
      params.signal ? { signal: params.signal } : undefined,
    )
    return response.result ?? null
  },

  async formatting(params: {
    uri: string
    languageId?: string
    text?: string
    options?: { tabSize: number; insertSpaces: boolean }
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      method: 'formatting',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
      options: params.options ?? { tabSize: 4, insertSpaces: true },
    })
    return response.result ?? null
  },

  async rangeFormatting(params: {
    uri: string
    languageId?: string
    text?: string
    range: {
      start: { line: number; character: number }
      end: { line: number; character: number }
    }
    options?: { tabSize: number; insertSpaces: boolean }
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      method: 'rangeFormatting',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
      range: params.range,
      options: params.options ?? { tabSize: 4, insertSpaces: true },
    })
    return response.result ?? null
  },

  async onTypeFormatting(params: {
    uri: string
    languageId?: string
    text?: string
    line: number
    character: number
    ch: string
    options?: { tabSize: number; insertSpaces: boolean }
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      method: 'onTypeFormatting',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
      line: params.line,
      character: params.character,
      characterTyped: params.ch,
      options: params.options ?? { tabSize: 4, insertSpaces: true },
    })
    return response.result ?? null
  },

  async codeAction(params: {
    uri: string
    languageId?: string
    text?: string
    range: {
      start: { line: number; character: number }
      end: { line: number; character: number }
    }
    diagnostics?: Array<Record<string, unknown>>
    only?: string[]
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      method: 'codeAction',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
      range: params.range,
      diagnostics: params.diagnostics,
      only: params.only,
    })
    return response.result ?? null
  },

  async prepareRename(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      ...params,
      method: 'prepareRename',
      uri: normalizeUri(params.uri),
    })
    return response.result ?? null
  },

  async rename(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    newName: string
    text?: string
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      method: 'rename',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
      line: params.line,
      character: params.character,
      newName: params.newName,
    })
    return response.result ?? null
  },

  async foldingRange(params: {
    uri: string
    languageId?: string
    text?: string
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      method: 'foldingRange',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
    })
    return response.result ?? null
  },

  async selectionRange(params: {
    uri: string
    languageId?: string
    text?: string
    positions: Array<{ line: number; character: number }>
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      method: 'selectionRange',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
      positions: params.positions,
    })
    return response.result ?? null
  },

  async semanticTokensFull(params: {
    uri: string
    languageId?: string
    text?: string
    signal?: AbortSignal
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<{ resultId?: string; data?: number[] }>(
      {
        method: 'semanticTokens/full',
        uri: normalizeUri(params.uri),
        languageId: params.languageId,
        text: params.text,
      },
      params.signal ? { signal: params.signal } : undefined,
    )
    return response.result ?? null
  },

  async semanticTokensFullDelta(params: {
    uri: string
    languageId?: string
    text?: string
    previousResultId: string
  }) {
    if (!hasServerForLanguage(params.languageId)) return null
    const response = await lspApi.request<unknown>({
      method: 'semanticTokens/full/delta',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
      previousResultId: params.previousResultId,
    })
    return response.result ?? null
  },

  async workspaceSymbol(params: {
    query: string
    languageId?: string
  }) {
    if (params.languageId && !hasServerForLanguage(params.languageId)) {
      return null
    }
    const response = await lspApi.request<unknown>({
      method: 'workspace/symbol',
      uri: '',
      languageId: params.languageId,
      query: params.query,
    })
    return response.result ?? null
  },

  async executeCommand(params: {
    command: string
    arguments?: unknown[]
    uri?: string
    languageId?: string
  }) {
    let languageId = params.languageId
    if (!languageId || languageId.trim() === '') {
      if (params.uri) {
        const inferred = inferLanguageFromPath(params.uri)
        if (inferred && inferred !== 'text') {
          languageId = inferred
        }
      }
      if (!languageId || languageId.trim() === '') {
        const state = useWorkspaceFilesStore.getState()
        const activeTab = state.activeTab
        if (activeTab) {
          const model = state.monacoModels[activeTab]
          if (model && !model.isDisposed?.()) {
            try {
              const fromModel = model.getLanguageId()
              if (fromModel && fromModel.trim() !== '') {
                languageId = fromModel
              }
            } catch {}
          }
          if (!languageId || languageId.trim() === '') {
            const fromPath = inferLanguageFromPath(activeTab)
            if (fromPath && fromPath !== 'text') {
              languageId = fromPath
            }
          }
        }
      }
    }
    const response = await lspApi.request<unknown>({
      method: 'executeCommand',
      uri: params.uri ? normalizeUri(params.uri) : '',
      languageId,
      command: params.command,
      arguments: params.arguments,
    })
    return response.result ?? null
  },

  async willSave(params: DocumentParams) {
    const uri = normalizeUri(params.uri)
    const pending = changeTimers.get(uri)
    if (pending) {
      clearTimeout(pending)
      changeTimers.delete(uri)
    }
    try {
      await lspApi.notify({
        method: 'didChange',
        uri,
        languageId: params.languageId,
        text: params.text,
      })
    } catch (err) {
      console.warn('[lsp] willSave didChange failed', err)
    }
  },

  diagnosticsFor(uri: string): LspDiagnostic[] {
    const normalized = normalizeUri(uri)
    const entry = useLspStore.getState().diagnosticsByUri[normalized]
    return entry?.diagnostics ?? []
  },
}

export function attachToSessionStream(sessionId: string): () => void {
  const dispatch = useLspStore.getState().handleBroadcastEvent
  const handler = (msg: ServerMessage) => {
    if (
      msg.type === 'lsp_diagnostics' ||
      msg.type === 'lsp_install_progress' ||
      msg.type === 'lsp_server_status'
    ) {
      dispatch(msg as LspBroadcastEvent)
    }
  }
  return wsManager.onMessage(sessionId, handler)
}

export function attachToAllSessions(): () => void {
  const ids = wsManager.getConnectedSessionIds()
  const offs = ids.map((id) => attachToSessionStream(id))
  return () => offs.forEach((fn) => fn())
}

export type AnyInstallPhase = LspInstallProgressPhase
