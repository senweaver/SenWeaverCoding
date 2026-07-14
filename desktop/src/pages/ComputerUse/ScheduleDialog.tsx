// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import { useTranslation } from '../../i18n'
import {
  createComputerScheduledTask,
  type ComputerScheduleTrigger,
} from '../../api/computer'
import { useComputerUseStore } from '../../stores/computerUseStore'

type ScheduleDialogProps = {
  name: string
  onClose: () => void
}

type TriggerKind = 'cron' | 'interval' | 'once'

export function ScheduleDialog({ name, onClose }: ScheduleDialogProps) {
  const t = useTranslation()

  const provider = useComputerUseStore((s) => s.provider)
  const model = useComputerUseStore((s) => s.model)
  const hasModel = Boolean(provider && model)

  const [taskName, setTaskName] = useState(() => `computer: ${name}`)
  const [trigger, setTrigger] = useState<TriggerKind>('cron')
  const [cronExpr, setCronExpr] = useState('0 9 * * *')
  const [everyMinutes, setEveryMinutes] = useState(60)
  const [runAt, setRunAt] = useState(() => {
    const d = new Date(Date.now() + 10 * 60 * 1000)
    const pad = (n: number) => String(n).padStart(2, '0')
    return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`
  })
  const [smart, setSmart] = useState(false)
  const [loopCount, setLoopCount] = useState(1)
  const [intervalSec, setIntervalSec] = useState(0)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleCreate = async () => {
    if (saving) return
    setError(null)
    let wireTrigger: ComputerScheduleTrigger
    if (trigger === 'cron') {
      if (!cronExpr.trim()) {
        setError(t('computerUse.schedule.cronRequired'))
        return
      }
      wireTrigger = { triggerType: 'cron', cron: cronExpr.trim() }
    } else if (trigger === 'interval') {
      const ms = Math.max(1, Math.round(everyMinutes)) * 60_000
      wireTrigger = { triggerType: 'interval', everyMs: ms }
    } else {
      const date = new Date(runAt)
      if (Number.isNaN(date.getTime()) || date.getTime() <= Date.now()) {
        setError(t('computerUse.schedule.runAtInvalid'))
        return
      }
      wireTrigger = { triggerType: 'once', runAt: date.toISOString() }
    }
    setSaving(true)
    try {
      await createComputerScheduledTask(taskName.trim() || `computer: ${name}`, wireTrigger, {
        mode: 'replay',
        recording: name,
        smart: smart && hasModel,
        ...(smart && hasModel && provider ? { provider } : {}),
        ...(smart && hasModel && model ? { model } : {}),
        ...(loopCount > 1 ? { loop_count: Math.min(100, loopCount) } : {}),
        ...(intervalSec > 0 ? { interval_ms: Math.min(3600, intervalSec) * 1000 } : {}),
      })
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  const triggerOptions: Array<{ id: TriggerKind; label: string; icon: string }> = [
    { id: 'cron', label: t('computerUse.schedule.cron'), icon: 'calendar_month' },
    { id: 'interval', label: t('computerUse.schedule.interval'), icon: 'timer' },
    { id: 'once', label: t('computerUse.schedule.once'), icon: 'event' },
  ]

  return (
    <div
      className="absolute inset-0 z-50 flex items-center justify-center bg-black/40"
      onClick={onClose}
    >
      <div
        className="flex w-[min(420px,92vw)] flex-col gap-3 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2">
          <span className="material-symbols-outlined text-[20px] text-[var(--color-brand)]">
            schedule
          </span>
          <div className="min-w-0">
            <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
              {t('computerUse.schedule.title')}
            </div>
            <div className="truncate text-[11px] text-[var(--color-text-secondary)]">{name}</div>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="ml-auto inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
            aria-label={t('computerUse.skills.close')}
          >
            <span className="material-symbols-outlined text-[16px]">close</span>
          </button>
        </div>

        <label className="flex flex-col gap-1 text-[11px] text-[var(--color-text-secondary)]">
          {t('computerUse.schedule.name')}
          <input
            type="text"
            value={taskName}
            onChange={(e) => setTaskName(e.target.value)}
            className="rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
          />
        </label>

        <div className="flex items-center gap-1 rounded-lg border border-[var(--color-border)] p-0.5">
          {triggerOptions.map((opt) => (
            <button
              key={opt.id}
              type="button"
              onClick={() => setTrigger(opt.id)}
              className={`flex flex-1 items-center justify-center gap-1 rounded-md px-2 py-1 text-[11px] font-medium transition-colors ${
                trigger === opt.id
                  ? 'bg-[var(--color-brand)] text-white'
                  : 'text-[var(--color-text-secondary)] hover:bg-black/[0.05] dark:hover:bg-white/[0.08]'
              }`}
            >
              <span className="material-symbols-outlined text-[13px]">{opt.icon}</span>
              {opt.label}
            </button>
          ))}
        </div>

        {trigger === 'cron' && (
          <label className="flex flex-col gap-1 text-[11px] text-[var(--color-text-secondary)]">
            {t('computerUse.schedule.cronExpr')}
            <input
              type="text"
              value={cronExpr}
              onChange={(e) => setCronExpr(e.target.value)}
              placeholder="0 9 * * *"
              className="rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 font-mono text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
            />
          </label>
        )}
        {trigger === 'interval' && (
          <label className="flex items-center gap-2 text-[11px] text-[var(--color-text-secondary)]">
            {t('computerUse.schedule.everyMinutes')}
            <input
              type="number"
              min={1}
              max={10080}
              value={everyMinutes}
              onChange={(e) =>
                setEveryMinutes(Math.min(10080, Math.max(1, Number(e.target.value) || 1)))
              }
              className="w-24 rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
            />
          </label>
        )}
        {trigger === 'once' && (
          <label className="flex flex-col gap-1 text-[11px] text-[var(--color-text-secondary)]">
            {t('computerUse.schedule.runAt')}
            <input
              type="datetime-local"
              value={runAt}
              onChange={(e) => setRunAt(e.target.value)}
              className="rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
            />
          </label>
        )}

        <div className="flex flex-wrap items-center gap-3">
          <label
            className={`flex items-center gap-1.5 text-[11px] ${
              hasModel
                ? 'text-[var(--color-text-primary)]'
                : 'text-[var(--color-text-tertiary)]'
            }`}
            title={t('computerUse.skills.smartReplayHint')}
          >
            <input
              type="checkbox"
              checked={smart && hasModel}
              disabled={!hasModel}
              onChange={(e) => setSmart(e.target.checked)}
              className="accent-[var(--color-brand)]"
            />
            {t('computerUse.schedule.smart')}
          </label>
          <label className="flex items-center gap-1.5 text-[11px] text-[var(--color-text-secondary)]">
            {t('computerUse.stepEditor.loopCount')}
            <input
              type="number"
              min={1}
              max={100}
              value={loopCount}
              onChange={(e) =>
                setLoopCount(Math.min(100, Math.max(1, Number(e.target.value) || 1)))
              }
              className="w-16 rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[12px] text-[var(--color-text-primary)] outline-none"
            />
          </label>
          <label className="flex items-center gap-1.5 text-[11px] text-[var(--color-text-secondary)]">
            {t('computerUse.stepEditor.loopInterval')}
            <input
              type="number"
              min={0}
              max={3600}
              value={intervalSec}
              onChange={(e) =>
                setIntervalSec(Math.min(3600, Math.max(0, Number(e.target.value) || 0)))
              }
              className="w-20 rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[12px] text-[var(--color-text-primary)] outline-none"
            />
          </label>
        </div>

        {error && (
          <div className="rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-[11px] text-red-600 dark:text-red-400">
            {error}
          </div>
        )}

        <p className="text-[10px] leading-snug text-[var(--color-text-tertiary)]">
          {t('computerUse.schedule.hint')}
        </p>

        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
          >
            {t('common.cancel')}
          </button>
          <button
            type="button"
            onClick={() => void handleCreate()}
            disabled={saving}
            className="inline-flex items-center gap-1.5 rounded-lg bg-[var(--color-brand)] px-3 py-1.5 text-[12px] font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            {saving && (
              <span className="material-symbols-outlined animate-spin text-[14px]">
                progress_activity
              </span>
            )}
            {t('computerUse.schedule.create')}
          </button>
        </div>
      </div>
    </div>
  )
}

export default ScheduleDialog
