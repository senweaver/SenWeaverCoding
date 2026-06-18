// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo } from 'react'
import { useTranslation } from '../../i18n'
import { useSessionStore } from '../../stores/sessionStore'
import {
  useWorkspaceQueueStore,
  workspaceKeyFor,
  type QueuedItem,
} from '../../stores/workspaceQueueStore'
import { resolveSessionTitle } from '../../utils/sessionTitle'
import { Spinner } from '../shared/Spinner'
import { refsToPlainText } from './composerRefs'

const PREVIEW_LIMIT = 80

function previewText(text: string): string {
  const trimmed = refsToPlainText(text).replace(/\s+/g, ' ').trim()
  if (trimmed.length <= PREVIEW_LIMIT) return trimmed
  return `${trimmed.slice(0, PREVIEW_LIMIT)}\u2026`
}

type Props = {
  sessionId: string | null | undefined
}

export function WorkspaceQueuePanel({ sessionId }: Props) {
  const t = useTranslation()
  const sessions = useSessionStore((s) => s.sessions)
  const queues = useWorkspaceQueueStore((s) => s.queues)
  const expandedSessions = useWorkspaceQueueStore((s) => s.expandedSessions)
  const cancel = useWorkspaceQueueStore((s) => s.cancel)
  const cancelAllForSession = useWorkspaceQueueStore((s) => s.cancelAllForSession)
  const toggleExpanded = useWorkspaceQueueStore((s) => s.toggleExpanded)

  const session = useMemo(
    () => (sessionId ? sessions.find((s) => s.id === sessionId) ?? null : null),
    [sessions, sessionId],
  )

  const workspaceKey = useMemo(
    () => workspaceKeyFor(session, sessionId ?? undefined),
    [session, sessionId],
  )

  const list = useMemo(
    () => (sessionId ? queues[workspaceKey] ?? [] : []),
    [sessionId, queues, workspaceKey],
  )
  const ownItems = useMemo(
    () => (sessionId ? list.filter((i) => i.sessionId === sessionId) : []),
    [list, sessionId],
  )
  const otherItems = useMemo(
    () => (sessionId ? list.filter((i) => i.sessionId !== sessionId) : []),
    [list, sessionId],
  )

  const otherSessionTitle = useMemo(() => {
    const firstOther = otherItems[0]
    if (!firstOther) return ''
    const meta = sessions.find((s) => s.id === firstOther.sessionId)
    return resolveSessionTitle(meta?.title, t('sidebar.untitled'))
  }, [otherItems, sessions, t])

  if (!sessionId) return null
  if (ownItems.length === 0 && otherItems.length === 0) return null

  const expanded = expandedSessions.has(sessionId)
  const ownCount = ownItems.length
  const otherCount = otherItems.length

  const headerLabel = ownCount > 0
    ? t('composer.queue.title', { count: ownCount })
    : t('composer.queue.waitingFor', { title: otherSessionTitle })

  return (
    <div
      className="mb-1.5 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-low)]"
      data-testid="workspace-queue-panel"
    >
      <button
        type="button"
        onClick={() => toggleExpanded(sessionId)}
        className="flex w-full items-center gap-2 rounded-lg px-3 py-1.5 text-left text-xs text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]"
        aria-expanded={expanded}
      >
        <span className="inline-flex items-center text-[var(--color-brand)]">
          <Spinner size={9} />
        </span>
        <span className="material-symbols-outlined text-[14px] flex-shrink-0 text-[var(--color-text-tertiary)]">
          playlist_play
        </span>
        <span className="flex-1 truncate">{headerLabel}</span>
        {otherCount > 0 && ownCount > 0 && (
          <span
            className="flex-shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 text-[10px] tabular-nums text-[var(--color-text-tertiary)]"
            title={t('composer.queue.waitingFor', { title: otherSessionTitle })}
          >
            +{otherCount}
          </span>
        )}
        {ownCount > 0 && (
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation()
              cancelAllForSession(sessionId)
            }}
            className="flex-shrink-0 rounded px-1.5 py-0.5 text-[11px] text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-error)]"
          >
            {t('composer.queue.cancelAll')}
          </button>
        )}
        <span
          className="material-symbols-outlined flex-shrink-0 text-[16px] text-[var(--color-text-tertiary)]"
          style={{ transform: expanded ? 'rotate(180deg)' : 'rotate(0deg)' }}
          aria-hidden="true"
        >
          expand_more
        </span>
      </button>

      {expanded && (
        <div className="border-t border-[var(--color-border)] px-2 py-1.5">
          {ownItems.length > 0 && (
            <ul className="flex flex-col gap-1">
              {ownItems.map((item, idx) => (
                <QueueRow
                  key={item.id}
                  item={item}
                  rank={idx + 1}
                  onCancel={() => cancel(item.id)}
                  attachmentsLabel={
                    item.attachments && item.attachments.length > 0
                      ? t('composer.queue.itemAttachments', { count: item.attachments.length })
                      : ''
                  }
                />
              ))}
            </ul>
          )}
          <div className="mt-1.5 px-1 text-[11px] text-[var(--color-text-tertiary)]">
            {t('composer.queue.autoSendHint')}
          </div>
        </div>
      )}
    </div>
  )
}

function QueueRow({
  item,
  rank,
  onCancel,
  attachmentsLabel,
}: {
  item: QueuedItem
  rank: number
  onCancel: () => void
  attachmentsLabel: string
}) {
  return (
    <li className="group flex items-start gap-2 rounded px-1.5 py-1 hover:bg-[var(--color-surface-hover)]">
      <span className="mt-[2px] inline-flex h-4 min-w-[18px] flex-shrink-0 items-center justify-center rounded bg-[var(--color-surface-container-high)] px-1 text-[10px] tabular-nums text-[var(--color-text-tertiary)]">
        #{rank}
      </span>
      <span className="min-w-0 flex-1 break-words text-[12px] leading-snug text-[var(--color-text-primary)]">
        {previewText(item.content) || item.options?.displayContent || ''}
        {attachmentsLabel && (
          <span className="ml-1 text-[11px] text-[var(--color-text-tertiary)]">
            ({attachmentsLabel})
          </span>
        )}
      </span>
      <button
        type="button"
        onClick={onCancel}
        aria-label="cancel"
        className="opacity-0 transition-opacity group-hover:opacity-100 flex-shrink-0 rounded p-0.5 text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-error)]"
      >
        <span className="material-symbols-outlined text-[14px] leading-none">close</span>
      </button>
    </li>
  )
}
