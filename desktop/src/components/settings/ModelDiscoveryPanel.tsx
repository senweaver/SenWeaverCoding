// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { Button } from '../shared/Button'
import { providersApi } from '../../api/providers'
import { ApiError } from '../../api/client'
import { useTranslation, type TranslationKey } from '../../i18n'
import type { ApiFormat, DiscoveredModel } from '../../types/provider'
import { MODEL_TYPES, modelTypeLabelKey } from '../../utils/modelTypes'

type ModelDiscoveryPanelProps = {
  baseUrl: string
  apiFormat: ApiFormat
  apiKey: string
  presetId: string
  providerId?: string
  existingModelIds: string[]
  onApply: (models: DiscoveredModel[]) => void
}

const ALL_TAB = '__all__'

const DISCOVERY_CACHE_PREFIX = 'sen.modelDiscovery.v1.'

type DiscoveryCacheEntry = {
  models: DiscoveredModel[]
  fetchedAt: number
}

function discoveryCacheKey(
  providerId: string | undefined,
  apiFormat: ApiFormat,
  baseUrl: string,
): string {
  const id = providerId?.trim()
  if (id) {
    return `${DISCOVERY_CACHE_PREFIX}id.${id}`
  }
  const normalizedBase = baseUrl.trim().toLowerCase().replace(/\/+$/, '')
  return `${DISCOVERY_CACHE_PREFIX}url.${apiFormat}.${normalizedBase}`
}

function loadDiscoveryCache(key: string): DiscoveryCacheEntry | null {
  try {
    const raw = localStorage.getItem(key)
    if (!raw) return null
    const parsed = JSON.parse(raw) as DiscoveryCacheEntry
    if (!parsed || !Array.isArray(parsed.models)) return null
    return parsed
  } catch {
    return null
  }
}

function saveDiscoveryCache(key: string, models: DiscoveredModel[]) {
  try {
    const entry: DiscoveryCacheEntry = { models, fetchedAt: Date.now() }
    localStorage.setItem(key, JSON.stringify(entry))
  } catch {
    return
  }
}

