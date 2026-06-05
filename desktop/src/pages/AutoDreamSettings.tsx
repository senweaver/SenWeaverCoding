// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'

import {
  autoDreamApi,
  type DreamPriority,
  type DreamTask,
  type DreamTaskInput,
  type DreamTrigger,
} from '../api/autoDream'
import { Button } from '../components/shared/Button'
import { ConfirmDialog } from '../components/shared/ConfirmDialog'
import { Input } from '../components/shared/Input'
import { Modal } from '../components/shared/Modal'
import { useTranslation } from '../i18n'
import { useUIStore } from '../stores/uiStore'

function triggerSummary(trigger: DreamTrigger, t: ReturnType<typeof useTranslation>): string {
  switch (trigger.type) {
    case 'idle':
      return `${t('settings.autoDream.form.triggerIdle')} · ${Math.round(trigger.afterIdleMs / 1000)}s`
    case 'interval':
      return `${t('settings.autoDream.form.triggerInterval')} · ${Math.round(trigger.everyMs / 1000)}s`
    case 'once':
      return `${t('settings.autoDream.form.triggerOnce')} · ${new Date(trigger.atMs).toLocaleString()}`
    case 'on_session_end':
      return t('settings.autoDream.form.triggerSessionEnd')
  }
}

function toDateTimeLocal(ms: number): string {
  const d = new Date(ms)
  if (Number.isNaN(d.getTime())) return ''
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
}

