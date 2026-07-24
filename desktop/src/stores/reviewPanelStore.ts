// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import {
  sessionsApi,
  type EditReviewFile,
  type EditReviewFileDiff,
} from '../api/sessions'

type ReviewPanelState = {
  open: boolean
  sessionId: string | null
  loading: boolean
  error: string | null
  files: EditReviewFile[]
  keptPaths: Record<string, true>
  diffs: Record<string, EditReviewFileDiff | null>
  diffLoading: Record<string, true>
  expandedPaths: Record<string, true>
  revertingPaths: Record<string, true>
  refreshTimer: number | null
  generation: number

  openPanel: (sessionId: string) => void
  closePanel: () => void
  refresh: () => Promise<void>
  notifyFileEdit: (sessionId: string) => void
  toggleExpanded: (path: string) => void
  loadDiff: (path: string) => Promise<void>
  retryDiff: (path: string) => void
  keepFile: (path: string) => void
  keepAll: () => void
  undoFile: (path: string) => Promise<void>
  undoAll: () => Promise<void>
  purgeSession: (sessionId: string) => void
}

export const useReviewPanelStore = create<ReviewPanelState>((set, get) => ({
  open: false,
  sessionId: null,
  loading: false,
  error: null,
  files: [],
  keptPaths: {},
  diffs: {},
  diffLoading: {},
  expandedPaths: {},
  revertingPaths: {},
  refreshTimer: null,
  generation: 0,

  openPanel: (sessionId) => {
    const prev = get()
    if (prev.refreshTimer !== null) {
      window.clearTimeout(prev.refreshTimer)
    }
    set({
      open: true,
      sessionId,
      files: prev.sessionId === sessionId ? prev.files : [],
      keptPaths: prev.sessionId === sessionId ? prev.keptPaths : {},
      diffs: prev.sessionId === sessionId ? prev.diffs : {},
      diffLoading: {},
      expandedPaths: prev.sessionId === sessionId ? prev.expandedPaths : {},
      revertingPaths: {},
      error: null,
      refreshTimer: null,
      generation: prev.generation + 1,
    })
    void get().refresh()
  },

  closePanel: () => {
    const timer = get().refreshTimer
    if (timer !== null) window.clearTimeout(timer)
    set({ open: false, refreshTimer: null })
  },

  refresh: async () => {
    const sessionId = get().sessionId
    if (!sessionId) return
    const generation = get().generation
    set({ loading: true })
    try {
      const res = await sessionsApi.getEditReview(sessionId)
      if (get().sessionId !== sessionId || get().generation !== generation) {
        return
      }
      set((s) => {
        const nextDiffs: Record<string, EditReviewFileDiff | null> = {}
        for (const f of res.files) {
          const cached = s.diffs[f.path]
          if (cached !== undefined && cached !== null) nextDiffs[f.path] = cached
        }
        return { files: res.files, diffs: nextDiffs, loading: false, error: null }
      })
    } catch (err) {
      if (get().sessionId !== sessionId || get().generation !== generation) {
        return
      }
      set({
        loading: false,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },

  notifyFileEdit: (sessionId) => {
    const s = get()
    if (!s.open || s.sessionId !== sessionId) return
    if (s.refreshTimer !== null) {
      window.clearTimeout(s.refreshTimer)
    }
    const timer = window.setTimeout(() => {
      set({ refreshTimer: null })
      const cur = get()
      if (!cur.open || cur.sessionId !== sessionId) return
      set({ diffs: {}, generation: cur.generation + 1 })
      void get().refresh()
    }, 800)
    set({ refreshTimer: timer })
  },

  toggleExpanded: (path) => {
    set((s) => {
      const expanded = { ...s.expandedPaths }
      if (expanded[path]) {
        delete expanded[path]
      } else {
        expanded[path] = true
      }
      return { expandedPaths: expanded }
    })
  },

  loadDiff: async (path) => {
    const sessionId = get().sessionId
    if (!sessionId) return
    if (get().diffLoading[path] || get().diffs[path] !== undefined) return
    const generation = get().generation
    set((s) => ({ diffLoading: { ...s.diffLoading, [path]: true } }))
    try {
      const diff = await sessionsApi.getEditReviewFile(sessionId, path)
      if (get().sessionId !== sessionId || get().generation !== generation) {
        return
      }
      set((s) => {
        const diffLoading = { ...s.diffLoading }
        delete diffLoading[path]
        return { diffs: { ...s.diffs, [path]: diff }, diffLoading }
      })
    } catch {
      if (get().sessionId !== sessionId || get().generation !== generation) {
        return
      }
      set((s) => {
        const diffLoading = { ...s.diffLoading }
        delete diffLoading[path]
        return { diffs: { ...s.diffs, [path]: null }, diffLoading }
      })
    }
  },

  retryDiff: (path) => {
    set((s) => {
      const diffs = { ...s.diffs }
      delete diffs[path]
      return { diffs }
    })
    void get().loadDiff(path)
  },

  keepFile: (path) => {
    set((s) => ({ keptPaths: { ...s.keptPaths, [path]: true } }))
  },

  keepAll: () => {
    set((s) => {
      const kept: Record<string, true> = { ...s.keptPaths }
      for (const f of s.files) kept[f.path] = true
      return { keptPaths: kept }
    })
  },

  undoFile: async (path) => {
    const sessionId = get().sessionId
    if (!sessionId || get().revertingPaths[path]) return
    set((s) => ({ revertingPaths: { ...s.revertingPaths, [path]: true } }))
    try {
      const res = await sessionsApi.revertFiles(sessionId, [path])
      if (!res.ok && res.failed.length > 0) {
        throw new Error(res.failed[0]?.error ?? 'revert failed')
      }
      set((s) => {
        const diffs = { ...s.diffs }
        delete diffs[path]
        return {
          files: s.files.filter((f) => f.path !== path),
          diffs,
        }
      })
    } finally {
      set((s) => {
        const reverting = { ...s.revertingPaths }
        delete reverting[path]
        return { revertingPaths: reverting }
      })
    }
  },

  undoAll: async () => {
    const sessionId = get().sessionId
    if (!sessionId) return
    const paths = get()
      .files.filter((f) => !get().keptPaths[f.path])
      .map((f) => f.path)
    if (paths.length === 0) return
    set({ loading: true, error: null })
    try {
      const res = await sessionsApi.revertFiles(sessionId, paths)
      await get().refresh()
      if (!res.ok && res.failed.length > 0) {
        set({
          error: res.failed
            .map((f) => `${f.path}: ${f.error}`)
            .join('; '),
        })
      }
    } catch (err) {
      set({
        loading: false,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },

  purgeSession: (sessionId) => {
    if (get().sessionId !== sessionId) return
    const timer = get().refreshTimer
    if (timer !== null) window.clearTimeout(timer)
    set({
      open: false,
      sessionId: null,
      loading: false,
      error: null,
      files: [],
      keptPaths: {},
      diffs: {},
      diffLoading: {},
      expandedPaths: {},
      revertingPaths: {},
      refreshTimer: null,
      generation: get().generation + 1,
    })
  },
}))
