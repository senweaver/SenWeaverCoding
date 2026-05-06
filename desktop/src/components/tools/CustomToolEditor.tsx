import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { Button } from '../shared/Button'
import { Input } from '../shared/Input'
import type { CustomToolDef } from '../../types/customTools'

type Props = {
  initial: CustomToolDef | null
  isSaving?: boolean
  onSubmit: (def: CustomToolDef) => Promise<void>
  onCancel: () => void
}

const DEFAULT_SCHEMA = {
  type: 'object',
  properties: {},
  additionalProperties: true,
}

function emptyTool(): CustomToolDef {
  return {
    name: '',
    description: '',
    command: '',
    args: [],
    cwd: null,
    env: {},
    timeoutSecs: 60,
    schema: DEFAULT_SCHEMA,
    enabled: true,
  }
}

export function CustomToolEditor({ initial, isSaving, onSubmit, onCancel }: Props) {
  const t = useTranslation()
  const isEdit = Boolean(initial)
  const [draft, setDraft] = useState<CustomToolDef>(initial ?? emptyTool())
  const [argsText, setArgsText] = useState('')
  const [envText, setEnvText] = useState('')
  const [schemaText, setSchemaText] = useState('')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    const next = initial ?? emptyTool()
    setDraft(next)
    setArgsText(next.args.join('\n'))
    setEnvText(
      Object.entries(next.env)
        .map(([k, v]) => `${k}=${v}`)
        .join('\n'),
    )
    setSchemaText(JSON.stringify(next.schema ?? DEFAULT_SCHEMA, null, 2))
    setError(null)
  }, [initial])

  const trimmedName = draft.name.trim()
  const isValidName = useMemo(() => /^[a-z_][a-z0-9_]*$/.test(trimmedName), [trimmedName])

  function parseArgs(text: string): string[] {
    return text
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter((s) => s.length > 0)
  }

  function parseEnv(text: string): Record<string, string> {
    const env: Record<string, string> = {}
    for (const line of text.split(/\r?\n/)) {
      const trimmed = line.trim()
      if (!trimmed || trimmed.startsWith('#')) continue
      const eqIdx = trimmed.indexOf('=')
      if (eqIdx <= 0) continue
      const key = trimmed.slice(0, eqIdx).trim()
      const value = trimmed.slice(eqIdx + 1)
      if (key) env[key] = value
    }
    return env
  }

  async function handleSubmit() {
    setError(null)

    if (!isValidName) {
      setError(t('settings.tools.errorNamePattern'))
      return
    }
    if (draft.command.trim().length === 0) {
      setError(t('settings.tools.errorCommandEmpty'))
      return
    }
    if (draft.timeoutSecs <= 0) {
      setError(t('settings.tools.errorTimeout'))
      return
    }
    let schema: unknown = DEFAULT_SCHEMA
    try {
      schema = schemaText.trim() ? JSON.parse(schemaText) : DEFAULT_SCHEMA
    } catch {
      setError(t('settings.tools.errorSchema'))
      return
    }

    try {
      await onSubmit({
        ...draft,
        name: trimmedName,
        args: parseArgs(argsText),
        env: parseEnv(envText),
        schema: schema as CustomToolDef['schema'],
      })
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    }
  }

  return (
    <div className="space-y-4">
      <header className="flex items-center justify-between">
        <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
          {isEdit ? t('settings.tools.editTitle') : t('settings.tools.createTitle')}
        </h3>
      </header>

      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <Field label={t('settings.tools.fieldName')} required>
          <Input
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            placeholder="deploy"
            disabled={isEdit}
          />
        </Field>
        <Field label={t('settings.tools.fieldTimeout')}>
          <Input
            type="number"
            min={1}
            value={draft.timeoutSecs}
            onChange={(e) =>
              setDraft({
                ...draft,
                timeoutSecs: Number.parseInt(e.target.value || '60', 10),
              })
            }
          />
        </Field>
      </div>

      <Field label={t('settings.tools.fieldDescription')}>
        <textarea
          value={draft.description}
          onChange={(e) => setDraft({ ...draft, description: e.target.value })}
          rows={2}
          className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm"
        />
      </Field>

      <Field label={t('settings.tools.fieldCommand')} required hint={t('settings.tools.fieldCommandHint')}>
        <Input
          value={draft.command}
          onChange={(e) => setDraft({ ...draft, command: e.target.value })}
          placeholder="kubectl"
        />
      </Field>

      <Field label={t('settings.tools.fieldArgs')} hint={t('settings.tools.fieldArgsHint')}>
        <textarea
          value={argsText}
          onChange={(e) => setArgsText(e.target.value)}
          rows={3}
          className="w-full font-mono rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-xs"
          placeholder={'apply\n-f\n{file}'}
        />
      </Field>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <Field label={t('settings.tools.fieldCwd')} hint={t('settings.tools.fieldCwdHint')}>
          <Input
            value={draft.cwd ?? ''}
            onChange={(e) =>
              setDraft({ ...draft, cwd: e.target.value || null })
            }
            placeholder=""
          />
        </Field>
        <Field label={t('settings.tools.fieldEnabled')}>
          <label className="flex items-center gap-2 h-10 px-3">
            <input
              type="checkbox"
              checked={draft.enabled}
              onChange={(e) => setDraft({ ...draft, enabled: e.target.checked })}
            />
            <span className="text-xs text-[var(--color-text-secondary)]">
              {draft.enabled ? t('common.enable') : t('common.disable')}
            </span>
          </label>
        </Field>
      </div>

      <Field label={t('settings.tools.fieldEnv')} hint={t('settings.tools.fieldEnvHint')}>
        <textarea
          value={envText}
          onChange={(e) => setEnvText(e.target.value)}
          rows={3}
          className="w-full font-mono rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-xs"
          placeholder={'KEY=value'}
        />
      </Field>

      <Field label={t('settings.tools.fieldSchema')} hint={t('settings.tools.fieldSchemaHint')}>
        <textarea
          value={schemaText}
          onChange={(e) => setSchemaText(e.target.value)}
          rows={8}
          className="w-full font-mono rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-xs"
        />
      </Field>

      <div className="flex items-center justify-end gap-2">
        <Button variant="ghost" onClick={onCancel}>
          {t('common.cancel')}
        </Button>
        <Button onClick={handleSubmit} disabled={isSaving}>
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
        <span className="block text-[11px] text-[var(--color-text-tertiary)]">
          {hint}
        </span>
      )}
    </label>
  )
}