export function ModelDiscoveryPanel({
  baseUrl,
  apiFormat,
  apiKey,
  presetId,
  providerId,
  existingModelIds,
  onApply,
}: ModelDiscoveryPanelProps) {
  const t = useTranslation()
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [fetched, setFetched] = useState<DiscoveredModel[] | null>(null)
  const [source, setSource] = useState<'remote' | 'cache' | null>(null)
  const [query, setQuery] = useState('')
  const [activeTab, setActiveTab] = useState<string>(ALL_TAB)
  const [selected, setSelected] = useState<Set<string>>(() => new Set())

  const cacheKey = useMemo(
    () => discoveryCacheKey(providerId, apiFormat, baseUrl),
    [providerId, apiFormat, baseUrl],
  )

  useEffect(() => {
    const cached = loadDiscoveryCache(cacheKey)
    if (cached) {
      setFetched(cached.models)
      setSource('cache')
    } else {
      setFetched(null)
      setSource(null)
    }
    setError(null)
    setSelected(new Set())
    setActiveTab(ALL_TAB)
    setQuery('')
  }, [cacheKey])

  const existingSet = useMemo(
    () => new Set(existingModelIds.map((id) => id.trim()).filter(Boolean)),
    [existingModelIds],
  )

  const handleFetch = async () => {
    setLoading(true)
    setError(null)
    try {
      const result = await providersApi.discoverModels({
        baseUrl: baseUrl.trim(),
        apiFormat,
        apiKey: apiKey.trim() ? apiKey.trim() : undefined,
        presetId: presetId || undefined,
        providerId: providerId || undefined,
      })
      const models = result.models ?? []
      setFetched(models)
      setSource('remote')
      saveDiscoveryCache(cacheKey, models)
      setSelected(new Set())
      setActiveTab(ALL_TAB)
      setQuery('')
    } catch (e) {
      const message =
        e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e)
      setError(message)
    } finally {
      setLoading(false)
    }
  }

  const availableTabs = useMemo(() => {
    if (!fetched) return [] as string[]
    const present = new Set<string>()
    for (const model of fetched) {
      for (const type of model.types ?? []) present.add(type)
    }
    return MODEL_TYPES.filter((type) => present.has(type))
  }, [fetched])

  const visibleModels = useMemo(() => {
    if (!fetched) return [] as DiscoveredModel[]
    const needle = query.trim().toLowerCase()
    return fetched.filter((model) => {
      if (activeTab !== ALL_TAB && !(model.types ?? []).includes(activeTab)) return false
      if (needle && !model.id.toLowerCase().includes(needle)) return false
      return true
    })
  }, [fetched, query, activeTab])

  const selectableVisible = useMemo(
    () => visibleModels.filter((model) => !existingSet.has(model.id.trim())),
    [visibleModels, existingSet],
  )

  const allVisibleSelected =
    selectableVisible.length > 0 && selectableVisible.every((model) => selected.has(model.id))

  const toggleOne = (id: string) => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const toggleSelectAll = () => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (allVisibleSelected) {
        for (const model of selectableVisible) next.delete(model.id)
      } else {
        for (const model of selectableVisible) next.add(model.id)
      }
      return next
    })
  }

  const handleApply = () => {
    if (!fetched) return
    const chosen = fetched.filter((model) => selected.has(model.id))
    if (chosen.length === 0) return
    onApply(chosen)
    setSelected(new Set())
  }

  return (
    <div className="mb-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface-container-low)] p-2.5">
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs font-medium text-[var(--color-text-secondary)]">
          {t('settings.providers.discoverTitle')}
        </span>
        <Button
          variant="secondary"
          size="sm"
          onClick={handleFetch}
          loading={loading}
          disabled={baseUrl.trim().length === 0}
          icon={<span className="material-symbols-outlined text-[14px]">cloud_download</span>}
        >
          {t('settings.providers.fetchModelList')}
        </Button>
      </div>

      {error && (
        <div className="mt-2 text-[11px] text-[var(--color-error)] break-words">
          {t('settings.providers.discoverError')}: {error}
        </div>
      )}

      {fetched && !error && (
        <div className="mt-2">
          {fetched.length === 0 ? (
            <div className="text-[11px] italic text-[var(--color-text-tertiary)] px-1 py-2">
              {t('settings.providers.discoverEmpty')}
            </div>
          ) : (
            <>
              <div className="flex items-center justify-between gap-2 mb-1.5">
                <span className="text-[11px] text-[var(--color-text-tertiary)]">
                  {t('settings.providers.discoveredCount', {
                    count: fetched.length,
                    source: t(
                      source === 'cache'
                        ? 'settings.providers.discoverSourceCache'
                        : 'settings.providers.discoverSourceRemote',
                    ),
                  })}
                </span>
                <button
                  type="button"
                  onClick={toggleSelectAll}
                  disabled={selectableVisible.length === 0}
                  className="text-[11px] text-[var(--color-brand)] hover:underline disabled:opacity-40 disabled:no-underline"
                >
                  {allVisibleSelected
                    ? t('settings.providers.deselectAll')
                    : t('settings.providers.selectAll')}
                </button>
              </div>

              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t('settings.providers.searchModels')}
                className="w-full h-8 px-2.5 mb-1.5 rounded-md bg-[var(--color-surface-container)] border border-[var(--color-border)] text-xs text-[var(--color-text-primary)] placeholder:text-[var(--color-text-tertiary)] outline-none focus:border-[var(--color-border-focus)]"
              />

              {availableTabs.length > 0 && (
                <div className="flex flex-wrap items-center gap-1 mb-1.5">
                  <button
                    type="button"
                    onClick={() => setActiveTab(ALL_TAB)}
                    className={`px-2 py-0.5 text-[10px] font-medium rounded leading-none transition-colors ${
                      activeTab === ALL_TAB
                        ? 'border border-[var(--color-brand)]/18 bg-[var(--color-brand)]/14 text-[var(--color-brand)]'
                        : 'border border-transparent bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]'
                    }`}
                  >
                    {t('settings.providers.allTab')}
                  </button>
                  {availableTabs.map((type) => (
                    <button
                      key={type}
                      type="button"
                      onClick={() => setActiveTab(type)}
                      className={`px-2 py-0.5 text-[10px] font-medium rounded leading-none transition-colors ${
                        activeTab === type
                          ? 'border border-[var(--color-brand)]/18 bg-[var(--color-brand)]/14 text-[var(--color-brand)]'
                          : 'border border-transparent bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]'
                      }`}
                    >
                      {t(modelTypeLabelKey(type) as TranslationKey)}
                    </button>
                  ))}
                </div>
              )}

              <div className="max-h-52 overflow-y-auto flex flex-col gap-0.5 pr-0.5">
                {visibleModels.length === 0 ? (
                  <div className="text-[11px] italic text-[var(--color-text-tertiary)] px-1 py-2">
                    {t('settings.providers.discoverEmpty')}
                  </div>
                ) : (
                  visibleModels.map((model) => {
                    const already = existingSet.has(model.id.trim())
                    const checked = already || selected.has(model.id)
                    return (
                      <label
                        key={model.id}
                        className={`flex items-center gap-2 px-2 py-1 rounded-md ${
                          already
                            ? 'opacity-50 cursor-default'
                            : 'cursor-pointer hover:bg-[var(--color-surface-container-high)]'
                        }`}
                      >
                        <input
                          type="checkbox"
                          checked={checked}
                          disabled={already}
                          onChange={() => toggleOne(model.id)}
                          className="accent-[var(--color-brand)]"
                        />
                        <span className="text-xs font-mono text-[var(--color-text-primary)] truncate flex-1">
                          {model.id}
                        </span>
                        {(model.types ?? [])
                          .filter((type) => type !== 'text')
                          .map((type) => (
                            <span
                              key={type}
                              className="px-1.5 py-0.5 text-[10px] font-medium rounded bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)] leading-none shrink-0"
                            >
                              {t(modelTypeLabelKey(type) as TranslationKey)}
                            </span>
                          ))}
                        {already && (
                          <span className="px-1.5 py-0.5 text-[10px] font-bold rounded border border-[var(--color-brand)]/18 bg-[var(--color-brand)]/14 text-[var(--color-brand)] leading-none shrink-0">
                            {t('settings.providers.alreadyAdded')}
                          </span>
                        )}
                      </label>
                    )
                  })
                )}
              </div>

              <div className="flex justify-end mt-2">
                <Button
                  variant="primary"
                  size="sm"
                  onClick={handleApply}
                  disabled={selected.size === 0}
                >
                  {t('settings.providers.applySelected', { count: selected.size })}
                </Button>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  )
}
