// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { agentSettingsApi } from '../api/agentSettings'
import type {
  AgentCoreConfig,
  AgentCorePatch,
  AgentRuntimeConfig,
  AgentRuntimePatch,
  WebFetchPatch,
  WebFetchSettings,
  WebSearchPatch,
  WebSearchSettings,
} from '../types/agentSettings'

type AgentSettingsStore = {
  agentConfig: AgentCoreConfig | null
  agentRuntime: AgentRuntimeConfig | null
  webSearch: WebSearchSettings | null
  webFetch: WebFetchSettings | null

  isLoading: boolean
  isSaving: boolean
  error: string | null

  fetchAll: () => Promise<void>
  refresh: () => Promise<void>
  updateAgent: (patch: AgentCorePatch) => Promise<void>
  updateRuntime: (patch: AgentRuntimePatch) => Promise<void>
  updateWebSearch: (patch: WebSearchPatch) => Promise<void>
  updateWebFetch: (patch: WebFetchPatch) => Promise<void>
}

export const useAgentSettingsStore = create<AgentSettingsStore>((set, get) => ({
  agentConfig: null,
  agentRuntime: null,
  webSearch: null,
  webFetch: null,
  isLoading: false,
  isSaving: false,
  error: null,

  fetchAll: async () => {
    set({ isLoading: get().agentConfig === null, error: null })
    try {
      const [agentConfig, agentRuntime, webSearch, webFetch] = await Promise.all([
        agentSettingsApi.getAgentConfig(),
        agentSettingsApi.getAgentRuntime(),
        agentSettingsApi.getWebSearch(),
        agentSettingsApi.getWebFetch(),
      ])
      set({ agentConfig, agentRuntime, webSearch, webFetch, isLoading: false })
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },

  refresh: async () => {
    await useAgentSettingsStore.getState().fetchAll()
  },

  updateAgent: async (patch) => {
    set({ isSaving: true, error: null })
    try {
      const next = await agentSettingsApi.updateAgentConfig(patch)
      set({ agentConfig: next, isSaving: false })
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },

  updateRuntime: async (patch) => {
    set({ isSaving: true, error: null })
    try {
      const next = await agentSettingsApi.updateAgentRuntime(patch)
      set({ agentRuntime: next, isSaving: false })
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },

  updateWebSearch: async (patch) => {
    set({ isSaving: true, error: null })
    try {
      const next = await agentSettingsApi.updateWebSearch(patch)
      set({ webSearch: next, isSaving: false })
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },

  updateWebFetch: async (patch) => {
    set({ isSaving: true, error: null })
    try {
      const next = await agentSettingsApi.updateWebFetch(patch)
      set({ webFetch: next, isSaving: false })
    } catch (err) {
      set({
        isSaving: false,
        error: err instanceof Error ? err.message : String(err),
      })
      throw err
    }
  },
}))
