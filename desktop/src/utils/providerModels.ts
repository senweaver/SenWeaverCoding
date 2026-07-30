// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { SavedProvider } from '../types/provider'
import { modelTypesForId, type ModelType } from './modelTypes'

export type AggregatedProviderModel = {
  modelId: string
  providerId: string
  providerName: string
  presetName?: string
  isPrimary: boolean
  enabled: boolean
  types: ModelType[]
}

export function isProviderModelEnabled(provider: SavedProvider, modelId: string): boolean {
  return provider.modelEnabled?.[modelId] !== false
}

export function aggregateProviderModels(
  providers: SavedProvider[],
  presetMap?: Map<string, { id: string; name: string }>,
): AggregatedProviderModel[] {
  const out: AggregatedProviderModel[] = []
  for (const provider of providers) {
    const seen = new Set<string>()
    const primaryModelId = provider.models[0]?.trim() ?? ''
    for (const raw of provider.models ?? []) {
      const modelId = raw.trim()
      if (!modelId || seen.has(modelId)) continue
      seen.add(modelId)
      const preset = presetMap?.get(provider.presetId)
      const presetName = preset && preset.id !== 'custom' ? preset.name : undefined
      const dedupedPresetName =
        presetName && presetName.trim().toLowerCase() === provider.name.trim().toLowerCase()
          ? undefined
          : presetName
      out.push({
        modelId,
        providerId: provider.id,
        providerName: provider.name,
        presetName: dedupedPresetName,
        isPrimary: modelId === primaryModelId,
        enabled: isProviderModelEnabled(provider, modelId),
        types: modelTypesForId(provider, modelId),
      })
    }
  }
  return out
}

export function enabledProviderModelIds(provider: SavedProvider): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const raw of provider.models ?? []) {
    const id = raw.trim()
    if (!id || seen.has(id) || !isProviderModelEnabled(provider, id)) continue
    seen.add(id)
    out.push(id)
  }
  return out
}
