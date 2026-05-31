// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type { ModelInfo, EffortLevel } from '../types/settings'
import type { AvailableModelsResponse } from '../types/evolution'

type ModelsResponse = { models: ModelInfo[]; provider: { id: string; name: string } | null }
type CurrentModelResponse = { model: ModelInfo }
type EffortResponse = { level: EffortLevel; available: EffortLevel[] }

export const modelsApi = {
  list() {
    return api.get<ModelsResponse>('/api/models')
  },

  listAvailable() {
    return api.get<AvailableModelsResponse>('/api/models/available')
  },

  getCurrent() {
    return api.get<CurrentModelResponse>('/api/models/current')
  },

  setCurrent(modelId: string) {
    return api.put<{ ok: true; model: string }>('/api/models/current', { modelId })
  },

  getEffort() {
    return api.get<EffortResponse>('/api/effort')
  },

  setEffort(level: EffortLevel) {
    return api.put<{ ok: true; level: EffortLevel }>('/api/effort', { level })
  },
}
