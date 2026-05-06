// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
//
// Validation helpers for the per-session `RuntimeSelection` we persist in
// `useSessionRuntimeStore`.
//
// Earlier builds shipped a hard-coded "default Claude" model catalog
// (`claude-opus-4-7` / `claude-sonnet-4-6` / `claude-haiku-4-5`) that the
// composer would auto-select when no per-session override existed. That
// catalog has been removed, but the previously-written `localStorage`
// entries can still surface those phantom IDs after an upgrade. The
// helpers below let UI code drop selections that no longer correspond to
// a configured provider/model and pick a sane fallback derived from the
// user's actual provider profiles.

import type { RuntimeSelection } from '../types/runtime'
import type { SavedProvider } from '../types/provider'

function normalizedModels(provider: SavedProvider): string[] {
  return (provider.models ?? [])
    .map((m) => (typeof m === 'string' ? m.trim() : ''))
    .filter((m) => m.length > 0)
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
