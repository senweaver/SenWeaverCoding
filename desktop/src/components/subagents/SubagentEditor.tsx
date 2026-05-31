// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import { Button } from '../shared/Button'
import { Input } from '../shared/Input'
import type { DelegateAgentDef } from '../../types/subagents'

type Props = {
  initial: DelegateAgentDef | null
  isSaving?: boolean
  onSubmit: (def: DelegateAgentDef) => Promise<void>
  onCancel: () => void
}

function emptyAgent(): DelegateAgentDef {
  return {
    name: '',
    provider: '',
    model: '',
    systemPrompt: '',
    apiKey: null,
    temperature: null,
    maxDepth: 3,
    agentic: false,
    allowedTools: [],
    maxIterations: 10,
    timeoutSecs: null,
    agenticTimeoutSecs: null,
    skillsDirectory: null,
  }
}

export function SubagentEditor({ initial, isSaving, onSubmit, onCancel }: Props) {
  const t = useTranslation()
  const isEdit = Boolean(initial)
  const [draft, setDraft] = useState<DelegateAgentDef>(initial ?? emptyAgent())
  const [allowedToolsText, setAllowedToolsText] = useState('')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const next = initial ?? emptyAgent()
    setDraft(next)
    setAllowedToolsText(next.allowedTools.join(', '))
    setError(null)
  }, [initial])

  async function handleSubmit() {
    setError(null)
    const name = draft.name.trim()
    if (!name) {
      setError(t('settings.subagents.errorNameEmpty'))
      return
    }
    if (!draft.provider.trim()) {
      setError(t('settings.subagents.errorProviderEmpty'))
      return
    }
    if (!draft.model.trim()) {
      setError(t('settings.subagents.errorModelEmpty'))
      return
    }
    if (draft.maxDepth <= 0 || draft.maxDepth > 10) {
      setError(t('settings.subagents.errorMaxDepth'))
      return
    }
    if (draft.maxIterations <= 0) {
      setError(t('settings.subagents.errorMaxIterations'))
      return
    }
    if (
      draft.temperature !== null &&
      (draft.temperature < 0 || draft.temperature > 2)
    ) {
      setError(t('settings.subagents.errorTemperature'))
      return
    }

    try {
      await onSubmit({
        ...draft,
        name,
        allowedTools: allowedToolsText
          .split(',')
          .map((s) => s.trim())
          .filter(Boolean),
        systemPrompt: draft.systemPrompt?.trim() || null,
        skillsDirectory: draft.skillsDirectory?.trim() || null,
        apiKey: draft.apiKey?.trim() || null,
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  return (
    <div className="space-y-4">
      <header>
        <h3 className="text-xs font-semibold text-[var(--color-text-primary)]">
          {isEdit
            ? t('settings.subagents.editTitle')
            : t('settings.subagents.createTitle')}
        </h3>
      </header>

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <Field label={t('settings.subagents.fieldName')} required>
          <Input
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            placeholder="researcher"
            disabled={isEdit}
          />
        </Field>
        <Field label={t('settings.subagents.fieldProvider')} required>
          <Input
            value={draft.provider}
            onChange={(e) => setDraft({ ...draft, provider: e.target.value })}
            placeholder="openrouter"
          />
        </Field>
        <Field label={t('settings.subagents.fieldModel')} required>
          <Input
            value={draft.model}
            onChange={(e) => setDraft({ ...draft, model: e.target.value })}
            placeholder="anthropic/claude-sonnet-4"
          />
        </Field>
        <Field label={t('settings.subagents.fieldTemperature')} hint={t('settings.subagents.fieldTemperatureHint')}>
          <Input
            type="number"
            step={0.1}
            min={0}
            max={2}
            value={draft.temperature ?? ''}
            onChange={(e) => {
              const v = e.target.value.trim()
              setDraft({
                ...draft,
                temperature: v === '' ? null : Number.parseFloat(v),
              })
            }}
          />
        </Field>
      </div>

      <Field label={t('settings.subagents.fieldSystemPrompt')}>
        <textarea
          value={draft.systemPrompt ?? ''}
          onChange={(e) => setDraft({ ...draft, systemPrompt: e.target.value })}
          rows={4}
          className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2.5 py-1.5 text-xs"
        />
      </Field>

      <Field
        label={t('settings.subagents.fieldAllowedTools')}
        hint={t('settings.subagents.fieldAllowedToolsHint')}
      >
        <Input
          value={allowedToolsText}
          onChange={(e) => setAllowedToolsText(e.target.value)}
          placeholder="shell, web_search"
        />
      </Field>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <Field label={t('settings.subagents.fieldMaxDepth')} hint={t('settings.subagents.fieldMaxDepthHint')}>
          <Input
            type="number"
            min={1}
            max={10}
            value={draft.maxDepth}
            onChange={(e) =>
              setDraft({
                ...draft,
                maxDepth: Number.parseInt(e.target.value || '3', 10),
              })
            }
          />
        </Field>
        <Field label={t('settings.subagents.fieldMaxIterations')}>
          <Input
            type="number"
            min={1}
            value={draft.maxIterations}
            onChange={(e) =>
              setDraft({
                ...draft,
                maxIterations: Number.parseInt(e.target.value || '10', 10),
              })
            }
          />
        </Field>
        <Field label={t('settings.subagents.fieldTimeout')} hint={t('settings.subagents.fieldTimeoutHint')}>
          <Input
            type="number"
            min={0}
            value={draft.timeoutSecs ?? ''}
            onChange={(e) => {
              const v = e.target.value.trim()
              setDraft({
                ...draft,
                timeoutSecs: v === '' ? null : Number.parseInt(v, 10),
              })
            }}
          />
        </Field>
        <Field
          label={t('settings.subagents.fieldAgenticTimeout')}
          hint={t('settings.subagents.fieldAgenticTimeoutHint')}
        >
          <Input
            type="number"
            min={0}
            value={draft.agenticTimeoutSecs ?? ''}
            onChange={(e) => {
              const v = e.target.value.trim()
              setDraft({
                ...draft,
                agenticTimeoutSecs: v === '' ? null : Number.parseInt(v, 10),
              })
            }}
          />
        </Field>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <Field label={t('settings.subagents.fieldAgentic')}>
          <label className="flex items-center gap-2 h-8 px-2.5">
            <input
              type="checkbox"
              checked={draft.agentic}
              onChange={(e) => setDraft({ ...draft, agentic: e.target.checked })}
            />
            <span className="text-xs text-[var(--color-text-secondary)]">
              {t('settings.subagents.fieldAgenticHint')}
            </span>
          </label>
        </Field>
        <Field label={t('settings.subagents.fieldSkillsDir')}>
          <Input
            value={draft.skillsDirectory ?? ''}
            onChange={(e) =>
              setDraft({ ...draft, skillsDirectory: e.target.value || null })
            }
            placeholder="agents/researcher/skills"
          />
        </Field>
      </div>

      <Field label={t('settings.subagents.fieldApiKey')} hint={t('settings.subagents.fieldApiKeyHint')}>
        <Input
          type="password"
          value={draft.apiKey ?? ''}
          onChange={(e) => setDraft({ ...draft, apiKey: e.target.value || null })}
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
