// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useSettingsStore } from '../../stores/settingsStore'
import { useDesignerStore } from '../../stores/designerStore'
import type { DesignerField } from '../../api/designer'
import { DesignSystemPicker } from './DesignSystemPicker'

const PILL =
  'flex h-[22px] items-center gap-1 rounded-full bg-[var(--color-surface-container-low)] px-2 py-0 text-[11px] font-medium text-[var(--color-text-secondary)] outline-none transition-colors hover:bg-[var(--color-surface-hover)]'
const PILL_ACTIVE =
  'flex h-[22px] items-center gap-1 rounded-full bg-[var(--color-accent)] px-2 py-0 text-[11px] font-medium text-[var(--color-on-accent)] outline-none transition-colors'

export function DesignerInlineControls({ sessionId }: { sessionId: string }) {
  const t = useTranslation()
  const locale = useSettingsStore((s) => s.locale)
  const catalog = useDesignerStore((s) => s.catalog)
  const load = useDesignerStore((s) => s.load)
  const selectedId = useDesignerStore(
    (s) => s.sessions[sessionId]?.selectedSubmodeId ?? null,
  )
  const selectSubmode = useDesignerStore((s) => s.selectSubmode)
  const paramsBySubmode = useDesignerStore(
    (s) => s.sessions[sessionId]?.paramsBySubmode,
  )
  const setParam = useDesignerStore((s) => s.setParam)

  useEffect(() => {
    void load()
  }, [load])

  useEffect(() => {
    const first = catalog[0]
    if (first && !selectedId) {
      selectSubmode(sessionId, first.id)
    }
  }, [catalog, selectedId, selectSubmode, sessionId])

  const label = (en: string, zh: string) => (locale === 'zh' ? zh : en)
  const submode = useMemo(
    () => catalog.find((s) => s.id === selectedId) ?? null,
    [catalog, selectedId],
  )

  if (catalog.length === 0) return null
  const params = (selectedId && paramsBySubmode?.[selectedId]) || {}

  return (
    <div className="flex min-h-[24px] flex-wrap items-center gap-1.5">
      <SubmodePill
        catalog={catalog}
        current={submode}
        label={label}
        onSelect={(id) => selectSubmode(sessionId, id)}
      />
      {submode?.fields.map((field) => (
        <ParamPill
          key={field.key}
          field={field}
          value={params[field.key]}
          label={label}
          onChange={(v) => selectedId && setParam(sessionId, selectedId, field.key, v)}
          t={t}
        />
      ))}
    </div>
  )
}

function usePopover() {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', onDoc)
    return () => document.removeEventListener('mousedown', onDoc)
  }, [open])
  return { open, setOpen, ref }
}

const PANEL =
  'absolute bottom-full left-0 z-[9999] mb-1 max-h-[360px] w-[220px] overflow-y-auto rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] py-1.5 shadow-[var(--shadow-dropdown)]'

type SubmodePillProps = {
  catalog: { id: string; icon: string; labelEn: string; labelZh: string }[]
  current: { id: string; icon: string; labelEn: string; labelZh: string } | null
  label: (en: string, zh: string) => string
  onSelect: (id: string) => void
}

function SubmodePill({ catalog, current, label, onSelect }: SubmodePillProps) {
  const { open, setOpen, ref } = usePopover()
  return (
    <div ref={ref} className="relative">
      <button type="button" onClick={() => setOpen((v) => !v)} className={PILL}>
        <span>{current?.icon ?? '🎨'}</span>
        <span>{current ? label(current.labelEn, current.labelZh) : ''}</span>
        <span className="text-[var(--color-text-secondary)]">▾</span>
      </button>
      {open && (
        <div className={PANEL}>
          {catalog.map((s) => {
            const active = current?.id === s.id
            return (
              <button
                key={s.id}
                type="button"
                onClick={() => {
                  onSelect(s.id)
                  setOpen(false)
                }}
                className={`flex w-full items-center gap-2 px-3 py-2 text-left text-sm transition-colors ${
                  active
                    ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
                    : 'text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]'
                }`}
              >
                <span>{s.icon}</span>
                <span className="truncate">{label(s.labelEn, s.labelZh)}</span>
              </button>
            )
          })}
        </div>
      )}
    </div>
  )
}

