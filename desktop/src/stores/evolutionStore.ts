// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { evolutionApi } from '../api/evolution'
import { modelsApi } from '../api/models'
import type {
  AvailableModelEntry,
  AvailableModelsResponse,
  AvailableProviderEntry,
  CloudTarget,
  EvolutionConfigState,
  EvolutionExportFormatId,
  EvolutionExportRecord,
  EvolutionLesson,
  EvolutionOverview,
  EvolutionPersistenceStatus,
  ExperienceRecyclingConfig,
  PurgeScopeId,
  PushReceiptView,
  RecycledExperienceItem,
  ReflectionRunItem,
  ReflectionSummary,
  SelfReflectionConfig,
} from '../types/evolution'

type EvolutionStore = {
  overview: EvolutionOverview | null
  config: EvolutionConfigState | null
  lessons: EvolutionLesson[]
  persistence: EvolutionPersistenceStatus | null
  exportFormats: Array<{ id: EvolutionExportFormatId; label: string }>
  exports: EvolutionExportRecord[]
  cloudTargets: CloudTarget[]
  pushHistory: PushReceiptView[]
  recyclingConfig: ExperienceRecyclingConfig | null
  recyclingItems: RecycledExperienceItem[]
  recyclingTotal: number
  reflectionConfig: SelfReflectionConfig | null
  reflectionRuns: ReflectionRunItem[]
  reflectionSummary: ReflectionSummary | null
  availableModels: AvailableModelEntry[]
  availableProviders: AvailableProviderEntry[]
  availableModelsTotal: number
  availableModelsProvidersConfigured: number
  reflectionStoreError: string | null
  lastPersistAutoEnabledAt: number | null
  loading: boolean
  error: string | null
  fetchAll: () => Promise<void>
  fetchAvailableModels: () => Promise<void>
  fetchOverview: () => Promise<void>
  fetchConfig: () => Promise<void>
  fetchLessons: () => Promise<void>
  fetchPersistence: () => Promise<void>
  fetchExportFormats: () => Promise<void>
  fetchExports: () => Promise<void>
  fetchCloudTargets: () => Promise<void>
  fetchPushHistory: () => Promise<void>
  fetchRecyclingConfig: () => Promise<void>
  fetchRecyclingRecent: () => Promise<void>
  fetchReflectionConfig: () => Promise<void>
  fetchReflectionRuns: () => Promise<void>
  updateConfig: (patch: Partial<EvolutionConfigState>) => Promise<void>
  updateLesson: (id: string, patch: Partial<EvolutionLesson>) => Promise<void>
  deleteLesson: (id: string) => Promise<void>
  setPersistence: (persist: boolean) => Promise<void>
  purge: (scope: PurgeScopeId, beforeMs: number | null) => Promise<void>
  createExport: (
    format: EvolutionExportFormatId,
    filter?: Record<string, unknown>,
    options?: Record<string, unknown>,
  ) => Promise<EvolutionExportRecord | null>
  deleteExport: (id: string) => Promise<void>
  upsertCloudTarget: (
    target: Partial<CloudTarget> & {
      name: string
      kind: CloudTarget['kind']
      endpoint: string
      enabled: boolean
    },
  ) => Promise<CloudTarget | null>
  deleteCloudTarget: (id: string) => Promise<void>
  push: (targetId: string, exportId: string) => Promise<PushReceiptView | null>
  distillTurn: (
    turnId: string,
  ) => Promise<{ ok: boolean; queued: boolean; turnId: string } | null>
  rescoreAll: () => Promise<
    { ok: boolean; rescored: number; errors: number; totalSeen: number } | null
  >
  updateRecyclingConfig: (patch: Partial<ExperienceRecyclingConfig>) => Promise<void>
  purgeRecycling: () => Promise<number>
  updateReflectionConfig: (patch: Partial<SelfReflectionConfig>) => Promise<void>
  triggerReflection: (sessionId?: string | null) => Promise<string | null>
}

