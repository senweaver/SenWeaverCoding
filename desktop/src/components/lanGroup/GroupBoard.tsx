// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { confirmDialog } from '../../lib/dialogs'
import { useLanGroupStore } from '../../stores/lanGroupStore'
import type { LanGroupSnapshot, LanTask, TaskInputPayload } from '../../types/lanGroup'
import {
  canContribute,
  phaseLabel,
  priorityColor,
  PRIORITY_LABEL,
  PRIORITY_ORDER,
  statusColor,
  TASK_STATUS_LABEL,
  TASK_STATUS_ORDER,
} from './shared'

type EditorState = {
  task: LanTask | null
  phaseId: string
}

export function GroupBoard({
  groupId,
  snapshot,
}: {
  groupId: string
  snapshot: LanGroupSnapshot
}) {
  const t = useTranslation()
  const upsertTask = useLanGroupStore((s) => s.upsertTask)
  const removeTask = useLanGroupStore((s) => s.removeTask)
  const [editor, setEditor] = useState<EditorState | null>(null)
  const editable = canContribute(snapshot.group.role)

  const columns = useMemo(() => {
    const known = new Set(snapshot.phases.map((p) => p.id))
    const cols = snapshot.phases.map((phase) => ({
      id: phase.id,
      label: phaseLabel(phase, t),
      color: phase.color,
      tasks: snapshot.tasks.filter((task) => task.phaseId === phase.id),
    }))
    const uncategorized = snapshot.tasks.filter((task) => !known.has(task.phaseId))
    if (uncategorized.length > 0) {
      cols.push({
        id: '',
        label: t('lanGroup.uncategorized'),
        color: 'var(--color-text-tertiary)',
        tasks: uncategorized,
      })
    }
    return cols
  }, [snapshot, t])

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="flex-1 overflow-x-auto overflow-y-hidden p-3">
        <div className="flex h-full gap-3">
          {columns.map((col) => (
            <div
              key={col.id || 'uncat'}
              className="flex h-full w-56 shrink-0 flex-col rounded-lg bg-[var(--color-surface-hover)]"
            >
              <div className="flex items-center justify-between gap-1 px-2.5 py-2">
                <span className="flex items-center gap-1.5 truncate text-xs font-semibold text-[var(--color-text-primary)]">
                  <span
                    className="h-2 w-2 shrink-0 rounded-full"
                    style={{ background: col.color || 'var(--color-text-tertiary)' }}
                  />
                  {col.label}
                  <span className="text-[10px] font-normal text-[var(--color-text-tertiary)]">
                    {col.tasks.length}
                  </span>
                </span>
                {editable && (
                  <button
                    type="button"
                    onClick={() => setEditor({ task: null, phaseId: col.id })}
                    title={t('lanGroup.addTask')}
                    className="inline-flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-selected)] hover:text-[var(--color-text-primary)]"
                  >
                    <span className="material-symbols-outlined text-[15px]">add</span>
                  </button>
                )}
              </div>
              <div className="flex-1 space-y-2 overflow-y-auto px-2 pb-2">
                {col.tasks.map((task) => (
                  <button
                    type="button"
                    key={task.id}
                    onClick={() => editable && setEditor({ task, phaseId: task.phaseId })}
                    className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-2 text-left transition-colors hover:border-[var(--color-border-focus)]"
                  >
                    <div className="flex items-start gap-1">
                      {task.kind === 'milestone' && (
                        <span className="material-symbols-outlined text-[14px] text-[#f59e0b]">
                          flag
                        </span>
                      )}
                      <span className="flex-1 text-xs font-medium text-[var(--color-text-primary)]">
                        {task.title}
                      </span>
                      <span
                        className="mt-0.5 h-2 w-2 shrink-0 rounded-full"
                        style={{ background: priorityColor(task.priority) }}
                        title={t(PRIORITY_LABEL[task.priority] ?? 'lanGroup.priorityMedium')}
                      />
                    </div>
                    <div className="mt-1.5 flex items-center justify-between gap-1">
                      <span
                        className="rounded px-1 text-[9px] font-semibold text-white"
                        style={{ background: statusColor(task.status) }}
                      >
                        {t(TASK_STATUS_LABEL[task.status] ?? 'lanGroup.statusTodo')}
                      </span>
                      {task.assigneeNickname && (
                        <span className="truncate text-[9px] text-[var(--color-text-tertiary)]">
                          {task.assigneeNickname}
                        </span>
                      )}
                    </div>
                    {task.progress > 0 && task.kind !== 'milestone' && (
                      <div className="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
                        <div
                          className="h-full rounded-full bg-[var(--color-brand)]"
                          style={{ width: `${task.progress}%` }}
                        />
                      </div>
                    )}
                  </button>
                ))}
                {col.tasks.length === 0 && (
                  <div className="px-1 py-3 text-center text-[10px] text-[var(--color-text-tertiary)]">
                    {t('lanGroup.noTasks')}
                  </div>
                )}
              </div>
            </div>
          ))}
        </div>
      </div>

      {editor && (
        <TaskEditor
          groupId={groupId}
          snapshot={snapshot}
          task={editor.task}
          phaseId={editor.phaseId}
          onClose={() => setEditor(null)}
          onSave={async (payload) => {
            await upsertTask(groupId, payload)
            setEditor(null)
          }}
          onDelete={
            editor.task
              ? async () => {
                  if (await confirmDialog(t('lanGroup.deleteTask'))) {
                    await removeTask(groupId, editor.task!.id)
                    setEditor(null)
                  }
                }
              : undefined
          }
        />
      )}
    </div>
  )
}

