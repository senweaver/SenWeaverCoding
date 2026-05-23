import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import { Button } from '../shared/Button'
import { ConfirmDialog } from '../shared/ConfirmDialog'
import { useRulesStore } from '../../stores/rulesStore'
import { useUIStore } from '../../stores/uiStore'
import { RuleEditor } from './RuleEditor'
import type { GuardrailRule, RulePolicy } from '../../types/rules'

const POLICY_BADGES: Record<RulePolicy, string> = {
  allow: 'bg-[var(--color-success-container)] text-[var(--color-success)]',
  deny: 'bg-[var(--color-error-container)] text-[var(--color-error)]',
  require_approval:
    'bg-[var(--color-warning-container)] text-[var(--color-warning)]',
  audit_only: 'bg-[var(--color-info-container)] text-[var(--color-info)]',
}

type Mode =
  | { kind: 'list' }
  | { kind: 'create' }
  | { kind: 'edit'; index: number; rule: GuardrailRule }

export function RuleList() {
  const t = useTranslation()
  const config = useRulesStore((s) => s.config)
  const isLoading = useRulesStore((s) => s.isLoading)
  const isSaving = useRulesStore((s) => s.isSaving)
  const error = useRulesStore((s) => s.error)
  const fetch = useRulesStore((s) => s.fetch)
  const update = useRulesStore((s) => s.update)
  const addToast = useUIStore((s) => s.addToast)
  const [mode, setMode] = useState<Mode>({ kind: 'list' })
  const [pendingDelete, setPendingDelete] = useState<{
    index: number
    rule: GuardrailRule
  } | null>(null)

  useEffect(() => {
    void fetch()
  }, [fetch])

  async function setEnabled(next: boolean) {
    try {
      await update({ enabled: next })
      addToast({ type: 'success', message: t('settings.rules.savedToast') })
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    }
  }

  async function setDefaultPolicy(policy: RulePolicy) {
    try {
      await update({ defaultPolicy: policy })
      addToast({ type: 'success', message: t('settings.rules.savedToast') })
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    }
  }

  async function saveRule(rule: GuardrailRule) {
    if (!config) return
    const nextRules = [...config.rules]
    if (mode.kind === 'edit') {
      nextRules[mode.index] = rule
    } else if (mode.kind === 'create') {
      nextRules.push(rule)
    }
    try {
      await update({ rules: nextRules })
      addToast({ type: 'success', message: t('settings.rules.savedToast') })
      setMode({ kind: 'list' })
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    }
  }

  async function confirmDelete() {
    if (!config || !pendingDelete) return
    const nextRules = config.rules.filter((_, i) => i !== pendingDelete.index)
    try {
      await update({ rules: nextRules })
      addToast({ type: 'success', message: t('settings.rules.deletedToast') })
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    } finally {
      setPendingDelete(null)
    }
  }

  if (mode.kind !== 'list') {
    return (
      <RuleEditor
        initial={mode.kind === 'edit' ? mode.rule : null}
        isSaving={isSaving}
        onSubmit={saveRule}
        onCancel={() => setMode({ kind: 'list' })}
      />
    )
  }

  return (
    <div className="space-y-3">
      {error && (
        <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      <section className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3 space-y-3">
        <div className="flex items-center justify-between">
          <div>
            <p className="text-xs font-semibold text-[var(--color-text-primary)]">
              {t('settings.rules.engineEnable')}
            </p>
            <p className="text-xs text-[var(--color-text-tertiary)]">
              {t('settings.rules.engineEnableHint')}
            </p>
          </div>
          <label className="flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={config?.enabled ?? false}
              onChange={(e) => void setEnabled(e.target.checked)}
            />
          </label>
        </div>
        <div>
          <p className="text-xs text-[var(--color-text-secondary)] mb-1">
            {t('settings.rules.defaultPolicy')}
          </p>
          <select
            className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] h-8 px-2.5 text-xs"
            value={config?.defaultPolicy ?? 'allow'}
            onChange={(e) => void setDefaultPolicy(e.target.value as RulePolicy)}
          >
            <option value="allow">{t('settings.rules.policy.allow')}</option>
            <option value="deny">{t('settings.rules.policy.deny')}</option>
            <option value="require_approval">
              {t('settings.rules.policy.requireApproval')}
            </option>
            <option value="audit_only">
              {t('settings.rules.policy.auditOnly')}
            </option>
          </select>
        </div>
      </section>

      <div className="flex items-center justify-between">
        <p className="text-xs text-[var(--color-text-secondary)]">
          {t('settings.rules.listDescription')}
        </p>
        <Button size="sm" onClick={() => setMode({ kind: 'create' })}>
          <span className="material-symbols-outlined text-[14px] mr-1">add</span>
          {t('settings.rules.create')}
        </Button>
      </div>

      {(config?.rules.length ?? 0) === 0 && !isLoading ? (
        <div className="rounded-lg border border-dashed border-[var(--color-border)] p-4 text-center text-xs text-[var(--color-text-secondary)]">
          {t('settings.rules.empty')}
        </div>
      ) : (
        <ul className="space-y-2">
          {config?.rules.map((rule, idx) => (
            <li
              key={`${rule.toolPattern}-${idx}`}
              className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] p-3"
            >
              <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 space-y-1">
                  <p className="text-xs font-semibold text-[var(--color-text-primary)] flex items-center gap-2">
                    <code className="font-mono text-xs">{rule.toolPattern}</code>
                    <span
                      className={`text-[10px] uppercase tracking-wide rounded px-1.5 py-[1px] ${
                        POLICY_BADGES[rule.policy]
                      }`}
                    >
                      {rule.policy.replace('_', ' ')}
                    </span>
                  </p>
                  {rule.reason && (
                    <p className="text-xs text-[var(--color-text-secondary)]">
                      {rule.reason}
                    </p>
                  )}
                  {rule.contexts.length > 0 && (
                    <p className="text-xs text-[var(--color-text-tertiary)] font-mono">
                      {rule.contexts.join(', ')}
                    </p>
                  )}
                </div>
                <div className="flex items-center gap-1 flex-shrink-0">
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setMode({ kind: 'edit', index: idx, rule })}
                  >
                    <span className="material-symbols-outlined text-[14px]">edit</span>
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => setPendingDelete({ index: idx, rule })}
                  >
                    <span className="material-symbols-outlined text-[14px]">delete</span>
                  </Button>
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={Boolean(pendingDelete)}
        title={t('settings.rules.confirmDeleteTitle')}
        body={
          pendingDelete
            ? t('settings.rules.confirmDeleteMessage').replace(
                '{pattern}',
                pendingDelete.rule.toolPattern,
              )
            : ''
        }
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        confirmVariant="danger"
        onConfirm={confirmDelete}
        onClose={() => setPendingDelete(null)}
      />
    </div>
  )
}
