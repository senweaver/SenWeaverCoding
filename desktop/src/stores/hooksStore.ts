import { create } from 'zustand'
import { hooksApi } from '../api/hooks'
import type { HooksConfig, HooksPatch } from '../types/hooks'

type HooksStore = {
  config: HooksConfig | null
  isLoading: boolean
  isSaving: boolean
  error: string | null

  fetch: () => Promise<void>
  update: (patch: HooksPatch) => Promise<void>
}

export const useHooksStore = create<HooksStore>((set) => ({
  config: null,
  isLoading: false,
  isSaving: false,
  error: null,

  fetch: async () => {
    set({ isLoading: true, error: null })
    try {
      const config = await hooksApi.get()
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
      const config = await hooksApi.update(patch)
      set({ config, isSaving: false })
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },
}))