function TaskEditor({
  snapshot,
  task,
  phaseId,
  onClose,
  onSave,
  onDelete,
}: {
  groupId: string
  snapshot: LanGroupSnapshot
  task: LanTask | null
  phaseId: string
  onClose: () => void
  onSave: (payload: TaskInputPayload) => Promise<void>
  onDelete?: () => Promise<void>
}) {
  const t = useTranslation()
  const [title, setTitle] = useState(task?.title ?? '')
  const [description, setDescription] = useState(task?.description ?? '')
  const [selectedPhase, setSelectedPhase] = useState(task?.phaseId ?? phaseId)
  const [assignee, setAssignee] = useState(task?.assignee ?? '')
  const [status, setStatus] = useState(task?.status ?? 'todo')
  const [priority, setPriority] = useState(task?.priority ?? 'medium')
  const [kind, setKind] = useState(task?.kind ?? 'task')
  const [progress, setProgress] = useState(task?.progress ?? 0)
  const [due, setDue] = useState(
    task?.dueMs ? new Date(task.dueMs).toISOString().slice(0, 10) : '',
  )

  async function submit() {
    const trimmed = title.trim()
    if (!trimmed) return
    const dueMs = due ? new Date(due).getTime() : 0
    await onSave({
      taskId: task?.id,
      title: trimmed,
      description,
      phaseId: selectedPhase,
      assignee,
      status,
      priority,
      kind,
      progress,
      dueMs: Number.isNaN(dueMs) ? 0 : dueMs,
      deps: task?.deps ?? [],
      parent: task?.parent ?? '',
    })
  }

  return (
    <div className="absolute inset-0 z-10 flex items-center justify-center bg-black/30 p-4">
      <div className="flex w-full max-w-md flex-col gap-2.5 rounded-[var(--radius-xl)] border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-[var(--shadow-dropdown)]">
        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder={t('lanGroup.taskTitle')}
          className="h-9 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
        />
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          placeholder={t('lanGroup.taskDescription')}
          rows={2}
          className="resize-none rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
        />
        <div className="grid grid-cols-2 gap-2">
          <label className="flex flex-col gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
            {t('lanGroup.selectPhase')}
            <select
              value={selectedPhase}
              onChange={(e) => setSelectedPhase(e.target.value)}
              className="h-8 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)]"
            >
              <option value="">{t('lanGroup.uncategorized')}</option>
              {snapshot.phases.map((p) => (
                <option key={p.id} value={p.id}>
                  {phaseLabel(p, t)}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
            {t('lanGroup.assignee')}
            <select
              value={assignee}
              onChange={(e) => setAssignee(e.target.value)}
              className="h-8 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)]"
            >
              <option value="">{t('lanGroup.unassigned')}</option>
              {snapshot.members.map((m) => (
                <option key={m.userId} value={m.userId}>
                  {m.nickname}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
            {t('lanGroup.status')}
            <select
              value={status}
              onChange={(e) => setStatus(e.target.value)}
              className="h-8 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)]"
            >
              {TASK_STATUS_ORDER.map((s) => (
                <option key={s} value={s}>
                  {t(TASK_STATUS_LABEL[s] ?? 'lanGroup.statusTodo')}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
            {t('lanGroup.priority')}
            <select
              value={priority}
              onChange={(e) => setPriority(e.target.value)}
              className="h-8 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)]"
            >
              {PRIORITY_ORDER.map((p) => (
                <option key={p} value={p}>
                  {t(PRIORITY_LABEL[p] ?? 'lanGroup.priorityMedium')}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
            {t('lanGroup.kindTask')} / {t('lanGroup.kindMilestone')}
            <select
              value={kind}
              onChange={(e) => setKind(e.target.value)}
              className="h-8 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)]"
            >
              <option value="task">{t('lanGroup.kindTask')}</option>
              <option value="milestone">{t('lanGroup.kindMilestone')}</option>
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
            {t('lanGroup.dueDate')}
            <input
              type="date"
              value={due}
              onChange={(e) => setDue(e.target.value)}
              className="h-8 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)]"
            />
          </label>
        </div>
        {kind !== 'milestone' && (
          <label className="flex items-center gap-2 text-[10px] text-[var(--color-text-tertiary)]">
            {t('lanGroup.progress')} {progress}%
            <input
              type="range"
              min={0}
              max={100}
              value={progress}
              onChange={(e) => setProgress(Number(e.target.value))}
              className="flex-1"
            />
          </label>
        )}
        <div className="mt-1 flex items-center justify-between">
          {onDelete ? (
            <button
              type="button"
              onClick={() => void onDelete()}
              className="rounded-md px-2 py-1 text-xs text-[var(--color-error)] hover:bg-[var(--color-surface-hover)]"
            >
              {t('common.delete')}
            </button>
          ) : (
            <span />
          )}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded-md px-2.5 py-1 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
            >
              {t('lanGroup.cancel')}
            </button>
            <button
              type="button"
              onClick={() => void submit()}
              disabled={!title.trim()}
              className="rounded-md bg-[var(--color-brand)] px-3 py-1 text-xs font-semibold text-white hover:opacity-90 disabled:opacity-40"
            >
              {t('lanGroup.confirm')}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
