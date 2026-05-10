// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { Button } from '../shared/Button'
import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import { useSkillStore } from '../../stores/skillStore'
import { useDockSuspend } from '../../hooks/useDockSuspend'

const SKILL_TEMPLATE = (name: string) => `---
name: ${name || 'my-skill'}
description: Briefly describe when the assistant should use this skill.
version: 0.1.0
alwaysApply: false
---

# ${name || 'My Skill'}

## When to use
Describe the scenarios that trigger this skill.

## How it works
Step-by-step instructions the assistant should follow.

## Example
Provide one or more concrete examples here.
`

type Props = {
  open: boolean
  onClose: () => void
  onCreated: () => void
}

export function CreateSkillDialog({ open, onClose, onCreated }: Props) {
  const t = useTranslation()
  const upsert = useSkillStore((s) => s.upsertUserSkill)
  const addToast = useUIStore((s) => s.addToast)
  useDockSuspend(open)

  const [name, setName] = useState('')
  const [content, setContent] = useState(SKILL_TEMPLATE(''))
  const [isSaving, setIsSaving] = useState(false)
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [touchedContent, setTouchedContent] = useState(false)

  useEffect(() => {
    if (open) {
      setName('')
      setContent(SKILL_TEMPLATE(''))
      setTouchedContent(false)
      setErrorMsg(null)
    }
  }, [open])

  if (!open) return null

  const trimmedName = name.trim()
  const invalid =
    trimmedName.length === 0 ||
    !/^[A-Za-z0-9][A-Za-z0-9\-_]*$/.test(trimmedName) ||
    trimmedName.length > 100

  function handleNameChange(value: string) {
    setName(value)
    setErrorMsg(null)
    if (!touchedContent) {
      setContent(SKILL_TEMPLATE(value.trim()))
    }
  }

  async function handleCreate() {
    if (invalid) {
      setErrorMsg(t('settings.skills.nameInvalid'))
      return
    }
    setIsSaving(true)
    setErrorMsg(null)
    try {
      await upsert(trimmedName, content)
      addToast({ type: 'success', message: t('settings.skills.createdToast') })
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
            {t('settings.skills.newDialogTitle')}
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
              {t('settings.skills.fieldName')}
            </label>
            <input
              type="text"
              value={name}
              onChange={(e) => handleNameChange(e.target.value)}
              placeholder={t('settings.skills.namePlaceholder')}
              className="mt-1 w-full px-3 py-1.5 text-sm rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text-primary)] focus:outline-none focus:border-[var(--color-brand)] font-mono"
            />
            <p className="text-[10px] text-[var(--color-text-tertiary)] mt-1">
              {t('settings.skills.nameHint')}
            </p>
          </div>
          <div>
            <label className="text-[11px] font-medium text-[var(--color-text-secondary)]">
              {t('settings.skills.fieldContent')}
            </label>
            <textarea
              value={content}
              onChange={(e) => {
                setContent(e.target.value)
                setTouchedContent(true)
              }}
              spellCheck={false}
              className="mt-1 w-full min-h-[300px] max-h-[460px] font-mono text-xs leading-5 px-3 py-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-text-primary)] resize-y focus:outline-none focus:border-[var(--color-brand)]"
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
          <Button onClick={() => void handleCreate()} disabled={isSaving || invalid}>
            <span className="material-symbols-outlined text-[14px] mr-1">add</span>
            {isSaving ? t('common.saving') : t('settings.skills.createButton')}
          </Button>
        </div>
      </div>
    </div>
  )
}
