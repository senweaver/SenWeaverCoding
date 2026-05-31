// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useProviderStore } from '../stores/providerStore'
import { useSettingsStore } from '../stores/settingsStore'
import type { SavedProvider } from '../types/provider'
import { resolveEffectiveRuntimeSelection } from './runtimeSelection'

export const NO_MODEL_CONFIGURED_CODE = 'NO_MODEL_CONFIGURED'

function providerHasUsableModel(provider: SavedProvider): boolean {
  if (!provider) return false
  const list = provider.models ?? []
  for (const raw of list) {
    if (typeof raw === 'string' && raw.trim().length > 0) return true
  }
  return false
}

export function anyProviderHasModel(providers: SavedProvider[]): boolean {
  return providers.some(providerHasUsableModel)
}

export function hasAnyAvailableModel(): boolean {
  const providers = useProviderStore.getState().providers
  if (anyProviderHasModel(providers)) return true
  const settings = useSettingsStore.getState()
  if ((settings.availableModels?.length ?? 0) > 0) return true
  if (settings.currentModel) return true
  return false
}

export function hasUsableModelForSession(sessionId: string | null | undefined): boolean {
  if (sessionId) {
    const providers = useProviderStore.getState().providers
    const activeId = useProviderStore.getState().activeId
    const settingsModelId = useSettingsStore.getState().currentModel?.id
    const runtime = resolveEffectiveRuntimeSelection(
      sessionId,
      providers,
      activeId,
      settingsModelId,
    )
    if (runtime?.providerId && runtime.modelId.trim()) return true
  }
  return hasAnyAvailableModel()
}

const NO_MODEL_PATTERNS: RegExp[] = [
  /no[_\s-]?model[_\s-]?configured/i,
  /please\s+add\s+at\s+least\s+one\s+model/i,
  /未添加模型/,
]

export function isNoModelConfiguredError(
  message: string | null | undefined,
  code?: string | null | undefined,
): boolean {
  if (code && code === NO_MODEL_CONFIGURED_CODE) return true
  if (!message) return false
  for (const re of NO_MODEL_PATTERNS) {
    if (re.test(message)) return true
  }
  return false
}
