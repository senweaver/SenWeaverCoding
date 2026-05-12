// SPDX-License-Identifier: MIT
//
// Client-side LSP coordination layer.
//
// Responsibilities:
// 1. Forward CodeMirror lifecycle events (`didOpen` / `didChange` /
//    `didSave` / `didClose`) to the backend `/api/lsp/textdocument`
//    relay, with a 300 ms debounce on `didChange` so a fast typist
//    doesn't drown the server.
// 2. Issue synchronous `hover` / `completion` / `definition` requests
//    against `/api/lsp/request`.
// 3. Subscribe to every active chat WebSocket so `lsp_diagnostics`,
//    `lsp_install_progress`, and `lsp_server_status` events flow into
//    the `lspStore` regardless of which session is currently focused.

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
  }) {
    const response = await lspApi.request<{
      contents?: unknown
      range?: unknown
    }>({
      ...params,
      method: 'hover',
      uri: normalizeUri(params.uri),
    })
    return response.result ?? null
  },

  async completion(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
  }) {
    const response = await lspApi.request<unknown>({
      ...params,
      method: 'completion',
      uri: normalizeUri(params.uri),
    })
    return response.result ?? null
  },

  async definition(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
  }) {
    const response = await lspApi.request<unknown>({
      ...params,
      method: 'definition',
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
  }) {
    const response = await lspApi.request<unknown>({
      ...params,
      method: 'references',
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
  }) {
    const response = await lspApi.request<unknown>({
      method: 'inlayHint',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
      range: params.range,
    })
    return response.result ?? null
  },

  async signatureHelp(params: {
    uri: string
    languageId?: string
    line: number
    character: number
    text?: string
    triggerCharacter?: string
  }) {
    const response = await lspApi.request<unknown>({
      ...params,
      method: 'signatureHelp',
      uri: normalizeUri(params.uri),
    })
    return response.result ?? null
  },

  async documentSymbol(params: {
    uri: string
    languageId?: string
    text?: string
  }) {
    const response = await lspApi.request<unknown>({
      method: 'documentSymbol',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
    })
    return response.result ?? null
  },

  async formatting(params: {
    uri: string
    languageId?: string
    text?: string
    options?: { tabSize: number; insertSpaces: boolean }
  }) {
    const response = await lspApi.request<unknown>({
      method: 'formatting',
      uri: normalizeUri(params.uri),
      languageId: params.languageId,
      text: params.text,
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
