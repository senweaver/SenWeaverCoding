// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useDesignerStore } from '../../stores/designerStore'
import type { DesignSystemMeta } from '../../api/designer'

type Props = {
  value: string
  onChange: (id: string) => void
  compact?: boolean
}

const AUTO_ID = 'auto'

export function DesignSystemPicker({ value, onChange, compact = false }: Props) {
  const t = useTranslation()
  const designSystems = useDesignerStore((s) => s.designSystems)
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const rootRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', onDoc)
    return () => document.removeEventListener('mousedown', onDoc)
  }, [open])

  const current = useMemo(() => {
    if (value === AUTO_ID) return { id: AUTO_ID, name: t('designer.designSystem.auto'), category: '', description: '' }
    return (
      designSystems.find((d) => d.id === value) ??
      designSystems.find((d) => d.id === 'default') ?? {
        id: value,
        name: value || t('designer.designSystem.auto'),
        category: '',
        description: '',
      }
    )
  }, [value, designSystems, t])

  const filtered = useMemo(() => {
    if (!open) return designSystems
    const q = query.trim().toLowerCase()
    if (!q) return designSystems
    return designSystems.filter(
      (d) =>
        d.name.toLowerCase().includes(q) ||
        d.id.toLowerCase().includes(q) ||
        d.category.toLowerCase().includes(q) ||
        d.description.toLowerCase().includes(q),
    )
  }, [designSystems, query, open])

  const grouped = useMemo(() => {
    if (!open) return [] as [string, DesignSystemMeta[]][]
    const map = new Map<string, DesignSystemMeta[]>()
    for (const d of filtered) {
      const arr = map.get(d.category) ?? []
      arr.push(d)
      map.set(d.category, arr)
    }
    return Array.from(map.entries())
  }, [filtered, open])

  const totalCount = designSystems.length + 1

  const choose = (id: string) => {
    onChange(id)
    setOpen(false)
    setQuery('')
  }

  return (
    <div ref={rootRef} className={compact ? 'relative' : 'relative flex flex-col gap-1'}>
      {!compact && (
        <span className="text-[11px] text-[var(--color-text-secondary)]">
          {t('designer.designSystem.label')}
        </span>
      )}
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={
          compact
            ? 'flex h-[22px] max-w-[180px] items-center gap-1 rounded-full border border-[var(--color-border)] bg-[var(--color-surface-raised)] px-2 py-0 text-[11px] text-[var(--color-text-secondary)] outline-none hover:border-[var(--color-accent)] hover:text-[var(--color-text-primary)]'
            : 'flex w-full items-center justify-between gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-[12px] text-[var(--color-text-primary)] outline-none hover:border-[var(--color-accent)]'
        }
      >
        {compact && (
          <span className="text-[var(--color-text-tertiary)]">
            {t('designer.designSystem.label')}:
          </span>
        )}
        <span className="truncate">{current.name}</span>
        <span className="text-[var(--color-text-secondary)]">▾</span>
      </button>

      {open && (
        <div className="absolute bottom-full left-0 z-[9999] mb-1 max-h-[360px] w-[280px] overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]">
          <div className="border-b border-[var(--color-border)] p-2">
            <input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('designer.designSystem.searchPlaceholder')}
              className="w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
            />
            <div className="mt-1.5 px-0.5 text-[10px] uppercase tracking-wide text-[var(--color-text-secondary)]">
              {t('designer.designSystem.count').replace('{n}', String(totalCount))}
            </div>
          </div>

          <div className="max-h-[260px] overflow-y-auto py-1">
            <Row
              active={value === AUTO_ID}
              name={t('designer.designSystem.auto')}
              description={t('designer.designSystem.autoHint')}
              onClick={() => choose(AUTO_ID)}
            />
            {grouped.map(([category, items]) => (
              <div key={category}>
                <div className="px-3 pb-0.5 pt-2 text-[10px] uppercase tracking-wide text-[var(--color-text-secondary)]">
                  {category}
                </div>
                {items.map((d) => (
                  <Row
                    key={d.id}
                    active={value === d.id}
                    name={d.name}
                    description={d.description}
                    onClick={() => choose(d.id)}
                  />
                ))}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

function Row({
  active,
  name,
  description,
  onClick,
}: {
  active: boolean
  name: string
  description: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`flex w-full items-start justify-between gap-2 px-3 py-2 text-left transition-colors ${
        active
          ? 'bg-[var(--color-surface-selected)]'
          : 'hover:bg-[var(--color-surface-hover)]'
      }`}
    >
      <span className="min-w-0">
        <span
          className={`block truncate text-sm ${
            active
              ? 'font-medium text-[var(--color-accent)]'
              : 'text-[var(--color-text-primary)]'
          }`}
        >
          {name}
        </span>
        {description ? (
          <span className="mt-0.5 block truncate text-[11px] text-[var(--color-text-secondary)]">
            {description}
          </span>
        ) : null}
      </span>
      {active ? <span className="text-[var(--color-accent)]">✓</span> : null}
    </button>
  )
}
