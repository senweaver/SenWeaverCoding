// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { customToolsApi } from '../api/customTools'
import type { CustomToolDef, CustomToolPatch } from '../types/customTools'

type CustomToolsStore = {
  tools: CustomToolDef[]
  isLoading: boolean
  isSaving: boolean
  error: string | null

  fetch: () => Promise<void>
  create: (def: CustomToolDef) => Promise<CustomToolDef>
  update: (name: string, patch: CustomToolPatch) => Promise<CustomToolDef>
  remove: (name: string) => Promise<void>
}

export const useCustomToolsStore = create<CustomToolsStore>((set, get) => ({
  tools: [],
  isLoading: false,
  isSaving: false,
  error: null,

  fetch: async () => {
    set({ isLoading: true, error: null })
    try {
      const { tools } = await customToolsApi.list()
      set({ tools, isLoading: false })
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },

  create: async (def) => {
    set({ isSaving: true, error: null })
    try {
      const { tool } = await customToolsApi.create(def)
      set({ tools: [...get().tools, tool], isSaving: false })
      return tool
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },

  update: async (name, patch) => {
    set({ isSaving: true, error: null })
    try {
      const { tool } = await customToolsApi.update(name, patch)
      set({
        tools: get().tools.map((t) => (t.name === name ? tool : t)),
        isSaving: false,
      })
      return tool
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },

  remove: async (name) => {
    set({ isSaving: true, error: null })
    try {
      await customToolsApi.remove(name)
      set({
        tools: get().tools.filter((t) => t.name !== name),
        isSaving: false,
      })
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },
}))
