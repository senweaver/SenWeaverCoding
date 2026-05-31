// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'
import { EMPTY_WORKERS, useWorkersStore } from '../../stores/workersStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation } from '../../i18n'
import { truncate } from '../../utils/toolFormatters'
import type { WorkerSnapshot } from '../../types/chat'

type Props = {
  sessionId: string | null
}

function statusDot(status: WorkerSnapshot['status']): string {
  switch (status) {
    case 'running':
    case 'pending':
      return 'bg-[var(--color-warning)] animate-pulse'
    case 'completed':
      return 'bg-[var(--color-success)]'
    case 'failed':
      return 'bg-[var(--color-error)]'
    case 'stopped':
      return 'bg-[var(--color-text-tertiary)]'
    default:
      return 'bg-[var(--color-outline)]'
  }
}

export function WorkersStrip({ sessionId }: Props) {
  const t = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const workers = useWorkersStore((s) =>
    sessionId ? s.workersByParent[sessionId] ?? EMPTY_WORKERS : EMPTY_WORKERS,
  )
  const openWorkerTab = useTabStore((s) => s.openWorkerTab)
  const stopWorker = useWorkersStore((s) => s.stopWorker)

  const running = useMemo(
    () =>
      workers.filter(
        (w) => w.status === 'running' || w.status === 'pending',
      ),
    [workers],
  )

  if (!sessionId || running.length === 0) return null

  return (
    <div className="mx-auto w-full max-w-[860px] px-8 pb-1">
      <div className="overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--color-surface-hover)]/50"
          aria-expanded={expanded}
        >
          <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
            smart_toy
          </span>
          <span className="shrink-0 text-[11px] font-semibold text-[var(--color-text-secondary)]">
            {t('chat.workers.runningCount', { count: running.length }) ||
              `${running.length} subagents running`}
          </span>
          <span className="ml-auto material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
            {expanded ? 'expand_less' : 'expand_more'}
          </span>
        </button>
        {expanded && (
          <ul className="border-t border-[var(--color-border)]/60 px-2 py-1.5 space-y-1">
            {running.map((w) => (
              <li
                key={w.workerId}
                className="flex items-center gap-2 rounded-md px-2 py-1 hover:bg-[var(--color-surface-hover)]/50"
              >
                <span className={`shrink-0 size-2 rounded-full ${statusDot(w.status)}`} />
                <button
                  type="button"
                  onClick={() => openWorkerTab(w.workerId, w.title || w.workerId)}
                  className="min-w-0 flex-1 truncate text-left text-[12px] text-[var(--color-text-primary)] hover:underline"
                  title={t('chat.workers.openDetail') || 'Open detail'}
                >
                  {truncate(w.title || w.workerId, 60)}
                </button>
                {(w.lastAction || w.lastDetail) && (
                  <span
                    className="hidden sm:inline-block min-w-0 max-w-[200px] truncate text-[10px] text-[var(--color-text-tertiary)]"
                    title={`${w.lastAction ?? ''} ${w.lastDetail ?? ''}`.trim()}
                  >
                    {w.lastAction ? `${w.lastAction} · ` : ''}
                    {w.lastDetail ?? ''}
                  </span>
                )}
                <button
                  type="button"
                  onClick={() => {
                    void stopWorker(w.workerId)
                  }}
                  className="shrink-0 inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)]"
                  title={t('chat.workers.stopWorker') || 'Stop'}
                >
                  <span className="material-symbols-outlined text-[12px]">stop</span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  )
}
