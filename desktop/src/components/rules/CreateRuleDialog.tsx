// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { Button } from '../shared/Button'
import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import { useUserRulesStore } from '../../stores/userRulesStore'

const RULE_TEMPLATE = `---
alwaysApply: true
description: ""
---

# Rule Title

Write your instruction rule here. The body will be injected into the
assistant's system prompt as a binding constraint when alwaysApply is true.

To make this rule on-demand instead, set \`alwaysApply: false\` and provide
a clear \`description\`. The assistant will load it via read_user_rule(name)
when relevant.
`

type Props = {
  open: boolean
  onClose: () => void
  onCreated: () => void
}

export function CreateRuleDialog({ open, onClose, onCreated }: Props) {
  const t = useTranslation()
  const upsert = useUserRulesStore((s) => s.upsert)
  const files = useUserRulesStore((s) => s.files)
  const addToast = useUIStore((s) => s.addToast)

  const [name, setName] = useState('')
  const [content, setContent] = useState(RULE_TEMPLATE)
  const [isSaving, setIsSaving] = useState(false)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)

  useEffect(() => {
    if (open) {
      setName('')
      setContent(RULE_TEMPLATE)
      setErrorMsg(null)
    }
  }, [open])

  if (!open) return null

  const trimmedName = name.trim()
  const finalName = trimmedName.toLowerCase().endsWith('.md') || trimmedName.toLowerCase().endsWith('.mdc')
    ? trimmedName
    : trimmedName.length > 0
      ? `${trimmedName}.md`
      : ''
  const conflict = finalName.length > 0 && files.some((f) => f.name === finalName)
  const invalid =
    trimmedName.length === 0 ||
    /[\\/\0]/.test(trimmedName) ||
    trimmedName.startsWith('.') ||
    trimmedName.includes('..')

  async function handleCreate() {
    if (invalid) {
      setErrorMsg(t('settings.userRules.nameInvalid'))
      return
    }
    if (conflict) {
      setErrorMsg(t('settings.userRules.nameConflict'))
      return
    }
    setIsSaving(true)
    setErrorMsg(null)
    try {
      await upsert(finalName, content)
      addToast({ type: 'success', message: t('settings.userRules.createdToast') })
      onCreated()
    } catch (err) {
      setErrorMsg(err instanceof Error ? err.message : String(err))
    } finally {
      setIsSaving(false)
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 px-4">
      <div className="w-full max-w-2xl rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl">
        <div className="flex items-center justify-between px-5 py-3 border-b border-[var(--color-border)]">
          <p className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t('settings.userRules.newDialogTitle')}
          </p>
          <button
            onClick={onClose}
            className="text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>
        <div className="p-5 space-y-3">
          <div>
            <label className="text-[11px] font-medium text-[var(--color-text-secondary)]">
              {t('settings.userRules.fieldName')}
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => {
                setName(e.target.value)
                setErrorMsg(null)
              }}
              placeholder={t('settings.userRules.namePlaceholder')}
              className="mt-1 w-full px-3 py-1.5 text-sm rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text-primary)] focus:outline-none focus:border-[var(--color-brand)]"
            />
            <p className="text-[10px] text-[var(--color-text-tertiary)] mt-1">
              {t('settings.userRules.nameHint', {
                computed: finalName || '—',
              })}
            </p>
          </div>
          <div>
            <label className="text-[11px] font-medium text-[var(--color-text-secondary)]">
              {t('settings.userRules.fieldContent')}
            </label>
            <textarea
              value={content}
              onChange={(e) => setContent(e.target.value)}
              spellCheck={false}
              className="mt-1 w-full min-h-[280px] max-h-[420px] font-mono text-xs leading-5 px-3 py-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text-primary)] resize-y focus:outline-none focus:border-[var(--color-brand)]"
            />
          </div>
          {errorMsg && (
            <div className="rounded-md border border-[var(--color-error-container)] bg-[var(--color-error-container)] px-3 py-2 text-xs text-[var(--color-error)]">
              {errorMsg}
            </div>
          )}
        </div>
        <div className="flex items-center justify-end gap-2 px-5 py-3 border-t border-[var(--color-border)]">
          <button
            onClick={onClose}
            disabled={isSaving}
            className="px-3 py-1.5 text-xs rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)] disabled:opacity-50"
          >
            {t('common.cancel')}
          </button>
          <Button
            onClick={() => void handleCreate()}
            disabled={isSaving || invalid || conflict}
          >
            <span className="material-symbols-outlined text-[14px] mr-1">add</span>
            {isSaving ? t('common.saving') : t('settings.userRules.createButton')}
          </Button>
        </div>
      </div>
    </div>
  )
}
