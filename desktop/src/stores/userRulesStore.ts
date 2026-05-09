// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { userRulesApi, type UserRuleFile } from '../api/userRules'

type UserRulesStore = {
  directory: string | null
  exists: boolean
  files: UserRuleFile[]
  contentByPath: Record<string, string>
  isLoading: boolean
  loadingContentPaths: Set<string>
  savingPaths: Set<string>
  error: string | null
  fetch: () => Promise<void>
  loadContent: (file: UserRuleFile) => Promise<string | null>
  upsert: (name: string, content: string) => Promise<void>
  delete: (name: string) => Promise<void>
  clearLocalContent: (path: string) => void
}

export const useUserRulesStore = create<UserRulesStore>((set, get) => ({
  directory: null,
  exists: false,
  files: [],
  contentByPath: {},
  isLoading: false,
  loadingContentPaths: new Set<string>(),
  savingPaths: new Set<string>(),
  error: null,
  fetch: async () => {
    set({ isLoading: true, error: null })
    try {
      const response = await userRulesApi.list()
      set({
        directory: response.directory,
        exists: response.exists,
        files: response.files,
        isLoading: false,
      })
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : 'Failed to load rules',
      })
    }
  },
  loadContent: async (file) => {
    const cached = get().contentByPath[file.path]
    if (cached !== undefined) return cached
    const next = new Set(get().loadingContentPaths)
    next.add(file.path)
    set({ loadingContentPaths: next })
    try {
      const response = await userRulesApi.get(file.name)
      set((state) => ({
        contentByPath: { ...state.contentByPath, [file.path]: response.content },
      }))
      return response.content
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : 'Failed to load rule content',
      })
      return null
    } finally {
      const after = new Set(get().loadingContentPaths)
      after.delete(file.path)
      set({ loadingContentPaths: after })
    }
  },
  upsert: async (name, content) => {
    const next = new Set(get().savingPaths)
    next.add(name)
    set({ savingPaths: next })
    try {
      await userRulesApi.upsert(name, content)
      await get().fetch()
      const refreshed = get().files.find((f) => f.name === name || f.path.endsWith(name))
      if (refreshed) {
        set((state) => ({
          contentByPath: { ...state.contentByPath, [refreshed.path]: content },
        }))
      }
    } finally {
      const after = new Set(get().savingPaths)
      after.delete(name)
      set({ savingPaths: after })
    }
  },
  delete: async (name) => {
    await userRulesApi.delete(name)
    const target = get().files.find((f) => f.name === name)
    if (target) {
      set((state) => {
        const copy = { ...state.contentByPath }
        delete copy[target.path]
        return { contentByPath: copy }
      })
    }
    await get().fetch()
  },
  clearLocalContent: (path) => {
    set((state) => {
      const copy = { ...state.contentByPath }
      delete copy[path]
      return { contentByPath: copy }
    })
  },
}))
