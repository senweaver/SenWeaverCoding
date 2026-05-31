// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { webApi } from '../api/web'
import type { WebFetchConfig, WebSearchConfig } from '../api/web'

type WebResearchStore = {
  webSearch: WebSearchConfig | null
  webFetch: WebFetchConfig | null
  isLoading: boolean
  isSaving: boolean
  hasFetched: boolean
  error: string | null

  fetch: () => Promise<void>
  updateWebSearch: (patch: Partial<WebSearchConfig>) => Promise<void>
  updateWebFetch: (patch: Partial<WebFetchConfig>) => Promise<void>
}

function readError(err: unknown): string {
  if (err instanceof Error) return err.message
  return 'Web research request failed'
}

export const useWebResearchStore = create<WebResearchStore>((set, get) => ({
  webSearch: null,
  webFetch: null,
  isLoading: false,
  isSaving: false,
  hasFetched: false,
  error: null,

  fetch: async () => {
    if (get().isLoading) return
    set({ isLoading: true, error: null })
    try {
      const [search, fetchCfg] = await Promise.all([
        webApi.getSearch(),
        webApi.getFetch(),
      ])
      set({
        webSearch: search,
        webFetch: fetchCfg,
        isLoading: false,
        hasFetched: true,
      })
    } catch (err) {
      set({ isLoading: false, error: readError(err) })
    }
  },

  updateWebSearch: async (patch) => {
    const previous = get().webSearch
    if (previous) {
      set({ webSearch: { ...previous, ...patch }, isSaving: true, error: null })
    } else {
      set({ isSaving: true, error: null })
    }
    try {
      const next = await webApi.updateSearch(patch)
      set({ webSearch: next, isSaving: false })
    } catch (err) {
      set({ webSearch: previous, isSaving: false, error: readError(err) })
      throw err
    }
  },

  updateWebFetch: async (patch) => {
    const previous = get().webFetch
    if (previous) {
      set({ webFetch: { ...previous, ...patch }, isSaving: true, error: null })
    } else {
      set({ isSaving: true, error: null })
    }
    try {
      const next = await webApi.updateFetch(patch)
      set({ webFetch: next, isSaving: false })
    } catch (err) {
      set({ webFetch: previous, isSaving: false, error: readError(err) })
      throw err
    }
  },
}))
