import { create } from 'zustand'
import { usageApi } from '../api/usage'
import type { UsageSummary } from '../types/usage'

type UsageStore = {
  summary: UsageSummary | null
  isLoading: boolean
  error: string | null

  fetch: () => Promise<void>
}

export const useUsageStore = create<UsageStore>((set) => ({
  summary: null,
  isLoading: false,
  error: null,

  fetch: async () => {
    set({ isLoading: true, error: null })
    try {
      const summary = await usageApi.get('all')
      set({ summary, isLoading: false })
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },
}))