export function AutoDreamSettings() {
  const t = useTranslation()
  const addToast = useUIStore((s) => s.addToast)

  const [enabled, setEnabled] = useState(false)
  const [tasks, setTasks] = useState<DreamTask[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [isToggling, setIsToggling] = useState(false)
  const [showForm, setShowForm] = useState(false)
  const [editing, setEditing] = useState<DreamTask | null>(null)
  const [pendingDelete, setPendingDelete] = useState<DreamTask | null>(null)
  const [isDeleting, setIsDeleting] = useState(false)

  const reload = async () => {
    setIsLoading(true)
    try {
      const state = await autoDreamApi.get()
      setEnabled(state.enabled)
      setTasks(state.tasks)
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.autoDream.loadFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    } finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    void reload()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const toggleEnabled = async (next: boolean) => {
    setIsToggling(true)
    try {
      const state = await autoDreamApi.setEnabled(next)
      setEnabled(state.enabled)
      setTasks(state.tasks)
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.autoDream.saveFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    } finally {
      setIsToggling(false)
    }
  }

  const submitTask = async (input: DreamTaskInput) => {
    if (editing) {
      await autoDreamApi.updateTask(editing.id, input)
    } else {
      await autoDreamApi.createTask(input)
    }
    addToast({ type: 'success', message: t('settings.autoDream.savedToast') })
    setShowForm(false)
    setEditing(null)
    await reload()
  }

  const confirmDelete = async () => {
    if (!pendingDelete) return
    setIsDeleting(true)
    try {
      await autoDreamApi.removeTask(pendingDelete.id)
      setPendingDelete(null)
      await reload()
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.autoDream.saveFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    } finally {
      setIsDeleting(false)
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h2 className="text-lg font-semibold">{t('settings.autoDream.title')}</h2>
        <p className="mt-1 text-sm text-[var(--color-text-secondary)]">
          {t('settings.autoDream.description')}
        </p>
      </div>

      <label className="flex items-center gap-3">
        <input
          type="checkbox"
          checked={enabled}
          disabled={isToggling || isLoading}
          onChange={(e) => void toggleEnabled(e.target.checked)}
        />
        <span className="text-sm font-medium">{t('settings.autoDream.enableLabel')}</span>
      </label>
      <p className="-mt-4 text-xs text-[var(--color-text-secondary)]">
        {t('settings.autoDream.enableHint')}
      </p>

      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold">{t('settings.autoDream.tasksTitle')}</h3>
        <Button
          size="sm"
          onClick={() => {
            setEditing(null)
            setShowForm(true)
          }}
        >
          {t('settings.autoDream.addTask')}
        </Button>
      </div>

      {isLoading ? (
        <div className="text-sm text-[var(--color-text-secondary)]">{t('common.loading')}</div>
      ) : tasks.length === 0 ? (
        <div className="rounded-md border border-[var(--color-border)] p-4 text-sm text-[var(--color-text-secondary)]">
          {t('settings.autoDream.empty')}
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {tasks.map((task) => (
            <div
              key={task.id}
              className="flex items-start justify-between gap-3 rounded-md border border-[var(--color-border)] p-3"
            >
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-medium">{task.prompt}</div>
                <div className="mt-1 flex flex-wrap gap-2 text-xs text-[var(--color-text-secondary)]">
                  <span>{triggerSummary(task.trigger, t)}</span>
                  <span>·</span>
                  <span>{priorityLabel(task.priority, t)}</span>
                  <span>·</span>
                  <span>
                    {task.enabled
                      ? t('settings.autoDream.enabledBadge')
                      : t('settings.autoDream.disabledBadge')}
                  </span>
                  {task.runCount > 0 ? (
                    <>
                      <span>·</span>
                      <span>{t('settings.autoDream.runCount', { count: task.runCount })}</span>
                    </>
                  ) : null}
                </div>
              </div>
              <div className="flex shrink-0 gap-2">
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={() => {
                    setEditing(task)
                    setShowForm(true)
                  }}
                >
                  {t('common.edit')}
                </Button>
                <Button
                  variant="danger"
                  size="sm"
                  onClick={() => setPendingDelete(task)}
                >
                  {t('common.delete')}
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {showForm ? (
        <TaskFormModal
          existing={editing}
          onClose={() => {
            setShowForm(false)
            setEditing(null)
          }}
          onSubmit={submitTask}
        />
      ) : null}

      <ConfirmDialog
        open={pendingDelete !== null}
        onClose={() => setPendingDelete(null)}
        onConfirm={confirmDelete}
        title={t('settings.autoDream.deleteTitle')}
        body={t('settings.autoDream.deleteBody')}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        loading={isDeleting}
      />
    </div>
  )
}

function priorityLabel(
  priority: DreamPriority,
  t: ReturnType<typeof useTranslation>,
): string {
  switch (priority) {
    case 'low':
      return t('settings.autoDream.form.priorityLow')
    case 'normal':
      return t('settings.autoDream.form.priorityNormal')
    case 'high':
      return t('settings.autoDream.form.priorityHigh')
  }
}

type FormProps = {
  existing: DreamTask | null
  onClose: () => void
  onSubmit: (input: DreamTaskInput) => Promise<void>
}

function TaskFormModal({ existing, onClose, onSubmit }: FormProps) {
  const t = useTranslation()

  const [prompt, setPrompt] = useState(existing?.prompt ?? '')
  const [priority, setPriority] = useState<DreamPriority>(existing?.priority ?? 'normal')
  const [triggerType, setTriggerType] = useState<DreamTrigger['type']>(
    existing?.trigger.type ?? 'idle',
  )
  const [afterIdleSec, setAfterIdleSec] = useState(
    existing?.trigger.type === 'idle' ? String(Math.round(existing.trigger.afterIdleMs / 1000)) : '300',
  )
  const [everySec, setEverySec] = useState(
    existing?.trigger.type === 'interval' ? String(Math.round(existing.trigger.everyMs / 1000)) : '3600',
  )
  const [atTime, setAtTime] = useState(
    existing?.trigger.type === 'once' ? toDateTimeLocal(existing.trigger.atMs) : '',
  )
  const [maxDurationSec, setMaxDurationSec] = useState(
    existing ? String(Math.round(existing.maxDurationMs / 1000)) : '120',
  )
  const [allowedToolsText, setAllowedToolsText] = useState(
    existing ? existing.allowedTools.join(', ') : '',
  )
  const [taskEnabled, setTaskEnabled] = useState(existing?.enabled ?? true)
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const buildTrigger = (): DreamTrigger | null => {
    switch (triggerType) {
      case 'idle':
        return { type: 'idle', afterIdleMs: Math.max(0, Number(afterIdleSec) || 0) * 1000 }
      case 'interval':
        return { type: 'interval', everyMs: Math.max(1, Number(everySec) || 0) * 1000 }
      case 'once': {
        const ms = Date.parse(atTime)
        if (Number.isNaN(ms)) return null
        return { type: 'once', atMs: ms }
      }
      case 'on_session_end':
        return { type: 'on_session_end' }
    }
  }

  const handleSubmit = async () => {
    setError(null)
    if (!prompt.trim()) {
      setError(t('settings.autoDream.form.promptRequired'))
      return
    }
    const trigger = buildTrigger()
    if (!trigger) {
      setError(t('settings.autoDream.form.timeRequired'))
      return
    }
    const input: DreamTaskInput = {
      prompt: prompt.trim(),
      priority,
      trigger,
      maxDurationMs: Math.max(1, Number(maxDurationSec) || 0) * 1000,
      allowedTools: allowedToolsText
        .split(',')
        .map((s) => s.trim())
        .filter(Boolean),
      enabled: taskEnabled,
    }
    setSubmitting(true)
    try {
      await onSubmit(input)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <Modal
      open
      onClose={onClose}
      title={
        existing
          ? t('settings.autoDream.form.editTitle')
          : t('settings.autoDream.form.createTitle')
      }
      width={560}
      footer={
        <>
          <Button variant="secondary" onClick={onClose}>
            {t('common.cancel')}
          </Button>
          <Button onClick={() => void handleSubmit()} disabled={submitting} loading={submitting}>
            {t('common.save')}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-4">
        <div>
          <label className="mb-1 block text-sm font-medium">
            {t('settings.autoDream.form.prompt')}
          </label>
          <textarea
            className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] p-2 text-sm"
            rows={4}
            value={prompt}
            placeholder={t('settings.autoDream.form.promptPlaceholder')}
            onChange={(e) => setPrompt(e.target.value)}
          />
        </div>

        <div className="grid grid-cols-2 gap-4">
          <div>
            <label className="mb-1 block text-sm font-medium">
              {t('settings.autoDream.form.trigger')}
            </label>
            <select
              className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] p-2 text-sm"
              value={triggerType}
              onChange={(e) => setTriggerType(e.target.value as DreamTrigger['type'])}
            >
              <option value="idle">{t('settings.autoDream.form.triggerIdle')}</option>
              <option value="interval">{t('settings.autoDream.form.triggerInterval')}</option>
              <option value="once">{t('settings.autoDream.form.triggerOnce')}</option>
              <option value="on_session_end">
                {t('settings.autoDream.form.triggerSessionEnd')}
              </option>
            </select>
          </div>
          <div>
            <label className="mb-1 block text-sm font-medium">
              {t('settings.autoDream.form.priority')}
            </label>
            <select
              className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] p-2 text-sm"
              value={priority}
              onChange={(e) => setPriority(e.target.value as DreamPriority)}
            >
              <option value="low">{t('settings.autoDream.form.priorityLow')}</option>
              <option value="normal">{t('settings.autoDream.form.priorityNormal')}</option>
              <option value="high">{t('settings.autoDream.form.priorityHigh')}</option>
            </select>
          </div>
        </div>

        {triggerType === 'idle' ? (
          <Input
            label={t('settings.autoDream.form.afterIdleSec')}
            type="number"
            min={0}
            value={afterIdleSec}
            onChange={(e) => setAfterIdleSec(e.target.value)}
          />
        ) : null}
        {triggerType === 'interval' ? (
          <Input
            label={t('settings.autoDream.form.everySec')}
            type="number"
            min={1}
            value={everySec}
            onChange={(e) => setEverySec(e.target.value)}
          />
        ) : null}
        {triggerType === 'once' ? (
          <Input
            label={t('settings.autoDream.form.atTime')}
            type="datetime-local"
            value={atTime}
            onChange={(e) => setAtTime(e.target.value)}
          />
        ) : null}

        <Input
          label={t('settings.autoDream.form.maxDurationSec')}
          type="number"
          min={1}
          value={maxDurationSec}
          onChange={(e) => setMaxDurationSec(e.target.value)}
        />

        <Input
          label={t('settings.autoDream.form.allowedTools')}
          value={allowedToolsText}
          placeholder="read_file, list_dir, code_search"
          onChange={(e) => setAllowedToolsText(e.target.value)}
        />
        <p className="-mt-2 text-xs text-[var(--color-text-secondary)]">
          {t('settings.autoDream.form.allowedToolsHint')}
        </p>

        <label className="flex items-center gap-2">
          <input
            type="checkbox"
            checked={taskEnabled}
            onChange={(e) => setTaskEnabled(e.target.checked)}
          />
          <span className="text-sm">{t('settings.autoDream.form.enabled')}</span>
        </label>

        {error ? (
          <div className="text-sm text-[var(--color-error)]">{error}</div>
        ) : null}
      </div>
    </Modal>
  )
}
