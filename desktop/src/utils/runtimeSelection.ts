// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { DRAFT_RUNTIME_SELECTION_KEY, useSessionRuntimeStore } from '../stores/sessionRuntimeStore'
import type { RuntimeSelection } from '../types/runtime'
import type { SavedProvider } from '../types/provider'
import { enabledProviderModelIds } from './providerModels'

function normalizedModels(provider: SavedProvider): string[] {
  return enabledProviderModelIds(provider)
}

export function isValidRuntimeSelection(
  selection: RuntimeSelection | null | undefined,
  providers: SavedProvider[],
): selection is RuntimeSelection {
  if (!selection) return false
  const trimmedModel = (selection.modelId ?? '').trim()
  if (!trimmedModel) return false
  if (!selection.providerId) return false
  const provider = providers.find((p) => p.id === selection.providerId)
  if (!provider) return false
  return normalizedModels(provider).includes(trimmedModel)
}

export function pickFirstConfiguredSelection(
  providers: SavedProvider[],
  preferredProviderId: string | null,
): RuntimeSelection | null {
  if (providers.length === 0) return null

  const ordered = preferredProviderId
    ? [
        ...providers.filter((p) => p.id === preferredProviderId),
        ...providers.filter((p) => p.id !== preferredProviderId),
      ]
    : providers

  for (const provider of ordered) {
    const [model] = normalizedModels(provider)
    if (model) {
      return { providerId: provider.id, modelId: model }
    }
  }
  return null
}

export function resolveEffectiveRuntimeSelection(
  sessionKey: string | null | undefined,
  providers: SavedProvider[],
  preferredProviderId: string | null,
  settingsModelId: string | undefined,
): RuntimeSelection | null {
  const selections = useSessionRuntimeStore.getState().selections
  const allowDraftFallback = !sessionKey || sessionKey === DRAFT_RUNTIME_SELECTION_KEY
  const keysToTry: string[] = []
  if (sessionKey) keysToTry.push(sessionKey)
  if (allowDraftFallback && sessionKey !== DRAFT_RUNTIME_SELECTION_KEY) {
    keysToTry.push(DRAFT_RUNTIME_SELECTION_KEY)
  }

  for (const key of keysToTry) {
    const stored = selections[key]
    if (isValidRuntimeSelection(stored, providers)) {
      return stored
    }
  }

  const trimmedSettings = settingsModelId?.trim()
  if (trimmedSettings) {
    for (const provider of providers) {
      if (enabledProviderModelIds(provider).includes(trimmedSettings)) {
        return { providerId: provider.id, modelId: trimmedSettings }
      }
    }
  }

  return pickFirstConfiguredSelection(providers, preferredProviderId)
}

export function persistRuntimeSelection(
  sessionKey: string | null | undefined,
  selection: RuntimeSelection,
): void {
  const isDraftScenario = !sessionKey || sessionKey === DRAFT_RUNTIME_SELECTION_KEY
  if (sessionKey) {
    useSessionRuntimeStore.getState().setSelection(sessionKey, selection)
  }
  if (isDraftScenario) {
    useSessionRuntimeStore.getState().setSelection(DRAFT_RUNTIME_SELECTION_KEY, selection)
  }
}
