// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { runtimeApi } from '../api/runtime'
import type { RuntimeSnapshot } from '../types/runtime'

type RuntimeStore = {
  snapshot: RuntimeSnapshot | null
  isLoading: boolean
  error: string | null
  fetch: () => Promise<void>
}

export const useRuntimeStore = create<RuntimeStore>((set) => ({
  snapshot: null,
  isLoading: false,
  error: null,

  fetch: async () => {
    set({ isLoading: true, error: null })
    try {
      const snapshot = await runtimeApi.snapshot()
      set({ snapshot, isLoading: false })
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },
}))
