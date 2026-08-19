// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { fileHistoryApi } from '../api/fileHistory'

export type FileHistoryInfo = {
  count: number
  lastTimestamp: number
}

type RootHistory = {
  files: Record<string, FileHistoryInfo>
  fetchedAt: number
  loading: boolean
  error?: string
}

type FileHistoryState = {
  byRoot: Record<string, RootHistory>
  fetchFiles: (root: string) => Promise<void>
  scheduleRefresh: (root: string) => void
  clearRoot: (root: string) => void
}

const REFRESH_DEBOUNCE_MS = 800

const refreshTimers: Record<string, ReturnType<typeof setTimeout>> = {}

export const useFileHistoryStore = create<FileHistoryState>((set, get) => ({
  byRoot: {},

  fetchFiles: async (root: string) => {
    if (!root) return
    set((s) => {
      const prev = s.byRoot[root]
      return {
        byRoot: {
          ...s.byRoot,
          [root]: {
            files: prev?.files ?? {},
            fetchedAt: prev?.fetchedAt ?? 0,
            loading: true,
            error: undefined,
          },
        },
      }
    })
    try {
      const res = await fileHistoryApi.files({ root })
      const files: Record<string, FileHistoryInfo> = {}
      for (const entry of res.files) {
        files[entry.relPath] = {
          count: entry.count,
          lastTimestamp: entry.lastTimestamp,
        }
      }
      set((s) => ({
        byRoot: {
          ...s.byRoot,
          [root]: {
            files,
            fetchedAt: Date.now(),
            loading: false,
            error: undefined,
          },
        },
      }))
    } catch (err) {
      set((s) => {
        const prev = s.byRoot[root]
        return {
          byRoot: {
            ...s.byRoot,
            [root]: {
              files: prev?.files ?? {},
              fetchedAt: prev?.fetchedAt ?? 0,
              loading: false,
              error: err instanceof Error ? err.message : String(err),
            },
          },
        }
      })
    }
  },

  scheduleRefresh: (root: string) => {
    if (!root) return
    const existing = refreshTimers[root]
    if (existing) clearTimeout(existing)
    refreshTimers[root] = setTimeout(() => {
      delete refreshTimers[root]
      void get().fetchFiles(root)
    }, REFRESH_DEBOUNCE_MS)
  },

  clearRoot: (root: string) => {
    if (refreshTimers[root]) {
      clearTimeout(refreshTimers[root])
      delete refreshTimers[root]
    }
    set((s) => {
      if (!(root in s.byRoot)) return {}
      const next = { ...s.byRoot }
      delete next[root]
      return { byRoot: next }
    })
  },
}))

export function selectFileHistoryInfo(root: string | null, relPath: string) {
  return (state: FileHistoryState): FileHistoryInfo | undefined => {
    if (!root) return undefined
    return state.byRoot[root]?.files[relPath]
  }
}
