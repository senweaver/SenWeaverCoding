// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useSettingsStore } from '../../stores/settingsStore'
import { useDebugStore } from '../../stores/debugStore'
import { useChatStore } from '../../stores/chatStore'
import type { DebugField } from '../../api/debug'

const PILL =
  'flex h-[22px] items-center gap-1 rounded-full bg-[var(--color-surface-container-low)] px-2 py-0 text-[11px] font-medium text-[var(--color-text-secondary)] outline-none transition-colors hover:bg-[var(--color-surface-hover)]'
const PILL_ACTIVE =
  'flex h-[22px] items-center gap-1 rounded-full bg-[var(--color-accent)] px-2 py-0 text-[11px] font-medium text-[var(--color-on-accent)] outline-none transition-colors'
const PANEL =
  'absolute bottom-full left-0 z-[9999] mb-1 max-h-[360px] w-[220px] overflow-y-auto rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] py-1.5 shadow-[var(--shadow-dropdown)]'

export function DebugInlineControls({ sessionId }: { sessionId: string }) {
  const t = useTranslation()
  const locale = useSettingsStore((s) => s.locale)
  const catalog = useDebugStore((s) => s.catalog)
  const loaded = useDebugStore((s) => s.loaded)
  const load = useDebugStore((s) => s.load)
  const selectedId = useDebugStore((s) => s.sessions[sessionId]?.selectedSubmodeId ?? null)
  const selectSubmode = useDebugStore((s) => s.selectSubmode)
  const paramsBySubmode = useDebugStore((s) => s.sessions[sessionId]?.paramsBySubmode)
  const setParam = useDebugStore((s) => s.setParam)
  const setSessionDebugSubmode = useChatStore((s) => s.setSessionDebugSubmode)

  useEffect(() => {
    if (loaded) return
    void load()
    const timer = window.setInterval(() => {
      if (useDebugStore.getState().loaded) {
        window.clearInterval(timer)
        return
      }
      void load()
    }, 1500)
    return () => window.clearInterval(timer)
  }, [loaded, load])

  useEffect(() => {
    const first = catalog[0]
    if (first && !selectedId) {
      selectSubmode(sessionId, first.id)
    }
  }, [catalog, selectedId, selectSubmode, sessionId])

  const params = useMemo(
    () => (selectedId && paramsBySubmode?.[selectedId]) || {},
    [selectedId, paramsBySubmode],
  )

  useEffect(() => {
    if (!selectedId) return
    setSessionDebugSubmode(sessionId, selectedId, params)
  }, [sessionId, selectedId, params, setSessionDebugSubmode])

  const label = (en: string, zh: string) => (locale === 'zh' ? zh : en)
  const submode = useMemo(
    () => catalog.find((s) => s.id === selectedId) ?? null,
    [catalog, selectedId],
  )

  if (catalog.length === 0) return null

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
        <span>{current?.icon ?? '🐞'}</span>
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

type ParamPillProps = {
  field: DebugField
  value: unknown
  label: (en: string, zh: string) => string
  onChange: (value: unknown) => void
  t: ReturnType<typeof useTranslation>
}

function ParamPill({ field, value, label, onChange, t }: ParamPillProps) {
  const { open, setOpen, ref } = usePopover()
  const name = label(field.labelEn, field.labelZh)

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
                      onChange(on ? sel.filter((v) => v !== opt.value) : [...sel, opt.value])
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
                  onChange(field.type === 'number' ? Number(e.target.value) : e.target.value)
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
