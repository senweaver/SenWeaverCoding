// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo } from 'react'
import { useTranslation } from '../../i18n'

type Todo = {
  id: string
  content: string
  status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
  notes?: string | null
}

type Props = {
  planPath: string
  title: string
  todos: Todo[]
  superseded?: boolean
  handoffKind?: 'plan' | 'curator'
}

export function PlanProgressCard({ planPath, title, todos, superseded, handoffKind }: Props) {
  const t = useTranslation()
  const isCurator = handoffKind === 'curator'

  const stats = useMemo(() => {
    const total = todos.length
    const completed = todos.filter((x) => x.status === 'completed').length
    const cancelled = todos.filter((x) => x.status === 'cancelled').length
    return { total, completed, cancelled, done: completed + cancelled }
  }, [todos])

  const fileName = planPath
    ? planPath.split(/[\\/]/).pop() || planPath
    : ''

  return (
    <div className={`mb-3 ${superseded ? 'opacity-60 saturate-50' : ''}`}>
      <div className="rounded-[var(--radius-lg)] border border-[var(--color-success)]/45 ring-1 ring-[var(--color-success)]/20 bg-[var(--color-surface-container-lowest)] overflow-hidden">
        <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)]">
          <span className="material-symbols-outlined text-[14px] text-[var(--color-success)]">
            check_circle
          </span>
          <span className="text-[12px] font-semibold text-[var(--color-text-primary)] truncate">
            {title || t(isCurator ? 'curator.untitled' : 'plan.untitledPlan')}
          </span>
          {fileName && (
            <span
              className="font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)] truncate hidden sm:inline"
              title={planPath}
            >
              {fileName}
            </span>
          )}
          <span
            className="ml-auto flex items-center gap-1 text-[11px] tabular-nums text-[var(--color-text-secondary)] shrink-0"
            title={t('plan.stickyProgressTitle', {
              completed: stats.completed,
              cancelled: stats.cancelled,
              total: stats.total,
            })}
          >
            {t('plan.stickyProgress', {
              completed: stats.done,
              total: stats.total,
            })}
          </span>
        </div>

        <div className="px-3 py-2">
          <div className="text-[11px] font-medium text-[var(--color-text-tertiary)] mb-1.5">
            {t(isCurator ? 'curator.progressCardTitle' : 'plan.progressCardTitle')}
          </div>
          <ul className="space-y-1">
            {todos.map((todo) => (
              <li
                key={todo.id}
                className="flex items-start gap-2 text-[12px] leading-snug"
              >
                <span
                  className={`shrink-0 mt-[3px] inline-block h-3 w-3 rounded-full border ${todoDotClass(todo.status)}`}
                  aria-label={todo.status}
                />
                <span
                  className={
                    todo.status === 'completed'
                      ? 'line-through text-[var(--color-text-tertiary)]'
                      : todo.status === 'cancelled'
                        ? 'line-through text-[var(--color-text-tertiary)] opacity-70'
                        : 'text-[var(--color-text-primary)]'
                  }
                >
                  {todo.content}
                </span>
              </li>
            ))}
          </ul>
        </div>
      </div>
    </div>
  )
}

function todoDotClass(status: Todo['status']): string {
  switch (status) {
    case 'completed':
      return 'border-[var(--color-success)] bg-[var(--color-success)]'
    case 'in_progress':
      return 'border-[var(--color-plan-accent)] bg-[var(--color-plan-accent)]/30'
    case 'cancelled':
      return 'border-[var(--color-text-tertiary)] bg-[var(--color-text-tertiary)]/30'
    case 'pending':
    default:
      return 'border-[var(--color-outline-variant)] bg-transparent'
  }
}
