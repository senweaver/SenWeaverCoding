// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { Button } from '../components/shared/Button'
import { useTranslation } from '../i18n'
import type { TranslationKey } from '../i18n'
import { useKeyboardShortcutsStore } from '../stores/keyboardShortcutsStore'
import {
  bindingFromKeyboardEvent,
  bindingsEqual,
  formatBinding,
  SHORTCUT_ACTIONS,
  DEFAULT_SHORTCUT_BINDINGS,
  type ShortcutActionId,
  type ShortcutBinding,
} from '../types/shortcuts'

const ACTION_ICON: Record<ShortcutActionId, string> = {
  'new-session': 'add_circle',
  'sidebar-search': 'search',
  'stop-generation': 'stop_circle',
  'toggle-terminal': 'terminal',
  'quick-mode-switcher': 'tune',
  'mode-plan': 'architecture',
  'close-modal': 'close',
}

function findConflict(
  bindings: Record<ShortcutActionId, ShortcutBinding>,
  candidate: ShortcutBinding,
  excluding: ShortcutActionId,
): ShortcutActionId | null {
  for (const id of SHORTCUT_ACTIONS) {
    if (id === excluding) continue
    if (bindingsEqual(bindings[id], candidate)) return id
  }
  return null
}

export function KeyboardShortcutsSettings() {
  const t = useTranslation()
  const { bindings, setBinding, resetBinding, resetAll } = useKeyboardShortcutsStore(
    useShallow((s) => ({
      bindings: s.bindings,
      setBinding: s.setBinding,
      resetBinding: s.resetBinding,
      resetAll: s.resetAll,
    })),
  )
  const [recordingId, setRecordingId] = useState<ShortcutActionId | null>(null)
  const [conflictMessage, setConflictMessage] = useState<string | null>(null)

  useEffect(() => {
    if (!recordingId) return
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        setRecordingId(null)
        setConflictMessage(null)
        return
      }
      const candidate = bindingFromKeyboardEvent(e)
      if (!candidate) return
      e.preventDefault()
      e.stopPropagation()
      const conflict = findConflict(bindings, candidate, recordingId)
      if (conflict) {
        const conflictLabel = t(
          `shortcuts.action.${conflict}.label` as TranslationKey,
        )
        setConflictMessage(
          t('shortcuts.conflict', {
            shortcut: formatBinding(candidate),
            action: conflictLabel,
          }),
        )
        return
      }
      setBinding(recordingId, candidate)
      setRecordingId(null)
      setConflictMessage(null)
    }
    document.addEventListener('keydown', handler, { capture: true })
    return () =>
      document.removeEventListener('keydown', handler, {
        capture: true,
      } as EventListenerOptions)
  }, [recordingId, bindings, setBinding, t])

  const rows = useMemo(
    () =>
      SHORTCUT_ACTIONS.map((id) => ({
        id,
        label: t(`shortcuts.action.${id}.label` as TranslationKey),
        description: t(`shortcuts.action.${id}.description` as TranslationKey),
        binding: bindings[id],
        isDefault: bindingsEqual(bindings[id], DEFAULT_SHORTCUT_BINDINGS[id]),
        recording: recordingId === id,
      })),
    [bindings, recordingId, t],
  )

  return (
    <div>
      <div className="flex items-start justify-between gap-4 mb-4">
        <div className="flex-1 min-w-0">
          <h2 className="text-xs font-bold text-[var(--color-text-primary)]">
            {t('settings.keyboard.title')}
          </h2>
          <p className="text-xs text-[var(--color-text-tertiary)] mt-1 leading-relaxed">
            {t('settings.keyboard.description')}
          </p>
        </div>
        <Button
          size="sm"
          className="flex-shrink-0 whitespace-nowrap"
          onClick={() => {
            resetAll()
            setRecordingId(null)
            setConflictMessage(null)
          }}
        >
          <span className="material-symbols-outlined text-[14px] mr-1">restart_alt</span>
          {t('settings.keyboard.resetAll')}
        </Button>
      </div>

      {conflictMessage && (
        <div className="mb-3 rounded-lg border border-[var(--color-error)]/30 bg-[var(--color-error)]/8 px-3 py-2 text-xs text-[var(--color-error)]">
          {conflictMessage}
        </div>
      )}

      <div className="rounded-xl border border-[var(--color-border)] overflow-hidden divide-y divide-[var(--color-border)]">
        {rows.map((row) => (
          <div
            key={row.id}
            className="flex items-center gap-3 px-3 py-2.5 bg-[var(--color-surface-container-lowest)]"
          >
            <span className="material-symbols-outlined text-[16px] text-[var(--color-text-secondary)] shrink-0">
              {ACTION_ICON[row.id]}
            </span>
            <div className="flex-1 min-w-0">
              <div className="text-xs font-semibold text-[var(--color-text-primary)] truncate">
                {row.label}
              </div>
              <div className="text-xs text-[var(--color-text-tertiary)] truncate">
                {row.description}
              </div>
            </div>
            <div className="flex items-center gap-2 shrink-0">
              {row.recording ? (
                <span className="inline-flex items-center gap-1 px-2.5 py-1 rounded-md border border-[var(--color-brand)]/40 bg-[var(--color-brand)]/8 text-xs font-bold text-[var(--color-brand)] tabular-nums animate-pulse">
                  <span className="material-symbols-outlined text-[14px]">
                    fiber_manual_record
                  </span>
                  {t('settings.keyboard.recording')}
                </span>
              ) : (
                <span className="inline-flex items-center px-2.5 py-1 rounded-md border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] text-xs font-bold text-[var(--color-text-secondary)] tabular-nums">
                  {formatBinding(row.binding)}
                </span>
              )}
              <button
                type="button"
                onClick={() => {
                  setConflictMessage(null)
                  setRecordingId(row.recording ? null : row.id)
                }}
                className="inline-flex items-center gap-1.5 whitespace-nowrap rounded-[var(--radius-md)] h-7 px-2.5 text-xs font-medium text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] transition-colors"
              >
                {row.recording
                  ? t('settings.keyboard.cancel')
                  : t('settings.keyboard.rebind')}
              </button>
              {!row.isDefault && !row.recording && (
                <button
                  type="button"
                  onClick={() => resetBinding(row.id)}
                  className="inline-flex items-center justify-center rounded-[var(--radius-md)] h-7 w-7 text-xs font-medium text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] transition-colors"
                  title={t('settings.keyboard.resetOne')}
                >
                  <span className="material-symbols-outlined text-[14px]">undo</span>
                </button>
              )}
            </div>
          </div>
        ))}
      </div>

      <div className="mt-3 text-xs text-[var(--color-text-tertiary)] leading-relaxed">
        {t('settings.keyboard.recordHint')}
      </div>
    </div>
  )
}
