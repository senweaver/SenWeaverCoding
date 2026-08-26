// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import {
  getRecordingSteps,
  saveRecordingSteps,
  type RecordedStepWire,
} from '../../api/computer'

type StepEditorProps = {
  name: string
  onClose: () => void
  onSaved?: () => void
}

const VALUE_ACTIONS = new Set(['type', 'key_press', 'scroll'])
const AMOUNT_ACTIONS = new Set(['scroll', 'wait'])

export function StepEditor({ name, onClose, onSaved }: StepEditorProps) {
  const t = useTranslation()

  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [steps, setSteps] = useState<RecordedStepWire[]>([])
  const [loopCount, setLoopCount] = useState(1)
  const [intervalSec, setIntervalSec] = useState(0)

  useEffect(() => {
    let disposed = false
    setLoading(true)
    setError(null)
    getRecordingSteps(name)
      .then((res) => {
        if (disposed) return
        setSteps(res.steps ?? [])
        setLoopCount(Math.max(1, res.run_config?.loop_count ?? 1))
        setIntervalSec(Math.round((res.run_config?.interval_ms ?? 0) / 1000))
      })
      .catch((err) => {
        if (disposed) return
        setError(err instanceof Error ? err.message : String(err))
      })
      .finally(() => {
        if (!disposed) setLoading(false)
      })
    return () => {
      disposed = true
    }
  }, [name])

  const updateStep = (idx: number, patch: Partial<RecordedStepWire>) => {
    setSteps((prev) => prev.map((s, i) => (i === idx ? { ...s, ...patch } : s)))
  }

  const removeStep = (idx: number) => {
    setSteps((prev) => prev.filter((_, i) => i !== idx))
  }

  const moveStep = (idx: number, delta: number) => {
    setSteps((prev) => {
      const next = [...prev]
      const target = idx + delta
      if (target < 0 || target >= next.length) return prev
      const [item] = next.splice(idx, 1)
      if (!item) return prev
      next.splice(target, 0, item)
      return next
    })
  }

  const handleSave = async () => {
    if (saving || steps.length === 0) return
    setSaving(true)
    setError(null)
    try {
      await saveRecordingSteps(
        name,
        steps.map((s, i) => ({ ...s, index: i })),
        { loopCount: Math.max(1, loopCount), intervalMs: Math.max(0, intervalSec) * 1000 },
      )
      onSaved?.()
      onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="absolute inset-0 z-40 flex justify-end bg-black/30" onClick={onClose}>
      <div
        className="flex h-full w-[min(520px,94vw)] flex-col border-l border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-4 py-3">
          <span className="material-symbols-outlined text-[20px] text-[var(--color-brand)]">
            edit_note
          </span>
          <div className="min-w-0">
            <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
              {t('computerUse.stepEditor.title')}
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

        <div className="flex shrink-0 flex-wrap items-center gap-3 border-b border-[var(--color-border)] px-4 py-2.5">
          <label className="flex items-center gap-1.5 text-[11px] text-[var(--color-text-secondary)]">
            {t('computerUse.stepEditor.loopCount')}
            <input
              type="number"
              min={1}
              max={100}
              value={loopCount}
              onChange={(e) => setLoopCount(Math.min(100, Math.max(1, Number(e.target.value) || 1)))}
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
              onChange={(e) => setIntervalSec(Math.min(3600, Math.max(0, Number(e.target.value) || 0)))}
              className="w-20 rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[12px] text-[var(--color-text-primary)] outline-none"
            />
          </label>
          <span className="text-[10px] text-[var(--color-text-tertiary)]">
            {t('computerUse.stepEditor.loopHint')}
          </span>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          {loading ? (
            <div className="flex items-center justify-center gap-2 py-10 text-[12px] text-[var(--color-text-secondary)]">
              <span className="material-symbols-outlined animate-spin text-[16px]">
                progress_activity
              </span>
              {t('common.loading')}
            </div>
          ) : (
            <ol className="flex flex-col gap-2">
              {steps.map((step, idx) => (
                <li
                  key={`${idx}-${step.action_type}-${step.screenshot_file ?? ''}`}
                  className="rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] p-2.5"
                >
                  <div className="flex items-center gap-2">
                    <span className="inline-flex w-8 shrink-0 items-center justify-center rounded-md bg-[var(--color-brand)]/12 px-1 py-0.5 text-[10px] font-semibold tabular-nums text-[var(--color-brand)]">
                      {idx + 1}
                    </span>
                    <span className="shrink-0 text-[11px] font-medium text-[var(--color-text-primary)]">
                      {step.action_type}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-[11px] text-[var(--color-text-secondary)]">
                      {step.element_description ?? ''}
                    </span>
                    <button
                      type="button"
                      onClick={() => moveStep(idx, -1)}
                      disabled={idx === 0}
                      className="inline-flex h-6 w-6 items-center justify-center rounded-md border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-black/[0.06] disabled:opacity-40 dark:hover:bg-white/[0.08]"
                      aria-label={t('computerUse.stepEditor.moveUp')}
                    >
                      <span className="material-symbols-outlined text-[14px]">arrow_upward</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => moveStep(idx, 1)}
                      disabled={idx === steps.length - 1}
                      className="inline-flex h-6 w-6 items-center justify-center rounded-md border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-black/[0.06] disabled:opacity-40 dark:hover:bg-white/[0.08]"
                      aria-label={t('computerUse.stepEditor.moveDown')}
                    >
                      <span className="material-symbols-outlined text-[14px]">arrow_downward</span>
                    </button>
                    <button
                      type="button"
                      onClick={() => removeStep(idx)}
                      className="inline-flex h-6 w-6 items-center justify-center rounded-md border border-[var(--color-border)] text-[var(--color-text-tertiary)] hover:border-red-500/50 hover:text-red-500"
                      aria-label={t('computerUse.skills.delete')}
                    >
                      <span className="material-symbols-outlined text-[14px]">delete</span>
                    </button>
                  </div>
                  <div className="mt-2 flex flex-wrap items-center gap-3">
                    <label className="flex items-center gap-1.5 text-[10px] text-[var(--color-text-secondary)]">
                      {t('computerUse.stepEditor.delay')}
                      <input
                        type="number"
                        min={0}
                        max={600000}
                        step={100}
                        value={step.delay_ms}
                        onChange={(e) =>
                          updateStep(idx, {
                            delay_ms: Math.min(600000, Math.max(0, Number(e.target.value) || 0)),
                          })
                        }
                        className="w-20 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5 text-[11px] text-[var(--color-text-primary)] outline-none"
                      />
                    </label>
                    {VALUE_ACTIONS.has(step.action_type) && (
                      <label className="flex min-w-0 flex-1 items-center gap-1.5 text-[10px] text-[var(--color-text-secondary)]">
                        {t('computerUse.stepEditor.value')}
                        <input
                          type="text"
                          value={step.value ?? ''}
                          onChange={(e) => updateStep(idx, { value: e.target.value })}
                          className="min-w-0 flex-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5 text-[11px] text-[var(--color-text-primary)] outline-none"
                        />
                      </label>
                    )}
                    {AMOUNT_ACTIONS.has(step.action_type) && (
                      <label className="flex items-center gap-1.5 text-[10px] text-[var(--color-text-secondary)]">
                        {t('computerUse.stepEditor.amount')}
                        <input
                          type="number"
                          value={step.amount ?? 0}
                          onChange={(e) =>
                            updateStep(idx, { amount: Number(e.target.value) || 0 })
                          }
                          className="w-20 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5 text-[11px] text-[var(--color-text-primary)] outline-none"
                        />
                      </label>
                    )}
                  </div>
                </li>
              ))}
            </ol>
          )}
        </div>

        {error && (
          <div className="mx-3 mb-2 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-[11px] text-red-600 dark:text-red-400">
            {error}
          </div>
        )}

        <div className="flex shrink-0 items-center gap-2 border-t border-[var(--color-border)] p-3">
          <span className="text-[10px] text-[var(--color-text-tertiary)]">
            {t('computerUse.stepEditor.count', { count: steps.length })}
          </span>
          <div className="ml-auto flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
            >
              {t('common.cancel')}
            </button>
            <button
              type="button"
              onClick={() => void handleSave()}
              disabled={saving || loading || steps.length === 0}
              className="inline-flex items-center gap-1.5 rounded-lg bg-[var(--color-brand)] px-3 py-1.5 text-[12px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90 disabled:opacity-50"
            >
              {saving && (
                <span className="material-symbols-outlined animate-spin text-[14px]">
                  progress_activity
                </span>
              )}
              {t('common.save')}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}

export default StepEditor
