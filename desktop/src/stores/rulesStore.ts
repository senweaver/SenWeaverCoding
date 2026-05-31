// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { guardrailsApi, type GuardrailsPatch } from '../api/guardrails'
import type { GuardrailsConfig } from '../types/rules'

type RulesStore = {
  config: GuardrailsConfig | null
  isLoading: boolean
  isSaving: boolean
  error: string | null

  fetch: () => Promise<void>
  update: (patch: GuardrailsPatch) => Promise<void>
}

export const useRulesStore = create<RulesStore>((set, get) => ({
  config: null,
  isLoading: false,
  isSaving: false,
  error: null,

  fetch: async () => {
    set({ isLoading: true, error: null })
    try {
      const config = await guardrailsApi.get()
      set({ config, isLoading: false })
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },

  update: async (patch) => {
    set({ isSaving: true, error: null })
    try {
      await guardrailsApi.update(patch)

      const config = await guardrailsApi.get()
      set({ config, isSaving: false })
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
    void get
  },
}))