type PromptTemplatePillProps = {
  surface: 'image' | 'video'
  value: string
  onChange: (value: unknown) => void
  label: string
  t: ReturnType<typeof useTranslation>
}

function PromptTemplatePill({ surface, value, onChange, label, t }: PromptTemplatePillProps) {
  const { open, setOpen, ref } = usePopover()
  const [query, setQuery] = useState('')
  const promptTemplates = useDesignerStore((s) => s.promptTemplates)

  const items = useMemo(
    () => promptTemplates.filter((tpl) => tpl.surface === surface),
    [promptTemplates, surface],
  )
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return items
    return items.filter(
      (tpl) =>
        tpl.title.toLowerCase().includes(q) ||
        tpl.category.toLowerCase().includes(q) ||
        tpl.tags.some((tag) => tag.toLowerCase().includes(q)),
    )
  }, [items, query])
  const grouped = useMemo(() => {
    const map = new Map<string, typeof filtered>()
    for (const tpl of filtered) {
      const arr = map.get(tpl.category) ?? []
      arr.push(tpl)
      map.set(tpl.category, arr)
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]))
  }, [filtered])

  const selected = items.find((tpl) => tpl.id === value)
  const display = selected ? selected.title : t('designer.inline.none')

  return (
    <div ref={ref} className="relative">
      <button type="button" onClick={() => setOpen((v) => !v)} className={PILL}>
        <span className="text-[var(--color-text-tertiary)]">{label}:</span>
        <span className="max-w-[150px] truncate">{display}</span>
        <span className="text-[var(--color-text-secondary)]">▾</span>
      </button>
      {open && (
        <div className="absolute bottom-full left-0 z-[9999] mb-1 max-h-[360px] w-[320px] overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]">
          <div className="border-b border-[var(--color-border)] p-2">
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('designer.promptTemplate.searchPlaceholder')}
              className="w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
            />
            <div className="mt-1.5 px-0.5 text-[10px] uppercase tracking-wide text-[var(--color-text-secondary)]">
              {t('designer.promptTemplate.count').replace('{n}', String(items.length))}
            </div>
          </div>
          <div className="max-h-[280px] overflow-y-auto py-1">
            <button
              type="button"
              onClick={() => {
                onChange('')
                setOpen(false)
              }}
              className={`flex w-full flex-col px-3 py-2 text-left transition-colors ${
                !value
                  ? 'bg-[var(--color-surface-selected)]'
                  : 'hover:bg-[var(--color-surface-hover)]'
              }`}
            >
              <span className="text-sm text-[var(--color-text-primary)]">
                {t('designer.promptTemplate.none')}
              </span>
              <span className="mt-0.5 text-[11px] text-[var(--color-text-secondary)]">
                {t('designer.promptTemplate.noneHint')}
              </span>
            </button>
            {grouped.map(([category, list]) => (
              <div key={category}>
                <div className="px-3 pb-0.5 pt-2 text-[10px] uppercase tracking-wide text-[var(--color-text-secondary)]">
                  {category}
                </div>
                {list.map((tpl) => (
                  <button
                    key={tpl.id}
                    type="button"
                    onClick={() => {
                      onChange(tpl.id)
                      setOpen(false)
                    }}
                    className={`flex w-full flex-col px-3 py-2 text-left transition-colors ${
                      value === tpl.id
                        ? 'bg-[var(--color-surface-selected)]'
                        : 'hover:bg-[var(--color-surface-hover)]'
                    }`}
                  >
                    <span
                      className={`truncate text-sm ${
                        value === tpl.id
                          ? 'font-medium text-[var(--color-accent)]'
                          : 'text-[var(--color-text-primary)]'
                      }`}
                    >
                      {tpl.title}
                    </span>
                    {tpl.summary ? (
                      <span className="mt-0.5 line-clamp-2 text-[11px] text-[var(--color-text-secondary)]">
                        {tpl.summary}
                      </span>
                    ) : null}
                  </button>
                ))}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

type HtmlTemplatePillProps = {
  value: string
  onChange: (value: unknown) => void
  label: string
  t: ReturnType<typeof useTranslation>
}

function HtmlTemplatePill({ value, onChange, label, t }: HtmlTemplatePillProps) {
  const { open, setOpen, ref } = usePopover()
  const [query, setQuery] = useState('')
  const htmlTemplates = useDesignerStore((s) => s.htmlTemplates)

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return htmlTemplates
    return htmlTemplates.filter(
      (tpl) =>
        tpl.title.toLowerCase().includes(q) ||
        tpl.category.toLowerCase().includes(q) ||
        tpl.summary.toLowerCase().includes(q) ||
        tpl.tags.some((tag) => tag.toLowerCase().includes(q)),
    )
  }, [htmlTemplates, query])
  const grouped = useMemo(() => {
    const map = new Map<string, typeof filtered>()
    for (const tpl of filtered) {
      const arr = map.get(tpl.category) ?? []
      arr.push(tpl)
      map.set(tpl.category, arr)
    }
    return [...map.entries()].sort((a, b) => a[0].localeCompare(b[0]))
  }, [filtered])

  const selected = htmlTemplates.find((tpl) => tpl.id === value)
  const display = selected ? selected.title : t('designer.inline.none')

  return (
    <div ref={ref} className="relative">
      <button type="button" onClick={() => setOpen((v) => !v)} className={PILL}>
        <span className="text-[var(--color-text-tertiary)]">{label}:</span>
        <span className="max-w-[150px] truncate">{display}</span>
        <span className="text-[var(--color-text-secondary)]">▾</span>
      </button>
      {open && (
        <div className="absolute bottom-full left-0 z-[9999] mb-1 max-h-[360px] w-[320px] overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]">
          <div className="border-b border-[var(--color-border)] p-2">
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('designer.htmlTemplate.searchPlaceholder')}
              className="w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
            />
            <div className="mt-1.5 px-0.5 text-[10px] uppercase tracking-wide text-[var(--color-text-secondary)]">
              {t('designer.htmlTemplate.count').replace('{n}', String(htmlTemplates.length))}
            </div>
          </div>
          <div className="max-h-[280px] overflow-y-auto py-1">
            <button
              type="button"
              onClick={() => {
                onChange('')
                setOpen(false)
              }}
              className={`flex w-full flex-col px-3 py-2 text-left transition-colors ${
                !value
                  ? 'bg-[var(--color-surface-selected)]'
                  : 'hover:bg-[var(--color-surface-hover)]'
              }`}
            >
              <span className="text-sm text-[var(--color-text-primary)]">
                {t('designer.htmlTemplate.none')}
              </span>
              <span className="mt-0.5 text-[11px] text-[var(--color-text-secondary)]">
                {t('designer.htmlTemplate.noneHint')}
              </span>
            </button>
            {grouped.map(([category, list]) => (
              <div key={category}>
                <div className="px-3 pb-0.5 pt-2 text-[10px] uppercase tracking-wide text-[var(--color-text-secondary)]">
                  {category}
                </div>
                {list.map((tpl) => (
                  <button
                    key={tpl.id}
                    type="button"
                    onClick={() => {
                      onChange(tpl.id)
                      setOpen(false)
                    }}
                    className={`flex w-full flex-col px-3 py-2 text-left transition-colors ${
                      value === tpl.id
                        ? 'bg-[var(--color-surface-selected)]'
                        : 'hover:bg-[var(--color-surface-hover)]'
                    }`}
                  >
                    <span
                      className={`truncate text-sm ${
                        value === tpl.id
                          ? 'font-medium text-[var(--color-accent)]'
                          : 'text-[var(--color-text-primary)]'
                      }`}
                    >
                      {tpl.title}
                    </span>
                    {tpl.summary ? (
                      <span className="mt-0.5 line-clamp-2 text-[11px] text-[var(--color-text-secondary)]">
                        {tpl.summary}
                      </span>
                    ) : null}
                  </button>
                ))}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}

type ParamPillProps = {
  field: DesignerField
  value: unknown
  label: (en: string, zh: string) => string
  onChange: (value: unknown) => void
  t: ReturnType<typeof useTranslation>
}

function ParamPill({ field, value, label, onChange, t }: ParamPillProps) {
  const { open, setOpen, ref } = usePopover()
  const name = label(field.labelEn, field.labelZh)

  if (field.type === 'designSystem') {
    return (
      <DesignSystemPicker
        value={typeof value === 'string' && value ? value : 'default'}
        onChange={onChange}
        compact
      />
    )
  }

  if (field.type === 'promptTemplate') {
    return (
      <PromptTemplatePill
        surface={field.surface === 'video' ? 'video' : 'image'}
        value={typeof value === 'string' ? value : ''}
        onChange={onChange}
        label={name}
        t={t}
      />
    )
  }

  if (field.type === 'htmlTemplate') {
    return (
      <HtmlTemplatePill
        value={typeof value === 'string' ? value : ''}
        onChange={onChange}
        label={name}
        t={t}
      />
    )
  }

  if (field.type === 'toggle') {
    const on = value === true
    return (
      <button type="button" onClick={() => onChange(!on)} className={on ? PILL_ACTIVE : PILL}>
        <span>{name}</span>
        <span>{on ? '✓' : '✕'}</span>
      </button>
    )
  }

  let display = '—'
  if (field.type === 'select') {
    const opt = (field.options ?? []).find((o) => o.value === value)
    display = opt ? label(opt.labelEn, opt.labelZh) : t('designer.inline.choose')
  } else if (field.type === 'multiselect') {
    const sel = Array.isArray(value) ? (value as string[]) : []
    display = sel.length > 0 ? String(sel.length) : t('designer.inline.any')
  } else if (field.type === 'number') {
    display = typeof value === 'number' ? String(value) : '—'
  } else {
    display = typeof value === 'string' && value ? value : '—'
  }

  return (
    <div ref={ref} className="relative">
      <button type="button" onClick={() => setOpen((v) => !v)} className={PILL}>
        <span className="text-[var(--color-text-tertiary)]">{name}:</span>
        <span className="max-w-[140px] truncate">{display}</span>
        <span className="text-[var(--color-text-secondary)]">▾</span>
      </button>
      {open && (
        <div className={PANEL}>
          {field.type === 'select' &&
            (field.options ?? []).map((opt) => (
              <button
                key={opt.value}
                type="button"
                onClick={() => {
                  onChange(opt.value)
                  setOpen(false)
                }}
                className={`flex w-full items-center px-3 py-2 text-left text-sm transition-colors ${
                  value === opt.value
                    ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
                    : 'text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]'
                }`}
              >
                {label(opt.labelEn, opt.labelZh)}
              </button>
            ))}

          {field.type === 'multiselect' &&
            (() => {
              const sel = Array.isArray(value) ? (value as string[]) : []
              return (field.options ?? []).map((opt) => {
                const on = sel.includes(opt.value)
                return (
                  <button
                    key={opt.value}
                    type="button"
                    onClick={() =>
                      onChange(
                        on ? sel.filter((v) => v !== opt.value) : [...sel, opt.value],
                      )
                    }
                    className={`flex w-full items-center gap-2 px-3 py-2 text-left text-sm transition-colors ${
                      on
                        ? 'text-[var(--color-accent)] hover:bg-[var(--color-surface-hover)]'
                        : 'text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]'
                    }`}
                  >
                    <span>{on ? '☑' : '☐'}</span>
                    <span className="truncate">{label(opt.labelEn, opt.labelZh)}</span>
                  </button>
                )
              })
            })()}

          {(field.type === 'number' || field.type === 'text') && (
            <div className="px-2 py-1.5">
              <input
                type={field.type === 'number' ? 'number' : 'text'}
                min={field.min}
                max={field.max}
                value={
                  field.type === 'number'
                    ? typeof value === 'number'
                      ? value
                      : ''
                    : typeof value === 'string'
                      ? value
                      : ''
                }
                onChange={(e) =>
                  onChange(
                    field.type === 'number' ? Number(e.target.value) : e.target.value,
                  )
                }
                placeholder={
                  field.placeholderEn || field.placeholderZh
                    ? label(field.placeholderEn ?? '', field.placeholderZh ?? '')
                    : undefined
                }
                className="w-full rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
              />
            </div>
          )}
        </div>
      )}
    </div>
  )
}
