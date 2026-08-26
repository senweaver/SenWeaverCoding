// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTaskStore } from '../../stores/taskStore'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation } from '../../i18n'
import { parseRunOutput } from '../../lib/parseRunOutput'
import type { TaskRun } from '../../types/task'

function RunOutput({ run }: { run: TaskRun }) {
  const t = useTranslation()

  if (run.error) {
    return (
      <div className="mt-2 max-h-40 overflow-y-auto whitespace-pre-wrap break-words rounded-xl border border-[var(--color-error)]/20 bg-[var(--color-error-container)]/28 p-2.5 text-xs text-[var(--color-error)]">
        {run.error}
      </div>
    )
  }

  const text = parseRunOutput(run.output || '')

  if (!text) {
    return (
      <div className="mt-2 rounded-xl bg-[var(--color-surface-container)] p-2.5 text-xs italic text-[var(--color-text-tertiary)]">
        {run.sessionId ? t('tasks.outputHintSession') : t('tasks.noOutputText')}
      </div>
    )
  }

  return (
    <div className="mt-2 max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded-xl bg-[var(--color-surface-container)] p-2.5 text-xs leading-relaxed text-[var(--color-text-tertiary)]">
      {text}
    </div>
  )
}

type Props = {
  taskId: string
  onClose: () => void
  refreshKey?: number
}

const STATUS_CONFIG: Record<string, { icon: string; color: string }> = {
  running:   { icon: 'sync',         color: 'var(--color-warning)' },
  completed: { icon: 'check_circle', color: 'var(--color-success)' },
  failed:    { icon: 'error',        color: 'var(--color-error)' },
  timeout:   { icon: 'timer_off',    color: 'var(--color-error)' },
}

export function TaskRunsPanel({ taskId, onClose, refreshKey }: Props) {
  const t = useTranslation()
  const fetchTaskRuns = useTaskStore((s) => s.fetchTaskRuns)
  const connectToSession = useChatStore((s) => s.connectToSession)
  const openTab = useTabStore((s) => s.openTab)
  const [runs, setRuns] = useState<TaskRun[]>([])
  const [loading, setLoading] = useState(true)
  const [expandedId, setExpandedId] = useState<string | null>(null)

  const openSession = (sessionId: string, taskName?: string) => {
    openTab(sessionId, taskName || 'Task Run')
    connectToSession(sessionId)
  }

  const refresh = () => {
    fetchTaskRuns(taskId).then((r) => {
      setRuns(r)
      setLoading(false)
    }).catch(() => setLoading(false))
  }

  useEffect(() => {
    setLoading(true)
    refresh()
  }, [taskId, fetchTaskRuns, refreshKey])

  const hasRunning = runs.some((r) => r.status === 'running')
  useEffect(() => {
    if (!hasRunning && refreshKey === 0) return

    let interval = 1000
    let timer = setInterval(refresh, interval)

    const slowDown = setTimeout(() => {
      clearInterval(timer)
      if (hasRunning) {
        timer = setInterval(refresh, 3000)
      }
    }, 10000)

    const stopTimer = hasRunning ? undefined : setTimeout(() => clearInterval(timer), 12000)
    return () => {
      clearInterval(timer)
      clearTimeout(slowDown)
      if (stopTimer) clearTimeout(stopTimer)
    }
  }, [hasRunning, taskId, refreshKey])

  return (
    <div className="overflow-hidden rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)]">
      <div className="flex items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2">
        <span className="text-xs font-semibold text-[var(--color-text-primary)]">{t('tasks.logsTitle')}</span>
        <button
          onClick={onClose}
          className="p-0.5 text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-primary)]"
        >
          <span className="material-symbols-outlined text-[16px]">close</span>
        </button>
      </div>

      <div className="max-h-64 overflow-y-auto">
        {loading ? (
          <div className="flex items-center justify-center py-6">
            <div className="h-4 w-4 animate-spin rounded-full border-2 border-[var(--color-brand)] border-t-transparent" />
          </div>
        ) : runs.length === 0 ? (
          <div className="px-3 py-6 text-center text-xs text-[var(--color-text-tertiary)]">
            {t('tasks.noLogs')}
          </div>
        ) : (
          <div className="divide-y divide-[var(--color-border)]">
            {runs.map((run) => {
              const cfg = STATUS_CONFIG[run.status] || STATUS_CONFIG.failed!
              const isExpanded = expandedId === run.id
              return (
                <div key={run.id} className="px-3 py-2">
                  <div className="flex items-center gap-3">
                    <span
                      className={`material-symbols-outlined text-[16px] ${run.status === 'running' ? 'animate-spin' : ''}`}
                      style={{ color: cfg.color, fontVariationSettings: "'FILL' 1" }}
                    >
                      {cfg.icon}
                    </span>

                    <span className="text-xs font-medium" style={{ color: cfg.color }}>
                      {t(`tasks.runStatus.${run.status}` as any)}
                    </span>

                    <span className="text-xs text-[var(--color-text-tertiary)]">
                      {new Date(run.startedAt).toLocaleString()}
                    </span>

                    {run.durationMs != null && (
                      <span className="text-xs text-[var(--color-text-tertiary)]">
                        {t('tasks.duration', { s: Math.round(run.durationMs / 1000) })}
                      </span>
                    )}

                    <div className="ml-auto flex items-center gap-2">
                      {run.sessionId && run.status !== 'running' && (
                        <button
                          onClick={() => openSession(run.sessionId!, run.taskName)}
                          className="inline-flex items-center gap-1 rounded-lg bg-[var(--color-brand)]/8 px-2 py-1 text-xs font-medium text-[var(--color-brand)] transition-colors hover:bg-[var(--color-brand)]/15"
                        >
                          <span className="material-symbols-outlined text-[14px]">open_in_new</span>
                          {t('tasks.openSession')}
                        </button>
                      )}

                      {(run.output || run.error) && (
                        <button
                          onClick={() => setExpandedId(isExpanded ? null : run.id)}
                          className="text-xs text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-text-secondary)]"
                        >
                          {isExpanded ? t('tasks.hideOutput') : t('tasks.viewOutput')}
                        </button>
                      )}
                    </div>
                  </div>

                  {isExpanded && (
                    <RunOutput run={run} />
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
