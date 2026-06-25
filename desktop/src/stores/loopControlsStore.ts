// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { settingsApi } from '../api/settings'
import type { LoopControlsSettings } from '../types/settings'

type LoopControlsStore = {
  data: LoopControlsSettings | null
  isLoading: boolean
  isSaving: boolean
  error: string | null
  hasFetched: boolean

  fetch: () => Promise<void>
  updatePartial: (patch: Partial<LoopControlsSettings>) => Promise<void>
  applyServer: (next: LoopControlsSettings) => void
}

const DEFAULT_LOOP_CONTROLS: LoopControlsSettings = {
  selfEvalEnabled: false,
  evaluateCodeEdits: false,
  evaluatorModel: '',
  maxEvaluatorRetries: 2,
  frozenRubricPath: '',
  maxCostPerDayCents: 500,
  estopEnabled: false,
  costTrackingEnabled: false,
}

export const useLoopControlsStore = create<LoopControlsStore>((set, get) => ({
  data: null,
  isLoading: false,
  isSaving: false,
  error: null,
  hasFetched: false,

  fetch: async () => {
    if (get().isLoading) return
    set({ isLoading: true, error: null })
    try {
      const view = await settingsApi.getLoopControls()
      set({
        data: { ...DEFAULT_LOOP_CONTROLS, ...view },
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
    const base = get().data ?? DEFAULT_LOOP_CONTROLS
    const optimistic: LoopControlsSettings = { ...base, ...patch }
    set({ data: optimistic, isSaving: true, error: null })
    try {
      const view = await settingsApi.setLoopControls(patch)
      set({ data: { ...DEFAULT_LOOP_CONTROLS, ...view }, isSaving: false })
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      set({ data: base, isSaving: false, error: message })
      throw e
    }
  },

  applyServer: (next) => {
    set({ data: { ...DEFAULT_LOOP_CONTROLS, ...next }, hasFetched: true })
  },
}))
