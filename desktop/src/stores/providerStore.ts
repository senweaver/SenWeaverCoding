

import { create } from 'zustand'
import { providersApi } from '../api/providers'
import { useSettingsStore } from './settingsStore'
import type {
  SavedProvider,
  CreateProviderInput,
  UpdateProviderInput,
  TestProviderConfigInput,
  ProviderTestResult,
} from '../types/provider'
import type { ProviderPreset } from '../types/providerPreset'

type ProviderStore = {
  providers: SavedProvider[]
  activeId: string | null
  presets: ProviderPreset[]
  isLoading: boolean
  isPresetsLoading: boolean
  error: string | null

  fetchProviders: () => Promise<void>
  fetchPresets: () => Promise<void>
  createProvider: (input: CreateProviderInput) => Promise<SavedProvider>
  updateProvider: (id: string, input: UpdateProviderInput) => Promise<SavedProvider>
  deleteProvider: (id: string) => Promise<void>
  activateProvider: (id: string) => Promise<void>
  activateOfficial: () => Promise<void>
  testProvider: (id: string, overrides?: { baseUrl?: string; modelId?: string; apiFormat?: string }) => Promise<ProviderTestResult>
  testConfig: (input: TestProviderConfigInput) => Promise<ProviderTestResult>
}

export const useProviderStore = create<ProviderStore>((set, get) => ({
  providers: [],
  activeId: null,
  presets: [],
  isLoading: false,
  isPresetsLoading: false,
  error: null,

  fetchProviders: async () => {
    set({ isLoading: true, error: null })
    try {
      const { providers, activeId } = await providersApi.list()
      set({ providers, activeId, isLoading: false })
    } catch (err) {
      set({ isLoading: false, error: err instanceof Error ? err.message : String(err) })
    }
  },

  fetchPresets: async () => {
    set({ isPresetsLoading: true, error: null })
    try {
      const { presets } = await providersApi.presets()
      set({ presets, isPresetsLoading: false })
    } catch (err) {
      set({ isPresetsLoading: false, error: err instanceof Error ? err.message : String(err) })
    }
  },

  createProvider: async (input) => {
    const { provider } = await providersApi.create(input)
    await get().fetchProviders()
    return provider
  },

  updateProvider: async (id, input) => {
    const { provider } = await providersApi.update(id, input)
    await get().fetchProviders()
    return provider
  },

  deleteProvider: async (id) => {
    await providersApi.delete(id)
    await get().fetchProviders()
  },

  activateProvider: async (id) => {
    await providersApi.activate(id)
    await get().fetchProviders()

    const provider = get().providers.find((p) => p.id === id)
    const fallbackModel = provider?.models?.find((m) => m.trim().length > 0)
    if (fallbackModel) {
      const settings = useSettingsStore.getState()
      await settings.setModel(fallbackModel)
      await settings.fetchAll()
    }
  },

  activateOfficial: async () => {

    await providersApi.activateOfficial().catch(() => undefined)
    await get().fetchProviders()
  },

  testProvider: async (id, overrides?) => {
    const { result } = await providersApi.test(id, overrides)
    return result
  },

  testConfig: async (input) => {
    const { result } = await providersApi.testConfig(input)
    return result
  },
}))
