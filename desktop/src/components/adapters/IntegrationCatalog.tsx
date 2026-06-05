// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'

import { integrationsApi, type IntegrationInfo } from '../../api/integrations'
import { useTranslation } from '../../i18n'
import {
  humanizeCategory,
  integrationStatusClass,
  integrationStatusLabel,
} from './integrationStatus'

export function IntegrationCatalog() {
  const t = useTranslation()
  const [integrations, setIntegrations] = useState<IntegrationInfo[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setIsLoading(true)
    setError(null)
    integrationsApi
      .list()
      .then((res) => {
        if (cancelled) return
        setIntegrations(res.integrations ?? [])
      })
      .catch((err) => {
        if (cancelled) return
        setError(err instanceof Error ? err.message : String(err))
      })
      .finally(() => {
        if (!cancelled) setIsLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [])

  const grouped = useMemo(() => {
    const map = new Map<string, IntegrationInfo[]>()
    for (const entry of integrations) {
      if (entry.category === 'Chat') continue
      const key = entry.category || 'Other'
      const list = map.get(key) ?? []
      list.push(entry)
      map.set(key, list)
    }
    return Array.from(map.entries())
  }, [integrations])

  return (
    <section className="rounded-xl border border-[var(--color-border)] overflow-hidden">
      <div className="px-3 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
        <div className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('settings.integrations.title')}
        </div>
        <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
          {t('settings.integrations.description')}
        </div>
      </div>
      <div className="p-3">
        {isLoading ? (
          <div className="flex justify-center py-4">
            <div className="animate-spin w-5 h-5 border-2 border-[var(--color-brand)] border-t-transparent rounded-full" />
          </div>
        ) : error ? (
          <div className="text-xs text-[var(--color-error)]">
            {t('settings.integrations.error', { error })}
          </div>
        ) : grouped.length === 0 ? (
          <div className="text-xs text-[var(--color-text-tertiary)] py-6 text-center border border-dashed border-[var(--color-border)] rounded-xl">
            {t('settings.integrations.empty')}
          </div>
        ) : (
          <div className="flex flex-col gap-4">
            {grouped.map(([category, entries]) => (
              <div key={category}>
                <div className="text-[10px] uppercase tracking-wider font-semibold text-[var(--color-text-tertiary)] mb-1.5">
                  {humanizeCategory(category)}
                </div>
                <div className="grid grid-cols-2 gap-1.5">
                  {entries.map((entry) => (
                    <div
                      key={entry.name}
                      className="flex items-center gap-3 px-3 py-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-low)]"
                    >
                      <div className="flex-1 min-w-0">
                        <div className="text-xs font-semibold text-[var(--color-text-primary)] truncate">
                          {entry.name}
                        </div>
                        <div className="text-xs text-[var(--color-text-tertiary)] truncate mt-0.5">
                          {entry.description}
                        </div>
                      </div>
                      <span
                        className={`px-1.5 py-0.5 text-[10px] font-bold rounded border leading-none flex-shrink-0 ${integrationStatusClass(entry.status)}`}
                      >
                        {integrationStatusLabel(t, entry.status)}
                      </span>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  )
}
