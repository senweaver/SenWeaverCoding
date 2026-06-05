// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type SessionSearchResult = {
  sessionId: string
  title: string
  matchCount: number
  matches: Array<{ line: number; text: string }>
}

type SessionSearchResponse = { results: SessionSearchResult[] }

export const searchApi = {
  searchSessions(query: string) {
    return api.post<SessionSearchResponse>('/api/search/sessions', { query })
  },
}
