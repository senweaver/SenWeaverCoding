// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../../../i18n'
import { useMinimalRecorderStore } from '../../../stores/minimalRecorderStore'
import { useMinimalComputerStore, isComputerBusy } from '../../../stores/minimalComputerStore'
import { useComputerUseStore } from '../../../stores/computerUseStore'
import {
  MINIMAL_EVENT_COMPUTER_REPLAY,
  MINIMAL_EVENT_RECORDER_CONTROL,
  emitMinimalEvent,
  type MinimalComputerReplay,
  type MinimalRecorderControl,
} from '../../../lib/minimalMode'

function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m}:${s.toString().padStart(2, '0')}`
}

export function RecorderPane() {
  const t = useTranslation()

  const status = useMinimalRecorderStore((s) => s.status)
  const error = useMinimalRecorderStore((s) => s.error)
  const statusMessage = useMinimalRecorderStore((s) => s.statusMessage)
  const stepCount = useMinimalRecorderStore((s) => s.stepCount)
  const lastActionType = useMinimalRecorderStore((s) => s.lastActionType)
  const lastActionValue = useMinimalRecorderStore((s) => s.lastActionValue)
  const savedRecordingName = useMinimalRecorderStore((s) => s.savedRecordingName)
  const savedSkillName = useMinimalRecorderStore((s) => s.savedSkillName)
  const startedAt = useMinimalRecorderStore((s) => s.startedAt)
  const narrationEnabled = useMinimalRecorderStore((s) => s.narrationEnabled)
  const narrationMuted = useMinimalRecorderStore((s) => s.narrationMuted)

  const computerStatus = useMinimalComputerStore((s) => s.status)
  const computerBusy = isComputerBusy(computerStatus) || computerStatus === 'call_user'

  const provider = useComputerUseStore((s) => s.provider)
  const model = useComputerUseStore((s) => s.model)
  const loadModels = useComputerUseStore((s) => s.loadModels)
  const hasModel = Boolean(provider && model)

  const [task, setTask] = useState('')
  const [now, setNow] = useState(() => Date.now())
  const [narrate, setNarrate] = useState(false)

  useEffect(() => {
    void loadModels()
  }, [loadModels])

  useEffect(() => {
    if (status !== 'recording' || startedAt === null) return
    const id = window.setInterval(() => setNow(Date.now()), 500)
    return () => window.clearInterval(id)
  }, [status, startedAt])

  const send = (control: MinimalRecorderControl) => {
    void emitMinimalEvent(MINIMAL_EVENT_RECORDER_CONTROL, control)
  }

  const executeSaved = () => {
    if (!savedRecordingName) return
    const payload: MinimalComputerReplay = hasModel
      ? {
          name: savedRecordingName,
          mode: 'smart',
          provider: provider ?? undefined,
          model: model ?? undefined,
        }
      : { name: savedRecordingName, mode: 'exact' }
    void emitMinimalEvent(MINIMAL_EVENT_COMPUTER_REPLAY, payload)
    send({ action: 'reset' })
  }

  const isRecording = status === 'recording'
  const isGenerating = status === 'generating'
  const elapsed = startedAt !== null ? formatElapsed(now - startedAt) : '0:00'

  return (
    <div
      className="flex flex-col gap-2 overflow-hidden rounded-2xl border border-white/50 bg-[var(--color-surface)]/95 p-2.5 shadow-[0_10px_40px_rgba(30,58,95,0.28)] backdrop-blur-md"
      data-minimal-computer-recorder
    >
      {status === 'idle' && (
        <>
          <textarea
            value={task}
            onChange={(e) => setTask(e.target.value)}
            rows={2}
            disabled={computerBusy}
            placeholder={t('computerUse.record.taskPlaceholder')}
            className="max-h-[96px] min-h-[44px] w-full resize-none rounded-xl border border-[var(--color-border)] bg-[var(--color-background)] px-3 py-2 text-[12px] text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] disabled:opacity-60"
          />
          <label className="flex items-center gap-1.5 text-[11px] text-[var(--color-text-secondary)]">
            <input
              type="checkbox"
              checked={narrate}
              onChange={(e) => setNarrate(e.target.checked)}
              className="h-3 w-3 accent-[var(--color-brand)]"
            />
            <span className="material-symbols-outlined text-[13px] text-[var(--color-brand)]">mic</span>
            {t('computerUse.narration.enable')}
          </label>
          <button
            type="button"
            onClick={() => send({ action: 'start', task: task.trim(), narrationEnabled: narrate })}
            disabled={computerBusy}
            title={computerBusy ? t('minimal.computer.record.busy') : undefined}
            className="inline-flex items-center justify-center gap-1.5 rounded-lg bg-red-500 px-3 py-1.5 text-[12px] font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            <span className="material-symbols-outlined text-[15px]">fiber_manual_record</span>
            {t('computerUse.record.start')}
          </button>
          <p className="text-[10px] leading-snug text-[var(--color-text-tertiary)]">
            {t('minimal.computer.record.hint')}
          </p>
          <p className="text-[10px] leading-snug text-amber-600 dark:text-amber-400">
            {t('computerUse.record.privacyHint')}
          </p>
        </>
      )}

      {isRecording && (
        <>
          <div className="flex items-center gap-2 text-[12px] text-[var(--color-text-primary)]">
            <span className="h-2 w-2 shrink-0 animate-pulse rounded-full bg-red-500" aria-hidden />
            <span className="font-semibold">{t('computerUse.record.recording')}</span>
            <span className="tabular-nums text-[var(--color-text-secondary)]">{elapsed}</span>
            <span className="truncate text-[11px] text-[var(--color-text-secondary)]">
              {t('computerUse.record.stepsRecorded', { count: stepCount })}
            </span>
          </div>
          {lastActionType && (
            <p className="truncate text-[11px] text-[var(--color-text-secondary)]">
              {lastActionType}
              {lastActionValue ? ` · ${lastActionValue}` : ''}
            </p>
          )}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => send({ action: 'stop' })}
              className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-lg bg-red-500 px-3 py-1.5 text-[12px] font-semibold text-white transition-opacity hover:opacity-90"
            >
              <span className="material-symbols-outlined text-[15px]">stop</span>
              {t('computerUse.record.stop')}
            </button>
            {narrationEnabled && (
              <button
                type="button"
                onClick={() => send({ action: 'toggle-mute' })}
                title={
                  narrationMuted
                    ? t('computerUse.narration.unmute')
                    : t('computerUse.narration.mute')
                }
                className="inline-flex items-center justify-center rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              >
                <span className="material-symbols-outlined text-[15px]">
                  {narrationMuted ? 'mic_off' : 'mic'}
                </span>
              </button>
            )}
            <button
              type="button"
              onClick={() => send({ action: 'discard' })}
              className="inline-flex items-center justify-center rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
            >
              {t('computerUse.record.discard')}
            </button>
          </div>
        </>
      )}

      {(status === 'stopped' || isGenerating || status === 'saved' || status === 'error') &&
        savedRecordingName && (
        <>
          <div className="flex items-center gap-2 rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-[12px] text-emerald-600 dark:text-emerald-400">
            <span className="material-symbols-outlined text-[16px]">check_circle</span>
            <span className="truncate">
              {t('computerUse.record.autoSaved', { name: savedRecordingName })}
            </span>
          </div>
          <button
            type="button"
            onClick={executeSaved}
            disabled={computerBusy}
            title={
              hasModel
                ? t('computerUse.record.executeNowSmartHint')
                : t('computerUse.skills.exactReplayHint')
            }
            className="inline-flex items-center justify-center gap-1.5 rounded-lg bg-[var(--color-brand)] px-3 py-1.5 text-[12px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            <span className="material-symbols-outlined text-[15px]">
              {hasModel ? 'auto_awesome' : 'play_arrow'}
            </span>
            {hasModel
              ? t('computerUse.record.executeNowSmart')
              : t('computerUse.record.executeNow')}
          </button>
          <div className="flex items-center gap-2">
            {savedSkillName ? (
              <div className="flex flex-1 items-center gap-2 rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-3 py-1.5 text-[11px] text-emerald-600 dark:text-emerald-400">
                <span className="material-symbols-outlined text-[14px]">auto_awesome</span>
                <span className="truncate">{t('computerUse.record.skillReady')}</span>
              </div>
            ) : (
              <button
                type="button"
                onClick={() => send({ action: 'generate' })}
                disabled={isGenerating || !hasModel}
                title={
                  hasModel
                    ? t('computerUse.skills.generateHint')
                    : t('computerUse.noVisionModels')
                }
                className="inline-flex flex-1 items-center justify-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-text-primary)] transition-colors hover:border-[var(--color-brand)]/50 hover:text-[var(--color-brand)] disabled:opacity-50"
              >
                {isGenerating && (
                  <span className="material-symbols-outlined animate-spin text-[15px]">
                    progress_activity
                  </span>
                )}
                {isGenerating
                  ? t('computerUse.record.generating')
                  : t('computerUse.record.generateOptional')}
              </button>
            )}
            <button
              type="button"
              onClick={() => send({ action: 'reset' })}
              className="inline-flex items-center justify-center rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
            >
              {t('common.close')}
            </button>
          </div>
          {statusMessage && (
            <p className="truncate text-[10px] text-[var(--color-text-tertiary)]">
              {statusMessage}
            </p>
          )}
        </>
      )}

      {status === 'stopped' && !savedRecordingName && (
        <>
          <p className="text-[11px] text-[var(--color-text-secondary)]">
            {t('computerUse.record.stepsRecorded', { count: stepCount })}
          </p>
          <button
            type="button"
            onClick={() => send({ action: 'reset' })}
            className="inline-flex items-center justify-center rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
          >
            {t('common.close')}
          </button>
        </>
      )}

      {error && (
        <div className="flex items-start gap-2 rounded-lg border border-red-500/30 bg-red-500/10 px-3 py-2 text-[11px] text-red-600 dark:text-red-400">
          <span className="min-w-0 flex-1 break-words">{error}</span>
          {status === 'error' && (
            <button
              type="button"
              onClick={() => send({ action: 'reset' })}
              className="shrink-0 rounded-md border border-red-500/40 px-2 py-0.5 text-[10px] font-medium transition-colors hover:bg-red-500/10"
            >
              {t('common.close')}
            </button>
          )}
        </div>
      )}
    </div>
  )
}
