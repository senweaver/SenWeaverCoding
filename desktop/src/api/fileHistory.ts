// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type FileHistorySummary = {
  relPath: string
  count: number
  lastTimestamp: number
}

export type FileHistoryEntry = {
  index: number
  timestamp: number
  toolName: string
  description: string
  byteSize: number
  absent: boolean
  sha256: string
  sessionId: string | null
  sessionName: string | null
}

export type FileHistorySnapshot = {
  content: string
  absent: boolean
  binary: boolean
  tooLarge: boolean
}

function qs(params: Record<string, string | number | boolean | undefined>) {
  const search = new URLSearchParams()
  for (const [key, value] of Object.entries(params)) {
    if (value === undefined) continue
    search.set(key, String(value))
  }
  const out = search.toString()
  return out ? `?${out}` : ''
}

export const fileHistoryApi = {
  files(opts: { root: string }) {
    return api.get<{ files: FileHistorySummary[] }>(
      `/api/workspace/history/files${qs({ root: opts.root })}`,
    )
  },

  list(opts: { root: string; path: string }) {
    return api.get<{ relPath: string; entries: FileHistoryEntry[] }>(
      `/api/workspace/history/list${qs({ root: opts.root, path: opts.path })}`,
    )
  },

  snapshot(opts: { root: string; path: string; index: number }) {
    return api.get<FileHistorySnapshot>(
      `/api/workspace/history/snapshot${qs({
        root: opts.root,
        path: opts.path,
        index: opts.index,
      })}`,
    )
  },

  revert(opts: {
    root: string
    path: string
    index: number
    expectedSha256?: string
  }) {
    return api.post<{ ok: boolean; relPath: string }>(
      '/api/workspace/history/revert',
      opts,
    )
  },
}
