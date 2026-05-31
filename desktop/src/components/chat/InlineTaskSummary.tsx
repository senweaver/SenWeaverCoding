// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import type { TaskSummaryItem } from '../../types/chat'
import { useTranslation } from '../../i18n'

const statusIcon: Record<TaskSummaryItem['status'], string> = {
  pending: 'radio_button_unchecked',
  in_progress: 'pending',
  completed: 'check_circle',
}

const statusColor: Record<TaskSummaryItem['status'], string> = {
  pending: 'var(--color-text-tertiary)',
  in_progress: 'var(--color-warning)',
  completed: 'var(--color-success)',
}

export function InlineTaskSummary({ tasks }: { tasks: TaskSummaryItem[] }) {
  const t = useTranslation()
  const completed = tasks.filter((tk) => tk.status === 'completed').length
  const total = tasks.length
  const allDone = total > 0 && completed === total

  const [expanded, setExpanded] = useState(!allDone)

  const headerLabel = allDone ? t('tasks.collapseList') : t('tasks.expandList')
  const toggleLabel = expanded ? t('tasks.collapseList') : t('tasks.expandList')

  return (
    <div className="mb-2 rounded-[var(--radius-lg)] border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] overflow-hidden">
      <button
        type="button"
        onClick={() => setExpanded((prev) => !prev)}
        aria-expanded={expanded}
        aria-label={toggleLabel}
        title={headerLabel}
        className="flex w-full items-center gap-3 px-4 py-2 bg-[var(--color-surface-container)] text-left transition-colors hover:bg-[var(--color-surface-container-high)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-primary)]"
      >
        <div className="flex items-center justify-center w-5 h-5 rounded-[var(--radius-md)] bg-[var(--color-success)]/10">
          <span
            className="material-symbols-outlined text-[13px] text-[var(--color-success)]"
            style={{ fontVariationSettings: "'FILL' 1" }}
          >
            task_alt
          </span>
        </div>
        <span className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('tasks.completed')}
        </span>
        <span className="text-[10px] text-[var(--color-text-tertiary)] tabular-nums">
          {completed}/{total}
        </span>
        <span
          className="material-symbols-outlined ml-auto text-[16px] text-[var(--color-text-tertiary)] transition-transform"
          style={{
            transform: expanded ? 'rotate(180deg)' : 'rotate(0deg)',
          }}
          aria-hidden="true"
        >
          expand_more
        </span>
      </button>
      {expanded && (
        <div className="px-4 py-1.5 flex flex-col">
          {tasks.map((task) => (
            <div key={task.id} className="flex items-center gap-2 py-0.5 px-1">
              <span
                className="material-symbols-outlined text-[14px] shrink-0"
                style={{ color: statusColor[task.status], fontVariationSettings: "'FILL' 1" }}
              >
                {statusIcon[task.status]}
              </span>
              <span className="text-[10px] font-mono text-[var(--color-text-tertiary)]">
                #{task.id}
              </span>
              <span
                className={`text-xs ${
                  task.status === 'completed'
                    ? 'text-[var(--color-text-tertiary)] line-through'
                    : 'text-[var(--color-text-primary)]'
                }`}
              >
                {task.subject}
              </span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}
