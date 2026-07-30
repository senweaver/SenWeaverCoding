// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'
import { useTranslation, type TranslationKey } from '../i18n'
import { Button } from '../components/shared/Button'
import type { SavedProvider, UpdateProviderInput } from '../types/provider'
import { modelTypeLabelKey, modelTypesForId } from '../utils/modelTypes'

export function ProviderModelsPanel({
  provider,
  onUpdateProvider,
  onEditModels,
}: {
  provider: SavedProvider
  onUpdateProvider: (providerId: string, input: UpdateProviderInput) => Promise<SavedProvider>
  onEditModels: () => void
}) {
  const t = useTranslation()
  const [busy, setBusy] = useState(false)

  const models = useMemo(() => {
    const seen = new Set<string>()
    const out: string[] = []
    for (const raw of provider.models ?? []) {
      const modelId = raw.trim()
      if (!modelId || seen.has(modelId)) continue
      seen.add(modelId)
      out.push(modelId)
    }
    return out
  }, [provider.models])

  const handleRemove = async (modelId: string) => {
    if (models.length <= 1) return
    setBusy(true)
    try {
      const nextModels = models.filter((m) => m !== modelId)
      const nextContextWindows = { ...(provider.modelContextWindows ?? {}) }
      delete nextContextWindows[modelId]
      const nextEnabled = { ...(provider.modelEnabled ?? {}) }
      delete nextEnabled[modelId]
      await onUpdateProvider(provider.id, {
        models: nextModels,
        modelContextWindows: nextContextWindows,
        modelEnabled: nextEnabled,
      })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div>
      <div className="text-xs font-medium text-[var(--color-text-primary)] mb-1">
        {t('settings.providers.modelsHeader')}
      </div>
      <div className="text-xs text-[var(--color-text-tertiary)] mb-2">
        {t('settings.providers.modelsHelp')}
      </div>
      {models.length === 0 ? (
        <div className="text-xs text-[var(--color-text-tertiary)] italic py-2">
          {t('settings.providers.noModels')}
        </div>
      ) : (
        <div className="flex flex-col gap-1">
          {models.map((modelId) => (
            <div
              key={modelId}
              className="flex items-center justify-between gap-2 px-3 py-1.5 rounded-md bg-[var(--color-surface-container-low)] border border-[var(--color-border)]"
            >
              <div className="flex items-center flex-wrap gap-x-2 gap-y-1 min-w-0 flex-1">
                <span className="text-xs font-mono text-[var(--color-text-primary)] truncate min-w-0">
                  {modelId}
                </span>
                {modelTypesForId(provider, modelId).map((type) => (
                  <span
                    key={type}
                    className="px-1.5 py-0.5 text-[10px] font-medium rounded bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)] leading-none flex-shrink-0"
                  >
                    {t(modelTypeLabelKey(type) as TranslationKey)}
                  </span>
                ))}
              </div>
              <button
                type="button"
                onClick={() => void handleRemove(modelId)}
                disabled={busy || models.length <= 1}
                className="text-[var(--color-text-tertiary)] hover:text-[var(--color-error)] flex-shrink-0 disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:text-[var(--color-text-tertiary)]"
                title={t('common.delete')}
              >
                <span className="material-symbols-outlined text-[18px]">close</span>
              </button>
            </div>
          ))}
        </div>
      )}
      <div className="mt-2 flex justify-end">
        <Button variant="ghost" size="sm" onClick={onEditModels} disabled={busy}>
          <span className="material-symbols-outlined text-[14px]">edit</span>
          {t('settings.providers.editModels')}
        </Button>
      </div>
    </div>
  )
}
