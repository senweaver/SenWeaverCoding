// SPDX-License-Identifier: MIT
// Thin REST wrappers for the `/api/lsp/*` routes.

import { api } from './client'
import type { LspListResponse, LspServerRecord, LspUpsertPayload } from '../types/lsp'

export const lspApi = {
  list: () => api.get<LspListResponse>('/api/lsp'),

  setGlobalEnabled: (enabled: boolean) =>
    api.put<{ ok: true; enabled: boolean }>('/api/lsp', { enabled }),

  create: (payload: LspUpsertPayload) =>
    api.post<{ server: LspServerRecord }>('/api/lsp/servers', payload),

  update: (id: string, payload: LspUpsertPayload) =>
    api.put<{ server: LspServerRecord }>(`/api/lsp/servers/${encodeURIComponent(id)}`, payload),

  remove: (id: string) =>
    api.delete<{ ok: true }>(`/api/lsp/servers/${encodeURIComponent(id)}`),

  toggle: (id: string) =>
    api.post<{ server: LspServerRecord }>(`/api/lsp/servers/${encodeURIComponent(id)}/toggle`),

  install: (id: string) =>
    api.post<{ ok: true; version: string; path: string }>(
      `/api/lsp/servers/${encodeURIComponent(id)}/install`,
      {},
      { timeout: 600_000 },
    ),

  restart: (id: string) =>
    api.post<{ ok: true }>(`/api/lsp/servers/${encodeURIComponent(id)}/restart`),

  notify: (body: {
    method: 'didOpen' | 'didChange' | 'didSave' | 'didClose'
    uri: string
    languageId?: string
    text?: string
    version?: number
  }) => api.post<{ ok: boolean; skipped?: string }>('/api/lsp/textdocument', body),

  request: <T = unknown>(
    body: {
      method:
        | 'hover'
        | 'completion'
        | 'completionItem/resolve'
        | 'completionResolve'
        | 'definition'
        | 'typeDefinition'
        | 'implementation'
        | 'declaration'
        | 'references'
        | 'documentHighlight'
        | 'inlayHint'
        | 'signatureHelp'
        | 'documentSymbol'
        | 'formatting'
        | 'rangeFormatting'
        | 'onTypeFormatting'
        | 'codeAction'
        | 'executeCommand'
        | 'prepareRename'
        | 'rename'
        | 'foldingRange'
        | 'selectionRange'
        | 'documentLink'
        | 'semanticTokens/full'
        | 'semanticTokensFull'
        | 'semanticTokens/full/delta'
        | 'semanticTokensFullDelta'
        | 'semanticTokens/range'
        | 'semanticTokensRange'
        | 'workspace/symbol'
        | 'workspaceSymbol'
      uri: string
      languageId?: string
      line?: number
      character?: number
      text?: string
      range?: {
        start: { line: number; character: number }
        end: { line: number; character: number }
      }
      options?: {
        tabSize: number
        insertSpaces: boolean
      }
      triggerCharacter?: string
      triggerKind?: number
      diagnostics?: Array<Record<string, unknown>>
      only?: string[]
      command?: string
      arguments?: unknown[]
      newName?: string
      item?: unknown
      positions?: Array<{ line: number; character: number }>
      previousResultId?: string
      characterTyped?: string
      query?: string
    },
    options?: { signal?: AbortSignal; timeout?: number },
  ) => api.post<{ result: T | null; error?: string }>('/api/lsp/request', body, options),

  getPreferences: () =>
    api.get<{ inlayHintsEnabled: boolean; formatOnSave: boolean; hoverDelayMs: number }>(
      '/api/lsp/preferences',
    ),

  setPreferences: (body: {
    inlayHintsEnabled?: boolean
    formatOnSave?: boolean
    hoverDelayMs?: number
  }) =>
    api.put<{
      ok: true
      inlayHintsEnabled: boolean
      formatOnSave: boolean
      hoverDelayMs: number
    }>('/api/lsp/preferences', body),
}

export type LspPreferences = {
  inlayHintsEnabled: boolean
  formatOnSave: boolean
  hoverDelayMs: number
}
