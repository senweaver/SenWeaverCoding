// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useChatStore } from '../../stores/chatStore'
import { useTranslation } from '../../i18n'
import { Modal } from '../shared/Modal'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import {
  selectPlanCardExecutionState,
  type PlanExecutionState,
} from '../../utils/activePlanSelector'

type Todo = {
  id: string
  content: string
  status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
}

type Props = {
  messageId: string
  planPath: string
  fileName: string
  title: string
  overview: string
  todos: Todo[]
  markdown?: string
  modelLabel?: string
  status: 'writing' | 'completed' | 'failed'
  error?: string
  superseded?: boolean
  sessionId?: string | null
}

export function PlanCard({
  messageId,
  planPath,
  fileName,
  title,
  overview,
  todos,
  markdown,
  modelLabel,
  status,
  error,
  superseded,
  sessionId,
}: Props) {
  const t = useTranslation()
  const [viewOpen, setViewOpen] = useState(false)
  const [pathCopied, setPathCopied] = useState(false)
  const requestModeSwitch = useChatStore((s) => s.requestModeSwitch)
  const cardRef = useRef<HTMLDivElement>(null)
  const announcedCompletionRef = useRef(false)

  const planInputs = useChatStore(
    useShallow((s) => {
      const session = sessionId ? s.sessions[sessionId] : undefined
      return {
        messages: session?.messages,
        chatState: session?.chatState,
      }
    }),
  )
  const execState = useMemo<PlanExecutionState>(() => {
    if (!sessionId || !planInputs.messages) return 'idle'
    return selectPlanCardExecutionState(
      planInputs.messages,
      messageId,
      planInputs.chatState ?? 'idle',
    )
  }, [sessionId, planInputs.messages, planInputs.chatState, messageId])
  const resumePlanExecution = useChatStore((s) => s.resumePlanExecution)

  const completed = status === 'completed'
  const failed = status === 'failed'
  const visibleTodos = todos.slice(0, 3)
  const moreCount = Math.max(0, todos.length - visibleTodos.length)
  const showMoreLabel = moreCount > 0 ? t('plan.todosShowMore', { count: moreCount }) : ''

  useEffect(() => {
    if (!completed || announcedCompletionRef.current) return
    announcedCompletionRef.current = true
    if (typeof document !== 'undefined' && document.hidden) return
    requestAnimationFrame(() => {
      if (typeof document !== 'undefined' && document.hidden) return
      cardRef.current?.scrollIntoView({ block: 'end' })
    })
  }, [completed])

  function handleBuild() {
    if (!completed || !sessionId || !planPath) return
    requestModeSwitch(sessionId, planPath)
  }

  async function handleCopyPath() {
    if (!planPath) return
    try {
      if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
        await navigator.clipboard.writeText(planPath)
      } else {
        const ta = document.createElement('textarea')
        ta.value = planPath
        ta.style.position = 'fixed'
        ta.style.opacity = '0'
        document.body.appendChild(ta)
        ta.select()
        document.execCommand('copy')
        document.body.removeChild(ta)
      }
      setPathCopied(true)
      setTimeout(() => setPathCopied(false), 1600)
    } catch {

      setPathCopied(false)
    }
  }

  const containerEmphasis = completed
    ? 'border-[var(--color-plan-accent)]/55 ring-1 ring-[var(--color-plan-accent)]/25 shadow-[0_2px_18px_-8px_var(--color-plan-accent)]'
    : 'border-[var(--color-outline-variant)]/40'

  return (
    <div
      ref={cardRef}
      className={`mb-3 ${superseded ? 'opacity-60 saturate-50 pointer-events-none' : ''}`}
    >
      <div
        className={`rounded-[var(--radius-lg)] border ${containerEmphasis} bg-[var(--color-surface-container-lowest)] overflow-hidden transition-all`}
      >
        <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)]">
          <span className="material-symbols-outlined text-[14px] text-[var(--color-plan-accent)]">
            description
          </span>
          <span
            className="font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)] truncate"
            title={planPath || fileName}
          >
            {fileName}
          </span>
          {planPath && (
            <button
              type="button"
              onClick={handleCopyPath}
              title={pathCopied ? t('plan.copyPathDone') : t('plan.copyPath', { path: planPath })}
              aria-label={t('plan.copyPath', { path: planPath })}
              className="inline-flex items-center justify-center rounded-[var(--radius-sm)] px-1 py-0.5 text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-container)] hover:text-[var(--color-text-primary)] transition-colors"
            >
              <span className="material-symbols-outlined text-[14px]">
                {pathCopied ? 'check' : 'content_copy'}
              </span>
            </button>
          )}
          <span className="ml-auto flex items-center gap-1.5">
            {failed ? (
              <span
                className="text-[11px] text-[var(--color-error)] flex items-center gap-1"
                title={error || t('plan.failedHint')}
              >
                <span className="material-symbols-outlined text-[12px]">error</span>
                {t('plan.failed')}
              </span>
            ) : !completed ? (
              <span className="text-[11px] text-[var(--color-text-tertiary)] flex items-center gap-1">
                <span className="material-symbols-outlined text-[12px] animate-spin">progress_activity</span>
                {t('plan.writingPlan')}
              </span>
            ) : null}
          </span>
        </div>

        <div className="px-3 py-2.5">
          <div className="text-[14px] font-bold text-[var(--color-text-primary)] leading-tight">
            {title || t('plan.untitledPlan')}
          </div>
          {overview && (
            <div className="mt-1 text-[12px] text-[var(--color-text-secondary)] leading-relaxed line-clamp-3">
              {overview}
            </div>
          )}

          {completed && todos.length > 0 && (
            <div className="mt-2.5 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] px-2 py-1.5">
              {}
              <div className="text-[11px] font-medium text-[var(--color-text-tertiary)] mb-1">
                {t('plan.todosCount', { count: todos.length })}
              </div>
              <ul className="space-y-1">
                {visibleTodos.map((todo) => (
                  <li key={todo.id} className="flex items-start gap-1.5 text-[12px]">
                    <span className={`shrink-0 mt-[3px] inline-block h-3 w-3 rounded-full border ${todoStatusClass(todo.status)}`} />
                    <span className="leading-snug text-[var(--color-text-primary)]">
                      {todo.content}
                    </span>
                  </li>
                ))}
                {moreCount > 0 && (
                  <li className="pl-[18px] text-[11px] text-[var(--color-text-tertiary)]">
                    {showMoreLabel}
                  </li>
                )}
              </ul>
            </div>
          )}
        </div>

        <div className="flex items-center justify-between gap-2 border-t border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)] px-3 py-1.5">
          <button
            type="button"
            onClick={() => setViewOpen(true)}
            disabled={!markdown && !planPath}
            className="flex items-center gap-1 text-[11px] font-medium text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] disabled:opacity-40 transition-colors"
          >
            <span className="material-symbols-outlined text-[12px]">description</span>
            {t('plan.viewPlan')}
          </button>
          <div className="flex items-center gap-2">
            {modelLabel && (
              <span className="text-[11px] text-[var(--color-text-tertiary)] truncate max-w-[120px]">
                {modelLabel}
              </span>
            )}
            {execState === 'executing' ? (
              <span
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-surface-container-low)] text-[var(--color-text-tertiary)] cursor-not-allowed select-none"
                aria-label={t('plan.executing')}
              >
                <span className="material-symbols-outlined text-[14px] animate-spin">
                  progress_activity
                </span>
                {t('plan.executing')}
              </span>
            ) : execState === 'incomplete_run' ? (
              <button
                type="button"
                onClick={() => {
                  if (!sessionId || !planPath) return
                  resumePlanExecution(sessionId, planPath)
                }}
                disabled={!sessionId || !planPath}
                title={t('plan.resumeTitle')}
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-plan-accent)] text-[var(--color-on-plan-accent-container)] hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed transition-all"
              >
                <span className="material-symbols-outlined text-[14px]">
                  play_arrow
                </span>
                {t('plan.resume')}
              </button>
            ) : execState === 'completed_run' ? (
              <span
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-success)]/15 text-[var(--color-success)] cursor-default select-none"
                aria-label={t('plan.completed')}
              >
                <span className="material-symbols-outlined text-[14px]">
                  check_circle
                </span>
                {t('plan.completed')}
              </span>
            ) : failed ? (
              <span
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-error)]/15 text-[var(--color-error)] cursor-default select-none"
                title={error || t('plan.failedHint')}
                aria-label={t('plan.failed')}
              >
                <span className="material-symbols-outlined text-[14px]">error</span>
                {t('plan.failed')}
              </span>
            ) : (
              <button
                type="button"
                onClick={handleBuild}
                disabled={
                  !completed || !planPath || execState === 'pending_switch'
                }
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-plan-accent)] text-[var(--color-on-plan-accent-container)] hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed transition-all"
              >
                {t('plan.build')}
                <span className="text-[10px] px-1 py-0.5 rounded bg-[var(--color-plan-accent-hover)]/20">
                  {t('plan.buildShortcut')}
                </span>
              </button>
            )}
          </div>
        </div>
      </div>

      <Modal open={viewOpen} onClose={() => setViewOpen(false)} title={fileName} width={760}>
        <div className="max-h-[70vh] overflow-y-auto px-1">
          {markdown ? (
            <div className="markdown-prose prose prose-sm max-w-none">
              <MarkdownRenderer content={markdown} />
            </div>
          ) : (
            <div className="text-[12px] text-[var(--color-text-tertiary)] py-2 break-all">
              {planPath}
            </div>
          )}
        </div>
      </Modal>
    </div>
  )
}

function todoStatusClass(status: Todo['status']): string {
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
