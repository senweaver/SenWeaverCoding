// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation } from '../../i18n'
import {
  isWorkspaceRootPath,
  splitPathForDisplay,
} from '../../utils/toolFormatters'

const EMPTY_PENDING_EDITS: ReturnType<
  typeof useChatStore.getState
>['sessions'][string]['pendingEdits'] = []

export function ReviewCard() {
  const t = useTranslation()
  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessionView = useChatStore(
    useShallow((s) => {
      const st = activeTabId ? s.sessions[activeTabId] : undefined
      return {
        pendingEdits: st?.pendingEdits,
        chatState: st?.chatState ?? 'idle',
      }
    }),
  )
  const stopGeneration = useChatStore((s) => s.stopGeneration)
  const clearPendingEdits = useChatStore((s) => s.clearPendingEdits)
  const undoAllPendingEdits = useChatStore((s) => s.undoAllPendingEdits)

  const [expanded, setExpanded] = useState(false)
  const [undoing, setUndoing] = useState(false)
  const [errorMessage, setErrorMessage] = useState<string | null>(null)

  const pendingEdits = sessionView.pendingEdits ?? EMPTY_PENDING_EDITS
  const chatState = sessionView.chatState
  const isActive = chatState !== 'idle'

  if (!activeTabId) return null

  if (!isActive && pendingEdits.length === 0) return null

  const fileCount = pendingEdits.length
  const onToggle = () => setExpanded((v) => !v)

  const onStop = (e: React.MouseEvent) => {
    e.stopPropagation()
    if (!activeTabId) return
    stopGeneration(activeTabId)
  }
  const onKeepAll = (e: React.MouseEvent) => {
    e.stopPropagation()
    if (!activeTabId) return
    clearPendingEdits(activeTabId)
    setErrorMessage(null)
  }
  const onUndoAll = async (e: React.MouseEvent) => {
    e.stopPropagation()
    if (!activeTabId || undoing) return
    setUndoing(true)
    setErrorMessage(null)
    try {
      await undoAllPendingEdits(activeTabId)
    } catch (err) {
      setErrorMessage(
        err instanceof Error ? err.message : t('review.undoFailed'),
      )
    } finally {
      setUndoing(false)
    }
  }

  return (
    <div className="mb-1.5">
      <div
        className="flex w-full items-center gap-2 rounded-lg border border-[var(--color-border)]/40 bg-[var(--color-surface-container-low)] px-3 py-1.5 text-left transition-colors hover:bg-[var(--color-surface-container-high)]"
      >
        <button
          type="button"
          onClick={onToggle}
          aria-expanded={expanded}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
            {expanded ? 'expand_less' : 'expand_more'}
          </span>
          <span className="shrink-0 text-[12px] font-semibold text-[var(--color-text-secondary)]">
            {t('review.filesCount', { count: fileCount })}
          </span>
        </button>

        <div className="flex shrink-0 items-center gap-1.5">
          {isActive ? (
            <button
              type="button"
              onClick={onStop}
              title={t('chat.stopTitle')}
              className="flex items-center gap-1 rounded-md px-2 py-0.5 text-[11px] text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-container-high)]"
            >
              <span>{t('review.stop')}</span>
              <kbd className="font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]">
                {t('review.stopShortcut')}
              </kbd>
            </button>
          ) : (
            pendingEdits.length > 0 && (
              <>
                <button
                  type="button"
                  onClick={onUndoAll}
                  disabled={undoing}
                  className="rounded-md px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-container-high)] disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {undoing ? t('review.undoing') : t('review.undoAll')}
                </button>
                <button
                  type="button"
                  onClick={onKeepAll}
                  className="rounded-md px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-container-high)]"
                >
                  {t('review.keepAll')}
                </button>
              </>
            )
          )}
          <button
            type="button"
            onClick={onToggle}
            className="rounded-md border border-[var(--color-border)]/60 px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-container-high)]"
          >
            {t('review.review')}
          </button>
        </div>
      </div>

      {errorMessage && (
        <div className="mt-1 rounded-md border border-[var(--color-error)]/40 bg-[var(--color-error-container)]/40 px-3 py-1 text-[11px] text-[var(--color-error)]">
          {errorMessage}
        </div>
      )}

      {expanded && pendingEdits.length > 0 && (
        <div className="mt-1.5 max-h-[320px] overflow-y-auto rounded-lg border border-[var(--color-border)]/30 bg-[var(--color-surface-container-lowest)]">
          {pendingEdits.map((edit) => {
            const workspaceRoot = isWorkspaceRootPath(edit.path)
            const { dir, tail, separator } = splitPathForDisplay(edit.path)
            const isCuratorPath = /(?:^|[\\/])curator[\\/]/i.test(edit.path)
            const rowCls = isCuratorPath
              ? 'flex items-center gap-2 border-b border-[var(--color-border)]/20 px-3 py-1.5 last:border-b-0 bg-[var(--color-curator-accent)]/8 border-l-2 border-l-[var(--color-curator-accent)]/70'
              : 'flex items-center gap-2 border-b border-[var(--color-border)]/20 px-3 py-1.5 last:border-b-0'
            const iconCls = isCuratorPath
              ? 'material-symbols-outlined shrink-0 text-[14px] text-[var(--color-curator-accent)]'
              : 'material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]'
            return (
              <div
                key={edit.path}
                className={rowCls}
                title={edit.path}
              >
                <span className={iconCls}>
                  {isCuratorPath ? 'auto_stories' : 'description'}
                </span>
                {workspaceRoot ? (
                  <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] italic text-[var(--color-text-tertiary)]">
                    {t('tool.list.workspaceRoot')}
                  </span>
                ) : dir ? (
                  <span className="min-w-0 flex-1 flex items-baseline truncate">
                    <span className="min-w-0 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-tertiary)]">
                      {dir}
                      {separator}
                    </span>
                    <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
                      {tail}
                    </span>
                  </span>
                ) : (
                  <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
                    {tail || edit.path || 'file'}
                  </span>
                )}
                {edit.additions > 0 && (
                  <span className="shrink-0 rounded-full bg-[var(--color-success-container)]/50 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-success)]">
                    +{edit.additions}
                  </span>
                )}
                {edit.deletions > 0 && (
                  <span className="shrink-0 rounded-full bg-[var(--color-error-container)]/50 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-error)]">
                    -{edit.deletions}
                  </span>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
