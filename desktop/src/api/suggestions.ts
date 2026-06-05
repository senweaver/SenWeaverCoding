// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type PromptSuggestion = {
  text: string
  category: string
  relevance_score: number
  description: string
}

export const suggestionsApi = {
  list() {
    return api.get<{ suggestions: PromptSuggestion[] }>('/api/suggestions')
  },
}
