// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'

import { integrationsApi, type IntegrationInfo } from '../../api/integrations'
import { useTranslation } from '../../i18n'
import { integrationStatusClass, integrationStatusLabel } from './integrationStatus'

export function SupportedChannels() {
  const t = useTranslation()
  const [items, setItems] = useState<IntegrationInfo[]>([])
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    integrationsApi
      .list()
      .then((res) => {
        if (cancelled) return
        setItems((res.integrations ?? []).filter((entry) => entry.category === 'Chat'))
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

  if (!isLoading && !error && items.length === 0) {
    return null
  }

  return (
    <section className="rounded-xl border border-[var(--color-border)] overflow-hidden">
      <div className="px-3 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
        <div className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('settings.adapters.supportedChannels')}
        </div>
        <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
          {t('settings.adapters.supportedChannelsDesc')}
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
        ) : (
          <div className="grid grid-cols-2 gap-1.5">
            {items.map((entry) => (
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
        )}
      </div>
    </section>
  )
}