export const useEvolutionStore = create<EvolutionStore>((set, get) => ({
  overview: null,
  config: null,
  lessons: [],
  persistence: null,
  exportFormats: [],
  exports: [],
  cloudTargets: [],
  pushHistory: [],
  recyclingConfig: null,
  recyclingItems: [],
  recyclingTotal: 0,
  reflectionConfig: null,
  reflectionRuns: [],
  reflectionSummary: null,
  availableModels: [],
  availableProviders: [],
  availableModelsTotal: 0,
  availableModelsProvidersConfigured: 0,
  reflectionStoreError: null,
  lastPersistAutoEnabledAt: null,
  loading: false,
  error: null,

  async fetchAll() {
    set({ loading: get().overview === null, error: null })
    try {
      await Promise.all([
        get().fetchOverview(),
        get().fetchConfig(),
        get().fetchLessons(),
        get().fetchPersistence(),
        get().fetchExportFormats(),
        get().fetchExports(),
        get().fetchCloudTargets(),
        get().fetchPushHistory(),
        get().fetchRecyclingConfig(),
        get().fetchRecyclingRecent(),
        get().fetchReflectionConfig(),
        get().fetchReflectionRuns(),
        get().fetchAvailableModels(),
      ])
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'failed' })
    } finally {
      set({ loading: false })
    }
  },

  async fetchAvailableModels() {
    try {
      const result: AvailableModelsResponse = await modelsApi.listAvailable()
      set({
        availableModels: result.models,
        availableProviders: result.providers,
        availableModelsTotal: result.total,
        availableModelsProvidersConfigured: result.providersConfigured,
      })
    } catch (error) {
      console.warn('evolution available models failed', error)
    }
  },

  async fetchOverview() {
    try {
      const overview = await evolutionApi.fetchOverview()
      set({ overview })
    } catch (error) {
      console.warn('evolution overview failed', error)
    }
  },

  async fetchConfig() {
    try {
      const config = await evolutionApi.fetchConfig()
      set({ config })
    } catch (error) {
      console.warn('evolution config failed', error)
    }
  },

  async fetchLessons() {
    try {
      const result = await evolutionApi.fetchLessons()
      set({ lessons: result.items })
    } catch (error) {
      console.warn('evolution lessons failed', error)
    }
  },

  async fetchPersistence() {
    try {
      const persistence = await evolutionApi.fetchPersistence()
      set({ persistence })
    } catch (error) {
      console.warn('evolution persistence failed', error)
    }
  },

  async fetchExportFormats() {
    try {
      const result = await evolutionApi.fetchExportFormats()
      set({ exportFormats: result.items })
    } catch (error) {
      console.warn('evolution export formats failed', error)
    }
  },

  async fetchExports() {
    try {
      const result = await evolutionApi.fetchExports()
      set({ exports: result.items })
    } catch (error) {
      console.warn('evolution exports failed', error)
    }
  },

  async fetchCloudTargets() {
    try {
      const result = await evolutionApi.fetchCloudTargets()
      set({ cloudTargets: result.items })
    } catch (error) {
      console.warn('evolution cloud targets failed', error)
    }
  },

  async fetchPushHistory() {
    try {
      const result = await evolutionApi.fetchPushHistory()
      set({ pushHistory: result.items })
    } catch (error) {
      console.warn('evolution push history failed', error)
    }
  },

  async updateConfig(patch) {
    await evolutionApi.updateConfig(patch)
    await get().fetchConfig()
    await get().fetchOverview()
  },

  async updateLesson(id, patch) {
    await evolutionApi.updateLesson(id, patch)
    await get().fetchLessons()
  },

  async deleteLesson(id) {
    await evolutionApi.deleteLesson(id)
    await get().fetchLessons()
    await get().fetchOverview()
  },

  async setPersistence(persist) {
    await evolutionApi.setPersistence(persist)
    await Promise.all([get().fetchPersistence(), get().fetchConfig(), get().fetchOverview()])
  },

  async purge(scope, beforeMs) {
    await evolutionApi.purgePersistence(scope, beforeMs)
    await Promise.all([
      get().fetchPersistence(),
      get().fetchOverview(),
      get().fetchExports(),
      get().fetchPushHistory(),
    ])
  },

  async createExport(format, filter, options) {
    try {
      const record = await evolutionApi.createExport({ format, filter, options })
      await get().fetchExports()
      await get().fetchOverview()
      return record
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'failed' })
      return null
    }
  },

  async deleteExport(id) {
    await evolutionApi.deleteExport(id)
    await get().fetchExports()
    await get().fetchOverview()
  },

  async upsertCloudTarget(target) {
    try {
      const created = await evolutionApi.upsertCloudTarget(target as never)
      await get().fetchCloudTargets()
      return created
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'failed' })
      return null
    }
  },

  async deleteCloudTarget(id) {
    await evolutionApi.deleteCloudTarget(id)
    await get().fetchCloudTargets()
  },

  async push(targetId, exportId) {
    try {
      const receipt = await evolutionApi.push(targetId, exportId)
      await get().fetchPushHistory()
      return receipt
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'failed' })
      return null
    }
  },

  async distillTurn(turnId) {
    try {
      const result = await evolutionApi.distillTurn(turnId)
      await Promise.all([get().fetchLessons(), get().fetchOverview()])
      return result
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'failed' })
      return null
    }
  },

  async rescoreAll() {
    try {
      const result = await evolutionApi.rescoreAll()
      await get().fetchOverview()
      return result
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'failed' })
      return null
    }
  },

  async fetchRecyclingConfig() {
    try {
      const cfg = await evolutionApi.fetchRecyclingConfig()
      set({ recyclingConfig: cfg })
    } catch (error) {
      console.warn('evolution recycling config failed', error)
    }
  },

  async fetchRecyclingRecent() {
    try {
      const result = await evolutionApi.fetchRecyclingRecent()
      set({ recyclingItems: result.items, recyclingTotal: result.total })
    } catch (error) {
      console.warn('evolution recycling recent failed', error)
    }
  },

  async fetchReflectionConfig() {
    try {
      const cfg = await evolutionApi.fetchReflectionConfig()
      set({ reflectionConfig: cfg })
    } catch (error) {
      console.warn('evolution reflection config failed', error)
    }
  },

  async fetchReflectionRuns() {
    try {
      const result = await evolutionApi.fetchReflectionRuns()
      set({
        reflectionRuns: result.items,
        reflectionSummary: result.summary,
        reflectionStoreError: null,
      })
    } catch (error) {
      const message = error instanceof Error ? error.message : 'unknown'
      const isStoreError = message.includes('reflection_store_unavailable')
      set({
        reflectionStoreError: isStoreError ? message : null,
      })
      console.warn('evolution reflection runs failed', error)
    }
  },

  async updateRecyclingConfig(patch) {
    try {
      const next = (await evolutionApi.updateRecyclingConfig(patch)) as
        | (ExperienceRecyclingConfig & { persistTrainingDataAutoEnabled?: boolean })
        | null
      if (next) {
        set({ recyclingConfig: next })
        if (next.persistTrainingDataAutoEnabled === true) {
          set({ lastPersistAutoEnabledAt: Date.now() })
          await Promise.all([
            get().fetchConfig(),
            get().fetchPersistence(),
            get().fetchOverview(),
          ])
        }
      }
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'failed' })
    }
  },

  async purgeRecycling() {
    try {
      const result = await evolutionApi.purgeRecycling()
      await get().fetchRecyclingRecent()
      return result.removed
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'failed' })
      return 0
    }
  },

  async updateReflectionConfig(patch) {
    try {
      const next = (await evolutionApi.updateReflectionConfig(patch)) as
        | (SelfReflectionConfig & { persistTrainingDataAutoEnabled?: boolean })
        | null
      if (next) {
        set({ reflectionConfig: next })
        if (next.persistTrainingDataAutoEnabled === true) {
          set({ lastPersistAutoEnabledAt: Date.now() })
          await Promise.all([
            get().fetchConfig(),
            get().fetchPersistence(),
            get().fetchOverview(),
          ])
        }
      }
    } catch (error) {
      set({ error: error instanceof Error ? error.message : 'failed' })
    }
  },

  async triggerReflection(sessionId) {
    try {
      const result = await evolutionApi.triggerReflection(sessionId ?? null)
      await get().fetchReflectionRuns()
      return result.runId
    } catch (error) {
      const message = error instanceof Error ? error.message : 'failed'
      set({ error: message })
      throw error instanceof Error ? error : new Error(message)
    }
  },
}))
