// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useChatStore } from '../../stores/chatStore'
import { useTranslation } from '../../i18n'
import {
  selectActiveExecutingPlan,
  type PlanExecutionState,
} from '../../utils/activePlanSelector'
import { selectActiveExecutingCurator } from '../../utils/activeCuratorSelector'

type Props = {
  sessionId?: string | null
}

type ActiveTracker = {
  kind: 'plan' | 'curator'
  title: string
  fileName: string
  overview: string
  todos: Array<{
    id: string
    content: string
    status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
    notes?: string | null
  }>
  resumePath: string
  pendingHydration: boolean
  state: PlanExecutionState | 'executing' | 'incomplete_run'
}

export function ActivePlanStickyBar({ sessionId }: Props) {
  const t = useTranslation()
  const [expanded, setExpanded] = useState(false)
  const resumePlanExecution = useChatStore((s) => s.resumePlanExecution)
  const resumeCuratorExecution = useChatStore((s) => s.resumeCuratorExecution)

  const view = useChatStore(
    useShallow((s): ActiveTracker | null => {
      if (!sessionId) return null
      const session = s.sessions[sessionId]
      if (!session) return null
      const plan = selectActiveExecutingPlan(session.messages, session.chatState)
      if (plan) {
        return {
          kind: 'plan',
          title: plan.card.title || t('plan.untitledPlan'),
          fileName: plan.card.fileName || '',
          overview: plan.card.overview || '',
          todos: plan.card.todos,
          resumePath: plan.card.planPath,
          pendingHydration: !!plan.card.pendingHydration,
          state: plan.state,
        }
      }
      const curator = selectActiveExecutingCurator(session.messages, session.chatState)
      if (curator) {
        return {
          kind: 'curator',
          title: curator.card.title || t('curator.untitled'),
          fileName: curator.card.slug || '',
          overview: '',
          todos: curator.card.todos ?? [],
          resumePath: curator.card.implBlueprintPath || curator.card.finalMdPath,
          pendingHydration: !!curator.card.pendingHydration,
          state: curator.state,
        }
      }
      return null
    }),
  )

  const stats = useMemo(() => {
    if (!view)
      return {
        total: 0,
        completed: 0,
        cancelled: 0,
        inProgress: 0,
        done: 0,
        ratio: 0,
      }
    const total = view.todos.length
    const completed = view.todos.filter((x) => x.status === 'completed').length
    const cancelled = view.todos.filter((x) => x.status === 'cancelled').length
    const inProgress = view.todos.filter((x) => x.status === 'in_progress').length
    const done = completed + cancelled
    return {
      total,
      completed,
      cancelled,
      inProgress,
      done,
      ratio: total > 0 ? done / total : 0,
    }
  }, [view])

  if (!view) return null

  const fileName = view.fileName
  const title = view.title
  const isIncomplete = view.state === 'incomplete_run'
  const isHydrating = !!view.pendingHydration && !isIncomplete

  return (
    <div className="shrink-0 px-8">
      <div
        className={[
          'mx-auto max-w-[860px] mb-2 overflow-hidden',
          'rounded-[var(--radius-lg)] border bg-[var(--color-surface-container-lowest)]',
          'border-[var(--color-plan-accent)]/55 ring-1 ring-[var(--color-plan-accent)]/25',
          'shadow-[0_4px_22px_-10px_var(--color-plan-accent)]',
          'transition-all',
        ].join(' ')}
      >
        {}
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="w-full flex items-center gap-2 px-3 py-2 text-left hover:bg-[var(--color-surface-container-low)] transition-colors"
          aria-expanded={expanded}
        >
          <span className="material-symbols-outlined text-[16px] text-[var(--color-plan-accent)] shrink-0">
            description
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 min-w-0">
              <span className="text-[12px] font-semibold text-[var(--color-text-primary)] truncate">
                {title}
              </span>
              {fileName && (
                <span className="font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)] truncate hidden sm:inline">
                  {fileName}
                </span>
              )}
            </div>
            {}
            <div className="mt-1 h-1 w-full rounded-full bg-[var(--color-outline-variant)]/30 overflow-hidden">
              {isHydrating ? (
                <div className="h-full w-full rounded-full bg-[var(--color-plan-accent)]/40 animate-pulse" />
              ) : (
                <div
                  className="h-full rounded-full bg-[var(--color-plan-accent)] transition-all duration-300"
                  style={{ width: `${Math.round(stats.ratio * 100)}%` }}
                />
              )}
            </div>
          </div>
          <span
            className="flex items-baseline gap-1 text-[11px] tabular-nums text-[var(--color-text-secondary)] shrink-0"
            title={
              isHydrating
                ? t('plan.hydratingDetail')
                : t('plan.stickyProgressTitle', {
                    completed: stats.completed,
                    cancelled: stats.cancelled,
                    total: stats.total,
                  })
            }
          >
            {isHydrating ? (
              <span className="text-[var(--color-text-tertiary)]">—/—</span>
            ) : (
              <>
                <span>
                  {t('plan.stickyProgress', {
                    completed: stats.done,
                    total: stats.total,
                  })}
                </span>
                {stats.cancelled > 0 && (
                  <span className="text-[10px] text-[var(--color-text-tertiary)]">
                    {t('plan.stickySucceeded', {
                      succeeded: stats.completed,
                    })}
                  </span>
                )}
              </>
            )}
          </span>
          {}
          {isIncomplete ? (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                if (!sessionId || !view.resumePath) return
                if (view.kind === 'curator') {
                  resumeCuratorExecution(sessionId, view.resumePath)
                } else {
                  resumePlanExecution(sessionId, view.resumePath)
                }
              }}
              disabled={!sessionId || !view.resumePath}
              title={t('plan.resumeTitle')}
              className="flex items-center gap-1 shrink-0 rounded-[var(--radius-md)] bg-[var(--color-plan-accent)] px-2 py-0.5 text-[11px] font-semibold text-[var(--color-on-plan-accent-container)] hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed transition-all"
            >
              <span className="material-symbols-outlined text-[14px]">
                play_arrow
              </span>
              {t('plan.resume')}
            </button>
          ) : (
            <span className="flex items-center gap-1 shrink-0 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-tertiary)]">
              <span className="material-symbols-outlined text-[14px] animate-spin">
                progress_activity
              </span>
              {isHydrating ? t('plan.hydrating') : t('plan.executing')}
            </span>
          )}
          <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)] shrink-0">
            {expanded ? 'expand_more' : 'expand_less'}
          </span>
        </button>

        {}
        {expanded && (
          <div className="border-t border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)]/40 px-3 py-2 max-h-[40vh] overflow-y-auto">
            {isHydrating && (
              <div className="mb-2 flex items-start gap-2 rounded-[var(--radius-md)] border border-[var(--color-plan-accent)]/40 bg-[var(--color-plan-accent)]/10 px-2 py-1.5 text-[11px] leading-relaxed text-[var(--color-text-secondary)]">
                <span className="material-symbols-outlined text-[14px] text-[var(--color-plan-accent)] animate-spin shrink-0">
                  progress_activity
                </span>
                <span>{t('plan.hydratingDetail')}</span>
              </div>
            )}
            {view.overview && (
              <div className="mb-2 text-[11px] leading-relaxed text-[var(--color-text-secondary)]">
                {view.overview}
              </div>
            )}
            <ul className={isHydrating ? 'space-y-1 opacity-60' : 'space-y-1'}>
              {view.todos.map((todo) => (
                <li
                  key={todo.id}
                  className="flex items-start gap-2 text-[12px] leading-snug"
                >
                  <span
                    className={`shrink-0 mt-[3px] inline-flex items-center justify-center h-3 w-3 rounded-full border ${todoDotClass(todo.status)}`}
                    aria-label={todo.status}
                  >
                    {todo.status === 'in_progress' && (
                      <span className="material-symbols-outlined text-[10px] text-[var(--color-plan-accent)] animate-spin">
                        progress_activity
                      </span>
                    )}
                  </span>
                  <span
                    className={
                      todo.status === 'completed'
                        ? 'line-through text-[var(--color-text-tertiary)]'
                        : todo.status === 'cancelled'
                          ? 'line-through text-[var(--color-text-tertiary)] opacity-70'
                          : todo.status === 'in_progress'
                            ? 'text-[var(--color-text-primary)] font-medium'
                            : 'text-[var(--color-text-primary)]'
                    }
                  >
                    {todo.content}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  )
}

function todoDotClass(
  status: 'pending' | 'in_progress' | 'completed' | 'cancelled',
): string {
  switch (status) {
    case 'completed':
      return 'border-[var(--color-success)] bg-[var(--color-success)]'
    case 'in_progress':
      return 'border-[var(--color-plan-accent)] bg-[var(--color-plan-accent)]/20'
    case 'cancelled':
      return 'border-[var(--color-text-tertiary)] bg-[var(--color-text-tertiary)]/30'
    case 'pending':
    default:
      return 'border-[var(--color-outline-variant)] bg-transparent'
  }
}
