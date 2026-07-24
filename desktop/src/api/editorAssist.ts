// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api, type RequestOptions } from './client'

export type InlineCompletionSuggestion = {
  insertText: string
  confidence?: number | null
}

export type InlineCompletionResult = {
  provider: string
  latencyMs: number
  cached: boolean
  suggestions: InlineCompletionSuggestion[]
}

export type InlineEditResult = {
  diff: string
  applied: string
  hunksExact: number
  hunksFuzzy: number
  validatorIssues: string[]
}

export type CompletionFeedbackEvent =
  | 'shown'
  | 'accepted'
  | 'accepted_partial'
  | 'rejected'
  | 'timed_out'

export type CompletionStats = {
  shown: number
  accepted: number
  acceptedPartial: number
  rejected: number
  timedOut: number
  acceptanceRate: number
  averageLatencyMs: number
}

export const editorAssistApi = {
  inlineComplete: (
    body: {
      prefix: string
      suffix: string
      path?: string
      root?: string
      maxTokens?: number
    },
    options?: RequestOptions,
  ) => api.post<InlineCompletionResult>('/api/editor/inline-completion', body, options),

  completionFeedback: (event: CompletionFeedbackEvent) =>
    api.post<{ ok: boolean }>('/api/editor/inline-completion/feedback', { event }),

  completionStats: () =>
    api.get<CompletionStats>('/api/editor/inline-completion/stats'),

  inlineEdit: (
    body: {
      path: string
      selection: string
      instruction: string
      contextLines?: string[]
    },
    options?: RequestOptions,
  ) => api.post<InlineEditResult>('/api/editor/inline-edit', body, options),
}
