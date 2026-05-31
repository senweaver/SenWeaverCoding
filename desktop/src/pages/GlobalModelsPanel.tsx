// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from '../i18n'
import { Button } from '../components/shared/Button'
import type { SavedProvider, UpdateProviderInput } from '../types/provider'
import type { ProviderPreset } from '../types/providerPreset'
import { aggregateProviderModels } from '../utils/providerModels'

const DEFAULT_VISIBLE_COUNT = 10

function ModelToggleSwitch({
  checked,
  disabled,
  onChange,
}: {
  checked: boolean
  disabled?: boolean
  onChange: () => void
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={onChange}
      className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors flex-shrink-0 ${
        checked ? 'bg-[#90c1f7]' : 'bg-[var(--color-border)]'
      } ${disabled ? 'opacity-60 cursor-not-allowed' : 'cursor-pointer'}`}
    >
      <span
        className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform ${
          checked ? 'translate-x-6' : 'translate-x-1'
        }`}
      />
    </button>
  )
}

export function GlobalModelsPanel({
  providers,
  presetMap,
  onRefresh,
  onUpdateProvider,
}: {
  providers: SavedProvider[]
  presetMap: Map<string, ProviderPreset>
  onRefresh: () => Promise<void>
  onUpdateProvider: (providerId: string, input: UpdateProviderInput) => Promise<SavedProvider>
}) {
  const t = useTranslation()
  const [query, setQuery] = useState('')
  const [showAll, setShowAll] = useState(false)
  const [busy, setBusy] = useState(false)
  const [refreshing, setRefreshing] = useState(false)

  const allModels = useMemo(
    () => aggregateProviderModels(providers, presetMap),
    [providers, presetMap],
  )

  const filteredModels = useMemo(() => {
    const needle = query.trim().toLowerCase()
    if (!needle) return allModels
    return allModels.filter((entry) => {
      const haystack = `${entry.modelId} ${entry.providerName} ${entry.presetName ?? ''}`.toLowerCase()
      return haystack.includes(needle)
    })
  }, [allModels, query])

  useEffect(() => {
    setShowAll(false)
  }, [query])

  const hasOverflow = filteredModels.length > DEFAULT_VISIBLE_COUNT
  const visibleModels = showAll ? filteredModels : filteredModels.slice(0, DEFAULT_VISIBLE_COUNT)

  const handleRefresh = async () => {
    setRefreshing(true)
    try {
      await onRefresh()
    } finally {
      setRefreshing(false)
    }
  }

  const handleToggle = async (providerId: string, modelId: string, enabled: boolean) => {
    setBusy(true)
    try {
      await onUpdateProvider(providerId, { modelEnabled: { [modelId]: enabled } })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container)] mb-4">
      <div className="flex items-center justify-between gap-3 px-4 py-3 border-b border-[var(--color-border-separator)]">
        <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('settings.providers.modelsHeader')}
        </h3>
        {hasOverflow && (
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setShowAll((prev) => !prev)}
          >
            {showAll ? t('settings.providers.showLessModels') : t('settings.providers.viewAllModels')}
            <span className="material-symbols-outlined text-[14px]">
              {showAll ? 'expand_less' : 'expand_more'}
            </span>
          </Button>
        )}
      </div>
      <div className="px-4 py-3">
        <div className="text-xs text-[var(--color-text-tertiary)] mb-3">
          {t('settings.providers.globalModelsHelp')}
        </div>
        <div className="flex items-center gap-2 mb-3">
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('settings.providers.searchAllModels')}
            disabled={busy}
            className="flex-1 min-w-0 h-8 px-2.5 rounded-md bg-[var(--color-surface-container)] border border-[var(--color-border)] text-xs text-[var(--color-text-primary)] placeholder:text-[var(--color-text-tertiary)] outline-none focus:border-[var(--color-border-focus)]"
          />
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void handleRefresh()}
            loading={refreshing}
            disabled={busy}
            title={t('settings.providers.refreshModels')}
          >
            <span className="material-symbols-outlined text-[16px]">refresh</span>
          </Button>
        </div>
        {allModels.length === 0 ? (
          <div className="text-xs text-[var(--color-text-tertiary)] italic py-2">
            {t('settings.providers.noModels')}
          </div>
        ) : filteredModels.length === 0 ? (
          <div className="text-xs text-[var(--color-text-tertiary)] italic py-2">
            {t('settings.providers.noModelsMatch')}
          </div>
        ) : (
          <div className="flex flex-col gap-1 max-h-[420px] overflow-y-auto pr-1">
            {visibleModels.map((entry) => (
              <div
                key={`${entry.providerId}:${entry.modelId}`}
                className="flex items-center justify-between gap-3 px-3 py-2 rounded-md bg-[var(--color-surface-container-low)] border border-[var(--color-border)]"
              >
                <div className="flex items-center gap-2 min-w-0 flex-1">
                  <span className="text-xs font-mono text-[var(--color-text-primary)] truncate">
                    {entry.modelId}
                  </span>
                  <span className="px-1.5 py-0.5 text-[10px] font-medium rounded bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)] leading-none flex-shrink-0 truncate max-w-[120px]">
                    {entry.providerName}
                  </span>
                  {entry.presetName && (
                    <span className="px-1.5 py-0.5 text-[10px] font-medium rounded bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)] leading-none flex-shrink-0">
                      {entry.presetName}
                    </span>
                  )}
                  {entry.isPrimary && (
                    <span className="px-1.5 py-0.5 text-[10px] font-bold rounded border border-[var(--color-brand)]/18 bg-[var(--color-brand)]/14 text-[var(--color-brand)] leading-none flex-shrink-0">
                      {t('settings.providers.primaryTag')}
                    </span>
                  )}
                </div>
                <ModelToggleSwitch
                  checked={entry.enabled}
                  disabled={busy}
                  onChange={() => void handleToggle(entry.providerId, entry.modelId, !entry.enabled)}
                />
              </div>
            ))}
          </div>
        )}
        {hasOverflow && !showAll && (
          <div className="mt-2 text-xs text-[var(--color-text-tertiary)]">
            {t('settings.providers.modelsShowingCount', {
              shown: String(visibleModels.length),
              total: String(filteredModels.length),
            })}
          </div>
        )}
      </div>
    </div>
  )
}
