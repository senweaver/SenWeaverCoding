// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import { Button } from '../shared/Button'
import { Input } from '../shared/Input'
import type { GuardrailRule, RulePolicy } from '../../types/rules'

type Props = {
  initial: GuardrailRule | null
  isSaving?: boolean
  onSubmit: (rule: GuardrailRule) => Promise<void>
  onCancel: () => void
}

function emptyRule(): GuardrailRule {
  return {
    toolPattern: '',
    policy: 'deny',
    reason: null,
    contexts: [],
  }
}

export function RuleEditor({ initial, isSaving, onSubmit, onCancel }: Props) {
  const t = useTranslation()
  const isEdit = Boolean(initial)
  const [draft, setDraft] = useState<GuardrailRule>(initial ?? emptyRule())
  const [contextsText, setContextsText] = useState('')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const next = initial ?? emptyRule()
    setDraft(next)
    setContextsText(next.contexts.join(', '))
    setError(null)
  }, [initial])

  async function handleSubmit() {
    setError(null)
    const pattern = draft.toolPattern.trim()
    if (!pattern) {
      setError(t('settings.rules.errorPatternEmpty'))
      return
    }
    try {
      await onSubmit({
        toolPattern: pattern,
        policy: draft.policy,
        reason: draft.reason?.trim() || null,
        contexts: contextsText
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  return (
    <div className="space-y-4">
      <header className="flex items-center justify-between">
        <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
          {isEdit ? t('settings.rules.editTitle') : t('settings.rules.createTitle')}
        </h3>
      </header>

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      <Field label={t('settings.rules.fieldPattern')} required hint={t('settings.rules.fieldPatternHint')}>
        <Input
          value={draft.toolPattern}
          onChange={(e) => setDraft({ ...draft, toolPattern: e.target.value })}
          placeholder="shell or file_*"
        />
      </Field>

      <Field label={t('settings.rules.fieldPolicy')} required>
        <select
          className="h-8 w-full rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 text-xs"
          value={draft.policy}
          onChange={(e) => setDraft({ ...draft, policy: e.target.value as RulePolicy })}
        >
          <option value="allow">{t('settings.rules.policy.allow')}</option>
          <option value="deny">{t('settings.rules.policy.deny')}</option>
          <option value="require_approval">
            {t('settings.rules.policy.requireApproval')}
          </option>
          <option value="audit_only">{t('settings.rules.policy.auditOnly')}</option>
        </select>
      </Field>

      <Field label={t('settings.rules.fieldReason')} hint={t('settings.rules.fieldReasonHint')}>
        <Input
          value={draft.reason ?? ''}
          onChange={(e) => setDraft({ ...draft, reason: e.target.value || null })}
        />
      </Field>

      <Field label={t('settings.rules.fieldContexts')} hint={t('settings.rules.fieldContextsHint')}>
        <Input
          value={contextsText}
          onChange={(e) => setContextsText(e.target.value)}
          placeholder="ci, prod"
        />
      </Field>

      <div className="flex items-center justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onCancel}>
          {t('common.cancel')}
        </Button>
        <Button size="sm" onClick={handleSubmit} disabled={isSaving}>
          {isSaving ? t('common.saving') : t('common.save')}
        </Button>
      </div>
    </div>
  )
}

function Field({
  label,
  hint,
  required,
  children,
}: {
  label: string
  hint?: string
  required?: boolean
  children: React.ReactNode
}) {
  return (
    <label className="block space-y-1">
      <span className="text-xs font-medium text-[var(--color-text-secondary)]">
        {label}
        {required && <span className="text-[var(--color-error)] ml-0.5">*</span>}
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
