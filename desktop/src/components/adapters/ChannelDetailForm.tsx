// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import { useTranslation } from '../../i18n'
import type { TranslationKey } from '../../i18n/locales/en'
import type { ChannelDefinition, ChannelField } from './channelDefinitions'

type Value = Record<string, unknown> | null | undefined

type Props = {
  definition: ChannelDefinition
  value: Value
  onChange: (next: Record<string, unknown>) => void
  disabled?: boolean
}

export function ChannelDetailForm({ definition, value, onChange, disabled }: Props) {
  const t = useTranslation()
  const obj: Record<string, unknown> = (value as Record<string, unknown>) ?? {}

  function setField(key: string, fieldValue: unknown) {
    const next = { ...obj }
    if (fieldValue === undefined) {
      delete next[key]
    } else {
      next[key] = fieldValue
    }
    onChange(next)
  }

  return (
    <div className="space-y-4">
      {definition.i18nNotice && (
        <div className="flex items-start gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-hover)] px-3 py-2">
          <span className="material-symbols-outlined text-[16px] text-[var(--color-text-secondary)]">info</span>
          <p className="text-xs text-[var(--color-text-secondary)] whitespace-pre-line">
            {t(definition.i18nNotice as TranslationKey)}
          </p>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3">
        {definition.fields.map((field) => (
          <div key={field.key} className={field.span === 2 ? 'col-span-2' : 'col-span-1'}>
            <FieldRenderer
              field={field}
              value={obj[field.key]}
              onChange={(v) => setField(field.key, v)}
              disabled={disabled}
            />
          </div>
        ))}
      </div>
    </div>
  )
}

type FieldRendererProps = {
  field: ChannelField
  value: unknown
  onChange: (value: unknown) => void
  disabled?: boolean
}

function FieldRenderer({ field, value, onChange, disabled }: FieldRendererProps) {
  const t = useTranslation()
  const label = t(field.i18nLabel as TranslationKey)
  const placeholder = field.i18nPlaceholder ? t(field.i18nPlaceholder as TranslationKey) : undefined
  const hint = field.i18nHint ? t(field.i18nHint as TranslationKey) : undefined

  const labelEl = (
    <label className="block text-xs font-medium text-[var(--color-text-primary)] mb-1">
      {label}
      {field.required && <span className="text-[var(--color-error)] ml-0.5">*</span>}
    </label>
  )
  const hintEl = hint ? (
    <p className="mt-1 text-xs text-[var(--color-text-tertiary)]">{hint}</p>
  ) : null

  if (field.type === 'checkbox') {
    return (
      <div>
        <label className="flex items-start gap-2 cursor-pointer">
          <input
            type="checkbox"
            checked={Boolean(value)}
            onChange={(e) => onChange(e.target.checked)}
            disabled={disabled}
            className="mt-0.5 w-4 h-4 rounded border-[var(--color-border)] accent-[var(--color-brand)]"
          />
          <div className="flex-1 min-w-0">
            <span className="text-xs text-[var(--color-text-primary)]">{label}</span>
            {hint && <p className="mt-0.5 text-xs text-[var(--color-text-tertiary)]">{hint}</p>}
          </div>
        </label>
      </div>
    )
  }

  if (field.type === 'tribool') {
    const current = value === true ? 'on' : value === false ? 'off' : 'follow'
    return (
      <div>
        {labelEl}
        <select
          value={current}
          onChange={(e) => {
            const v = e.target.value
            if (v === 'follow') onChange(null)
            else if (v === 'on') onChange(true)
            else onChange(false)
          }}
          disabled={disabled}
          className={selectClass}
        >
          <option value="follow">{t('settings.adapters.tribool.follow')}</option>
          <option value="on">{t('settings.adapters.tribool.on')}</option>
          <option value="off">{t('settings.adapters.tribool.off')}</option>
        </select>
        {hintEl}
      </div>
    )
  }

  if (field.type === 'select') {
    return (
      <div>
        {labelEl}
        <select
          value={typeof value === 'string' ? value : ''}
          onChange={(e) => onChange(e.target.value || undefined)}
          disabled={disabled}
          className={selectClass}
        >
          <option value="">{t('settings.adapters.select.unset')}</option>
          {(field.options ?? []).map((opt) => (
            <option key={opt.value} value={opt.value}>
              {t(opt.i18nLabel as TranslationKey)}
            </option>
          ))}
        </select>
        {hintEl}
      </div>
    )
  }

  if (field.type === 'number') {
    return (
      <div>
        {labelEl}
        <input
          type="number"
          value={typeof value === 'number' ? value : value === undefined || value === null ? '' : String(value)}
          onChange={(e) => {
            const raw = e.target.value
            if (raw === '') {
              onChange(undefined)
            } else {
              const n = Number(raw)
              onChange(Number.isFinite(n) ? n : undefined)
            }
          }}
          placeholder={placeholder}
          disabled={disabled}
          className={inputClass}
        />
        {hintEl}
      </div>
    )
  }

  if (field.type === 'password') {
    return (
      <PasswordField
        label={labelEl}
        value={typeof value === 'string' ? value : ''}
        onChange={(v) => onChange(v === '' ? undefined : v)}
        placeholder={placeholder}
        disabled={disabled}
        hint={hintEl}
      />
    )
  }

  if (field.type === 'csv' || field.type === 'csv_number') {
    const arr = Array.isArray(value) ? value : []
    const display = arr.join(', ')
    return (
      <div>
        {labelEl}
        <input
          type="text"
          value={display}
          onChange={(e) => {
            const raw = e.target.value
            const parts = raw
              .split(',')
              .map((s) => s.trim())
              .filter(Boolean)
            if (field.type === 'csv_number') {
              const nums = parts.map(Number).filter((n) => Number.isFinite(n))
              onChange(nums)
            } else {
              onChange(parts)
            }
          }}
          placeholder={placeholder}
          disabled={disabled}
          className={inputClass}
        />
        {hintEl}
      </div>
    )
  }

  if (field.type === 'textarea') {
    return (
      <div>
        {labelEl}
        <textarea
          value={typeof value === 'string' ? value : ''}
          onChange={(e) => onChange(e.target.value || undefined)}
          placeholder={placeholder}
          disabled={disabled}
          rows={3}
          className={`${inputClass} h-auto py-2 resize-y`}
        />
        {hintEl}
      </div>
    )
  }

  return (
    <div>
      {labelEl}
      <input
        type="text"
        value={typeof value === 'string' ? value : ''}
        onChange={(e) => onChange(e.target.value || undefined)}
        placeholder={placeholder}
        disabled={disabled}
        className={inputClass}
      />
      {hintEl}
    </div>
  )
}

const inputClass = `
  h-8 w-full px-2.5 rounded-[var(--radius-md)] border text-xs
  bg-[var(--color-surface)] text-[var(--color-text-primary)]
  placeholder:text-[var(--color-text-tertiary)]
  border-[var(--color-border)]
  focus:border-[var(--color-border-focus)] focus:shadow-[var(--shadow-focus-ring)]
  outline-none transition-colors
  disabled:opacity-50
`

const selectClass = `${inputClass} cursor-pointer`

function PasswordField({
  label,
  value,
  onChange,
  placeholder,
  disabled,
  hint,
}: {
  label: React.ReactNode
  value: string
  onChange: (v: string) => void
  placeholder?: string
  disabled?: boolean
  hint?: React.ReactNode
}) {
  const [shown, setShown] = useState(false)
  return (
    <div>
      {label}
      <div className="relative">
        <input
          type={shown ? 'text' : 'password'}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={placeholder}
          disabled={disabled}
          className={`${inputClass} pr-9`}
        />
        <button
          type="button"
          onClick={() => setShown((s) => !s)}
          disabled={disabled}
          className="absolute right-2 top-1/2 -translate-y-1/2 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)] disabled:opacity-50"
          tabIndex={-1}
        >
          <span className="material-symbols-outlined text-[14px]">{shown ? 'visibility_off' : 'visibility'}</span>
        </button>
      </div>
      {hint}
    </div>
  )
}
