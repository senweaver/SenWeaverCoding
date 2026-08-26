// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState, useRef } from 'react'
import type { RefObject } from 'react'
import { createPortal } from 'react-dom'
import type { CronTask } from '../../types/task'
import { useTaskStore } from '../../stores/taskStore'
import { useTranslation } from '../../i18n'
import { useAnchoredDropdown } from '../../hooks/useAnchoredDropdown'
import { describeTrigger } from '../../lib/cronDescribe'
import { Button } from '../shared/Button'
import { TaskRunsPanel } from './TaskRunsPanel'
import { NewTaskModal } from './NewTaskModal'

type Props = {
  task: CronTask
  showLogs: boolean
  onToggleLogs: () => void
}

type ConfirmAction = 'run' | 'toggle' | 'delete' | null

export function TaskRow({ task, showLogs, onToggleLogs }: Props) {
  const deleteTask = useTaskStore((s) => s.deleteTask)
  const updateTask = useTaskStore((s) => s.updateTask)
  const runTask = useTaskStore((s) => s.runTask)
  const t = useTranslation()
  const [showEdit, setShowEdit] = useState(false)
  const [showMenu, setShowMenu] = useState(false)
  const [isRunning, setIsRunning] = useState(false)
  const [confirmAction, setConfirmAction] = useState<ConfirmAction>(null)
  const [logsRefreshKey, setLogsRefreshKey] = useState(0)
  const runBtnRef = useRef<HTMLButtonElement>(null)
  const menu = useAnchoredDropdown<HTMLButtonElement>(
    showMenu && !confirmAction,
    () => setShowMenu(false),
    { align: 'right', estimatedHeight: 150 },
  )

  const handleRunNow = async () => {
    setConfirmAction(null)
    setIsRunning(true)
    if (!showLogs) onToggleLogs()
    try {
      await runTask(task.id)
      setLogsRefreshKey((k) => k + 1)
    } catch (err) {
      console.error('Failed to run task:', err)
    } finally {
      setIsRunning(false)
    }
  }

  const handleToggle = () => {
    setConfirmAction(null)
    setShowMenu(false)
    updateTask(task.id, { enabled: !task.enabled })
  }

  const handleDelete = () => {
    setConfirmAction(null)
    setShowMenu(false)
    deleteTask(task.id)
  }

  const iconBtn = 'p-1.5 rounded-lg transition-colors'
  const menuItem = 'flex items-center gap-2.5 w-full px-3 py-2 text-xs text-left rounded-lg transition-colors'

  return (
    <div>
      <div className="flex items-center justify-between px-3 py-2 transition-colors hover:bg-[var(--color-surface-hover)] group">
        <div className="flex items-center gap-3 min-w-0 flex-1">
          <span className={`w-2 h-2 rounded-full flex-shrink-0 ${task.enabled ? 'bg-[var(--color-success)]' : 'bg-[var(--color-text-tertiary)]'}`} />
          <div className="min-w-0">
            <div className="truncate text-xs font-semibold text-[var(--color-text-primary)]">{task.name}</div>
            {task.description && (
              <div className="truncate text-xs text-[var(--color-text-tertiary)]">{task.description}</div>
            )}
            <div className="flex items-center gap-3 text-xs text-[var(--color-text-tertiary)] mt-0.5">
              <span>{t('tasks.createdAt')}{new Date(task.createdAt).toLocaleDateString()}</span>
              {task.lastFiredAt && (
                <span>{t('tasks.lastRunAt')}{new Date(task.lastFiredAt).toLocaleDateString()}</span>
              )}
            </div>
          </div>
        </div>

        <div className="flex min-w-0 shrink-0 items-center gap-3">
          <span className="max-w-[200px] truncate text-xs text-[var(--color-text-tertiary)]" title={describeTrigger(task, t)}>
            {describeTrigger(task, t)}
          </span>

          <div className="flex items-center gap-0.5">
            <div className="relative">
              <button
                ref={runBtnRef}
                onClick={() => isRunning || !task.enabled ? undefined : setConfirmAction(confirmAction === 'run' ? null : 'run')}
                disabled={isRunning || !task.enabled}
                className={`${iconBtn} ${task.enabled ? 'text-[var(--color-brand)] hover:bg-[var(--color-surface-selected)]' : 'text-[var(--color-text-tertiary)] cursor-not-allowed'} disabled:opacity-50`}
                title={task.enabled ? t('tasks.runNow') : undefined}
              >
                <span className={`material-symbols-outlined text-[16px] ${isRunning ? 'animate-spin' : ''}`}>
                  {isRunning ? 'sync' : 'play_arrow'}
                </span>
              </button>
              {confirmAction === 'run' && (
                <ConfirmPopover
                  anchorRef={runBtnRef}
                  message={t('tasks.confirmRun')}
                  confirmLabel={t('tasks.runNow')}
                  onConfirm={handleRunNow}
                  onCancel={() => setConfirmAction(null)}
                  cancelLabel={t('common.cancel')}
                />
              )}
            </div>

            <button
              onClick={onToggleLogs}
              className={`${iconBtn} ${showLogs ? 'text-[var(--color-brand)] bg-[var(--color-surface-selected)]' : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-selected)]'}`}
              title={t('tasks.viewLogs')}
            >
              <span className="material-symbols-outlined text-[16px]">receipt_long</span>
            </button>

            <div className="relative">
              <button
                ref={menu.triggerRef}
                onClick={() => { setShowMenu(!showMenu); setConfirmAction(null) }}
                className={`${iconBtn} text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-selected)]`}
              >
                <span className="material-symbols-outlined text-[16px]">more_vert</span>
              </button>

              {showMenu && !confirmAction && menu.style && createPortal(
                <div
                  ref={menu.menuRef}
                  style={menu.style}
                  className="w-44 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg py-1"
                >
                  <button
                    onClick={() => { setShowMenu(false); setShowEdit(true) }}
                    className={`${menuItem} text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]`}
                  >
                    <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)]">edit</span>
                    {t('tasks.edit')}
                  </button>

                  <button
                    onClick={() => setConfirmAction('toggle')}
                    className={`${menuItem} text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]`}
                  >
                    <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)]">
                      {task.enabled ? 'pause_circle' : 'play_circle'}
                    </span>
                    {task.enabled ? t('common.disable') : t('common.enable')}
                  </button>

                  <div className="my-1 h-px bg-[var(--color-border)]" />

                  <button
                    onClick={() => setConfirmAction('delete')}
                    className={`${menuItem} text-[var(--color-error)] hover:bg-[var(--color-error-container)]/18`}
                  >
                    <span className="material-symbols-outlined text-[16px]">delete</span>
                    {t('common.delete')}
                  </button>
                </div>,
                menu.portalTarget,
              )}

              {confirmAction === 'toggle' && (
                <ConfirmPopover
                  anchorRef={menu.triggerRef}
                  message={task.enabled ? t('tasks.confirmDisable') : t('tasks.confirmEnable')}
                  confirmLabel={task.enabled ? t('common.disable') : t('common.enable')}
                  onConfirm={handleToggle}
                  onCancel={() => { setConfirmAction(null); setShowMenu(false) }}
                  cancelLabel={t('common.cancel')}
                />
              )}
              {confirmAction === 'delete' && (
                <ConfirmPopover
                  anchorRef={menu.triggerRef}
                  message={t('tasks.confirmDelete')}
                  confirmLabel={t('common.delete')}
                  onConfirm={handleDelete}
                  onCancel={() => { setConfirmAction(null); setShowMenu(false) }}
                  cancelLabel={t('common.cancel')}
                  variant="error"
                />
              )}
            </div>
          </div>
        </div>
      </div>

      {showLogs && (
        <div className="px-3 pb-3">
          <TaskRunsPanel taskId={task.id} onClose={onToggleLogs} refreshKey={logsRefreshKey} />
        </div>
      )}

      {showEdit && (
        <NewTaskModal open editTask={task} onClose={() => setShowEdit(false)} />
      )}
    </div>
  )
}

function ConfirmPopover({ anchorRef, message, confirmLabel, onConfirm, onCancel, cancelLabel, variant = 'brand' }: {
  anchorRef: RefObject<HTMLElement | null>
  message: string
  confirmLabel: string
  onConfirm: () => void
  onCancel: () => void
  cancelLabel: string
  variant?: 'brand' | 'error'
}) {
  const { menuRef, style, portalTarget } = useAnchoredDropdown(true, onCancel, {
    anchorRef,
    align: 'right',
    estimatedHeight: 120,
    gap: 6,
  })
  if (!style) return null
  return createPortal(
    <div
      ref={menuRef}
      style={style}
      className="w-52 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg p-3"
    >
      <p className="mb-2.5 text-xs text-[var(--color-text-tertiary)]">{message}</p>
      <div className="flex justify-end gap-1.5">
        <Button size="sm" variant="secondary" onClick={onCancel}>{cancelLabel}</Button>
        <Button size="sm" variant={variant === 'error' ? 'danger' : 'primary'} onClick={onConfirm}>
          {confirmLabel}
        </Button>
      </div>
    </div>,
    portalTarget,
  )
}
