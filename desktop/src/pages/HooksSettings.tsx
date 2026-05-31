// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../i18n'
import { Button } from '../components/shared/Button'
import { Input } from '../components/shared/Input'
import { useHooksStore } from '../stores/hooksStore'
import { useUIStore } from '../stores/uiStore'

export function HooksSettings() {
  const t = useTranslation()
  const config = useHooksStore((s) => s.config)
  const isLoading = useHooksStore((s) => s.isLoading)
  const isSaving = useHooksStore((s) => s.isSaving)
  const error = useHooksStore((s) => s.error)
  const fetch = useHooksStore((s) => s.fetch)
  const update = useHooksStore((s) => s.update)
  const addToast = useUIStore((s) => s.addToast)

  const [enabled, setEnabled] = useState(true)
  const [commandLogger, setCommandLogger] = useState(false)
  const [auditEnabled, setAuditEnabled] = useState(false)
  const [auditUrl, setAuditUrl] = useState('')
  const [auditPatterns, setAuditPatterns] = useState('')
  const [includeArgs, setIncludeArgs] = useState(false)
  const [maxArgsBytes, setMaxArgsBytes] = useState<number>(4096)

  useEffect(() => {
    void fetch()
  }, [fetch])

  useEffect(() => {
    if (!config) return
    setEnabled(config.enabled)
    setCommandLogger(config.builtin?.commandLogger ?? false)
    setAuditEnabled(config.builtin?.webhookAudit?.enabled ?? false)
    setAuditUrl(config.builtin?.webhookAudit?.url ?? '')
    setAuditPatterns((config.builtin?.webhookAudit?.toolPatterns ?? []).join(', '))
    setIncludeArgs(config.builtin?.webhookAudit?.includeArgs ?? false)
    setMaxArgsBytes(config.builtin?.webhookAudit?.maxArgsBytes ?? 4096)
  }, [config])

  async function handleSave() {
    try {
      await update({
        enabled,
        builtin: {
          commandLogger,
          webhookAudit: {
            enabled: auditEnabled,
            url: auditUrl.trim(),
            toolPatterns: auditPatterns
              .split(',')
              .map((s) => s.trim())
              .filter(Boolean),
            includeArgs,
            maxArgsBytes: Number.isFinite(maxArgsBytes) ? maxArgsBytes : 4096,
          },
        },
      })
      addToast({ type: 'success', message: t('settings.hooks.savedToast') })
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    }
  }

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('settings.hooks.title')}
        </h2>
        <p className="text-xs text-[var(--color-text-secondary)] mt-1">
          {t('settings.hooks.description')}
        </p>
      </div>

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      <Section title={t('settings.hooks.runtimeSection')}>
        <CheckboxRow
          checked={enabled}
          onChange={setEnabled}
          label={t('settings.hooks.enableLifecycle')}
          hint={t('settings.hooks.enableLifecycleHint')}
        />
        <CheckboxRow
          checked={commandLogger}
          onChange={setCommandLogger}
          label={t('settings.hooks.commandLogger')}
          hint={t('settings.hooks.commandLoggerHint')}
          disabled={!enabled}
        />
      </Section>

      <Section title={t('settings.hooks.webhookSection')}>
        <CheckboxRow
          checked={auditEnabled}
          onChange={setAuditEnabled}
          label={t('settings.hooks.webhookEnable')}
          hint={t('settings.hooks.webhookEnableHint')}
          disabled={!enabled}
        />
        <Field label={t('settings.hooks.webhookUrl')}>
          <Input
            value={auditUrl}
            onChange={(e) => setAuditUrl(e.target.value)}
            placeholder="https://example.com/audit"
            disabled={!enabled || !auditEnabled}
          />
        </Field>
        <Field
          label={t('settings.hooks.webhookPatterns')}
          hint={t('settings.hooks.webhookPatternsHint')}
        >
          <Input
            value={auditPatterns}
            onChange={(e) => setAuditPatterns(e.target.value)}
            placeholder="shell, file_write"
            disabled={!enabled || !auditEnabled}
          />
        </Field>
        <CheckboxRow
          checked={includeArgs}
          onChange={setIncludeArgs}
          label={t('settings.hooks.webhookIncludeArgs')}
          hint={t('settings.hooks.webhookIncludeArgsHint')}
          disabled={!enabled || !auditEnabled}
        />
        <Field label={t('settings.hooks.webhookMaxArgsBytes')}>
          <Input
            type="number"
            min={0}
            value={maxArgsBytes}
            onChange={(e) =>
              setMaxArgsBytes(Number.parseInt(e.target.value || '0', 10))
            }
            disabled={!enabled || !auditEnabled}
          />
        </Field>
      </Section>

      <Section title={t('settings.hooks.scriptSection')}>
        <p className="text-xs text-[var(--color-text-secondary)] mb-2">
          {t('settings.hooks.scriptDescription')}
        </p>
        <ul className="space-y-1 text-xs text-[var(--color-text-secondary)]">
          {(config?.scriptHookPaths ?? []).map((path) => (
            <li
              key={path}
              className="rounded-md bg-[var(--color-surface-container)] px-2 py-1 font-mono"
            >
              {path}
            </li>
          ))}
        </ul>
      </Section>

      <div className="flex items-center gap-2">
        <Button size="sm" onClick={handleSave} disabled={isLoading || isSaving}>
          {isSaving ? t('common.saving') : t('common.save')}
        </Button>
        <Button variant="ghost" size="sm" onClick={() => void fetch()} disabled={isLoading}>
          {t('common.reload')}
        </Button>
      </div>
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-3">
      <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
        {title}
      </h3>
      {children}
    </section>
  )
}

function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <label className="block space-y-1">
      <span className="text-xs font-medium text-[var(--color-text-secondary)]">
        {label}
      </span>
      {children}
      {hint && (
        <span className="block text-xs text-[var(--color-text-tertiary)]">
          {hint}
        </span>
      )}
    </label>
  )
}

function CheckboxRow({
  checked,
  onChange,
  label,
  hint,
  disabled,
}: {
  checked: boolean
  onChange: (next: boolean) => void
  label: string
  hint?: string
  disabled?: boolean
}) {
  return (
    <label className={`flex items-start gap-2 text-xs ${disabled ? 'opacity-60' : ''}`}>
      <input
        type="checkbox"
        className="mt-[2px]"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        disabled={disabled}
      />
      <span>
        <span className="text-[var(--color-text-primary)] font-medium">{label}</span>
        {hint && (
          <span className="block text-xs text-[var(--color-text-tertiary)]">
            {hint}
          </span>
        )}
      </span>
    </label>
  )
}
