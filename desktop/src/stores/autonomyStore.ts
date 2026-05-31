// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { settingsApi } from '../api/settings'
import type { AutonomySettings } from '../types/settings'

type AutonomyStore = {
  data: AutonomySettings | null
  isLoading: boolean
  isSaving: boolean
  error: string | null
  hasFetched: boolean

  fetch: () => Promise<void>
  updatePartial: (patch: Partial<AutonomySettings>) => Promise<void>

  applyServer: (next: AutonomySettings) => void
}

const DEFAULT_AUTONOMY: AutonomySettings = {
  autoApprove: [],
  alwaysAsk: [],
  protectBrowserTools: true,
  protectMcpTools: true,
  autoApproveModeTransitions: [],
  enableCommandPolicy: false,
}

export const useAutonomyStore = create<AutonomyStore>((set, get) => ({
  data: null,
  isLoading: false,
  isSaving: false,
  error: null,
  hasFetched: false,

  fetch: async () => {
    if (get().isLoading) return
    set({ isLoading: true, error: null })
    try {
      const view = await settingsApi.getAutonomy()
      set({
        data: { ...DEFAULT_AUTONOMY, ...view },
        isLoading: false,
        hasFetched: true,
      })
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      set({ isLoading: false, error: message })
    }
  },

  updatePartial: async (patch) => {
    const current = get().data
    if (!current) {

      await get().fetch()
    }
    const base = get().data ?? DEFAULT_AUTONOMY
    const optimistic: AutonomySettings = { ...base, ...patch }
    set({ data: optimistic, isSaving: true, error: null })
    try {
      const view = await settingsApi.setAutonomy(patch)
      set({ data: { ...DEFAULT_AUTONOMY, ...view }, isSaving: false })
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      set({ data: base, isSaving: false, error: message })
      throw e
    }
  },

  applyServer: (next) => {
    set({ data: { ...DEFAULT_AUTONOMY, ...next }, hasFetched: true })
  },
}))
