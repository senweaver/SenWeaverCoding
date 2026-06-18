// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo } from 'react'
import { useTranslation } from '../../i18n'
import type { LanGroupSnapshot } from '../../types/lanGroup'
import { formatDate, statusColor } from './shared'

export function GroupTimeline({ snapshot }: { snapshot: LanGroupSnapshot }) {
  const t = useTranslation()

  const rows = useMemo(() => {
    return snapshot.tasks
      .map((task) => {
        const created = task.createdAt || task.dueMs
        const due = task.dueMs || task.createdAt
        const start = Math.min(created, due)
        const end = Math.max(created, due)
        return { task, start, end }
      })
      .filter((row) => row.start > 0)
      .sort((a, b) => a.start - b.start)
  }, [snapshot.tasks])

  const range = useMemo(() => {
    if (rows.length === 0) return null
    const min = Math.min(...rows.map((r) => r.start))
    const max = Math.max(...rows.map((r) => r.end))
    const span = Math.max(max - min, 1)
    return { min, max, span }
  }, [rows])

  if (!range) {
    return (
      <div className="flex h-full items-center justify-center px-3 text-center text-xs text-[var(--color-text-tertiary)]">
        {t('lanGroup.timelineEmpty')}
      </div>
    )
  }

  return (
    <div className="h-full overflow-y-auto p-3">
      <div className="mb-2 flex items-center justify-between text-[10px] text-[var(--color-text-tertiary)]">
        <span>{formatDate(range.min)}</span>
        <span>{formatDate(range.max)}</span>
      </div>
      <div className="space-y-1.5">
        {rows.map(({ task, start, end }) => {
          const left = ((start - range.min) / range.span) * 100
          const width = Math.max(((end - start) / range.span) * 100, 1.5)
          const isMilestone = task.kind === 'milestone'
          return (
            <div key={task.id} className="flex items-center gap-2">
              <span className="w-28 shrink-0 truncate text-[11px] text-[var(--color-text-secondary)]">
                {task.title}
              </span>
              <div className="relative h-4 flex-1 rounded bg-[var(--color-surface-hover)]">
                {isMilestone ? (
                  <span
                    className="absolute top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rotate-45"
                    style={{ left: `${left}%`, background: '#f59e0b' }}
                    title={task.title}
                  />
                ) : (
                  <span
                    className="absolute top-0 h-4 rounded"
                    style={{
                      left: `${left}%`,
                      width: `${width}%`,
                      background: statusColor(task.status),
                      opacity: 0.85,
                    }}
                    title={task.title}
                  />
                )}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}
