// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'

import { useTranslation, type TranslationKey } from '../../i18n'
import { useChatStore } from '../../stores/chatStore'

interface PrivacyBannerProps {
  sessionId: string | null | undefined
}

const KIND_ORDER = [
  'id_card',
  'phone',
  'email',
  'bank_card',
  'jwt',
  'api_key',
  'bearer',
  'auth_header',
  'url_password',
  'kv_secret',
  'private_key',
  'ipv4',
  'mac',
] as const

const LEGACY_KIND_ALIAS: Record<string, string> = {
  authorization_header: 'auth_header',
  mac_address: 'mac',
}

function kindLabel(
  t: (key: TranslationKey, params?: Record<string, string | number>) => string,
  kind: string,
): string {
  const key = `debug.privacy.categories.${kind}` as TranslationKey
  const translated = t(key)
  if (translated && translated !== (key as string)) return translated
  return kind
}

export function PrivacyBanner({ sessionId }: PrivacyBannerProps) {
  const t = useTranslation()
  const stats = useChatStore((s) =>
    sessionId ? s.sessions[sessionId]?.debugPiiStats : undefined,
  )
  const resetStats = useChatStore((s) => s.resetDebugPiiStats)
  const [expanded, setExpanded] = useState(false)

  const total = stats?.total ?? 0
  const counts = stats?.counts ?? {}

  const sortedEntries = useMemo(() => {
    const folded = new Map<string, number>()
    for (const [rawKey, value] of Object.entries(counts)) {
      if (!value || value <= 0) continue
      const key = LEGACY_KIND_ALIAS[rawKey] ?? rawKey
      folded.set(key, (folded.get(key) ?? 0) + value)
    }
    const entries = Array.from(folded.entries())
    entries.sort((a, b) => {
      const ai = KIND_ORDER.indexOf(a[0] as (typeof KIND_ORDER)[number])
      const bi = KIND_ORDER.indexOf(b[0] as (typeof KIND_ORDER)[number])
      const norm = (idx: number) => (idx < 0 ? KIND_ORDER.length : idx)
      const diff = norm(ai) - norm(bi)
      if (diff !== 0) return diff
      return a[0].localeCompare(b[0])
    })
    return entries
  }, [counts])

  const handleToggle = () => setExpanded((prev) => !prev)

  return (
    <div
      className="mb-1 flex flex-col gap-1 rounded-md border border-[var(--color-brand)]/30 bg-[var(--color-brand)]/5 px-2.5 py-1.5 text-[12px] text-[var(--color-text-secondary)]"
      role="status"
      aria-live="polite"
    >
      <div className="flex items-center gap-2">
        <span
          className="material-symbols-outlined text-[14px] text-[var(--color-brand)]"
          aria-hidden="true"
        >
          privacy_tip
        </span>
        <span className="flex-1 leading-tight">
          {t('debug.privacy.banner.collapsed')}
        </span>
        <span
          className="rounded-full border border-[var(--color-brand)]/40 px-1.5 py-px text-[11px] font-medium text-[var(--color-brand)]"
          title={t('debug.privacy.banner.totalLabel')}
        >
          {total}
        </span>
        <button
          type="button"
          onClick={handleToggle}
          className="inline-flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          aria-label={
            expanded
              ? t('debug.privacy.banner.collapse')
              : t('debug.privacy.banner.expand')
          }
          aria-expanded={expanded}
        >
          <span className="material-symbols-outlined text-[14px]">
            {expanded ? 'expand_less' : 'expand_more'}
          </span>
        </button>
      </div>
      {expanded && (
        <div className="flex flex-col gap-1.5 border-t border-[var(--color-brand)]/20 pt-1.5">
          <p className="leading-tight text-[var(--color-text-tertiary)]">
            {t('debug.privacy.banner.expanded')}
          </p>
          {sortedEntries.length === 0 ? (
            <p className="leading-tight text-[var(--color-text-tertiary)]">
              {t('debug.privacy.banner.empty')}
            </p>
          ) : (
            <div className="grid grid-cols-2 gap-x-3 gap-y-0.5 sm:grid-cols-3">
              {sortedEntries.map(([kind, count]) => (
                <div
                  key={kind}
                  className="flex items-center justify-between gap-2 text-[11.5px]"
                >
                  <span className="truncate text-[var(--color-text-secondary)]">
                    {kindLabel(t, kind)}
                  </span>
                  <span className="font-mono text-[var(--color-text-primary)]">
                    {count}
                  </span>
                </div>
              ))}
            </div>
          )}
          <div className="flex items-center justify-end gap-2 pt-0.5">
            <button
              type="button"
              onClick={() => sessionId && resetStats(sessionId)}
              className="rounded border border-[var(--color-border)] px-2 py-0.5 text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
            >
              {t('debug.privacy.banner.clear')}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}

export default PrivacyBanner
