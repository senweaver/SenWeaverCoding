// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { enterMinimalMode } from '../../lib/minimalMode'
import {
  useComputerRecorderStore,
  type RecorderStep,
} from '../../stores/computerRecorderStore'
import { useComputerUseStore } from '../../stores/computerUseStore'
import { NARRATION_LANGUAGES } from '../../lib/computerNarration'
import { WhatsRecordedSheet } from './WhatsRecordedSheet'

let privacyReviewedThisRun = false

function actionIcon(actionType: string): string {
  switch (actionType) {
    case 'click':
    case 'double_click':
    case 'right_click':
      return 'ads_click'
    case 'type':
      return 'keyboard'
    case 'key_press':
      return 'keyboard_command_key'
    case 'scroll':
      return 'swap_vert'
    case 'drag':
      return 'drag_pan'
    default:
      return 'bolt'
  }
}

function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000))
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${m}:${s.toString().padStart(2, '0')}`
}

export function RecorderPanel({ onClose }: { onClose: () => void }) {
  const t = useTranslation()

  const status = useComputerRecorderStore((s) => s.status)
  const error = useComputerRecorderStore((s) => s.error)
  const statusMessage = useComputerRecorderStore((s) => s.statusMessage)
  const task = useComputerRecorderStore((s) => s.task)
  const savedRecordingName = useComputerRecorderStore((s) => s.savedRecordingName)
  const savedSkillName = useComputerRecorderStore((s) => s.savedSkillName)
  const steps = useComputerRecorderStore((s) => s.steps)
  const selectedStepIndex = useComputerRecorderStore((s) => s.selectedStepIndex)
  const startedAt = useComputerRecorderStore((s) => s.startedAt)

  const setTask = useComputerRecorderStore((s) => s.setTask)
  const selectStep = useComputerRecorderStore((s) => s.selectStep)
  const startRecording = useComputerRecorderStore((s) => s.startRecording)
  const stopRecording = useComputerRecorderStore((s) => s.stopRecording)
  const discardRecording = useComputerRecorderStore((s) => s.discardRecording)
  const generateSkill = useComputerRecorderStore((s) => s.generateSkill)
  const reset = useComputerRecorderStore((s) => s.reset)

  const narrationEnabled = useComputerRecorderStore((s) => s.narrationEnabled)
  const narrationLanguage = useComputerRecorderStore((s) => s.narrationLanguage)
  const micDeviceId = useComputerRecorderStore((s) => s.micDeviceId)
  const micDevices = useComputerRecorderStore((s) => s.micDevices)
  const narrationMuted = useComputerRecorderStore((s) => s.narrationMuted)
  const narrationError = useComputerRecorderStore((s) => s.narrationError)
  const setNarrationEnabled = useComputerRecorderStore((s) => s.setNarrationEnabled)
  const setNarrationLanguage = useComputerRecorderStore((s) => s.setNarrationLanguage)
  const setMicDevice = useComputerRecorderStore((s) => s.setMicDevice)
  const loadMicrophones = useComputerRecorderStore((s) => s.loadMicrophones)
  const toggleNarrationMuted = useComputerRecorderStore((s) => s.toggleNarrationMuted)

  const [showPrivacy, setShowPrivacy] = useState(false)
  const [showWhatsRecorded, setShowWhatsRecorded] = useState(false)

  useEffect(() => {
    if (narrationEnabled) void loadMicrophones()
  }, [narrationEnabled, loadMicrophones])

  const startComputerRun = useComputerUseStore((s) => s.start)
  const visionProvider = useComputerUseStore((s) => s.provider)
  const visionModel = useComputerUseStore((s) => s.model)
  const hasVisionModel = Boolean(visionProvider && visionModel)

  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    if (status !== 'recording' || startedAt === null) return
    const id = window.setInterval(() => setNow(Date.now()), 500)
    return () => window.clearInterval(id)
  }, [status, startedAt])

  const elapsed = startedAt !== null ? formatElapsed(now - startedAt) : '0:00'
  const isRecording = status === 'recording'
  const isGenerating = status === 'generating'
  const canClose = !isRecording

  const selectedStep: RecorderStep | null = useMemo(() => {
    if (steps.length === 0) return null
    if (selectedStepIndex === null) return steps[steps.length - 1] ?? null
    return steps[selectedStepIndex] ?? null
  }, [steps, selectedStepIndex])

  const handleClose = () => {
    if (!canClose) return
    if (status === 'stopped' || status === 'saved' || status === 'error') {
      reset()
    }
    onClose()
  }

  const beginRecording = () => {
    privacyReviewedThisRun = true
    setShowPrivacy(false)
    startRecording()
    onClose()
    void enterMinimalMode('computer')
  }

  const handleStart = () => {
    if (!privacyReviewedThisRun) {
      setShowPrivacy(true)
      return
    }
    beginRecording()
  }

  return (
    <div className="absolute inset-0 z-20 flex flex-col bg-[var(--color-background)]">
      <div
        className={`flex shrink-0 flex-wrap items-center gap-3 border-b px-4 py-2.5 ${
          isRecording
            ? 'border-red-500/40 bg-red-500/10'
            : 'border-[var(--color-border)] bg-[var(--color-surface)]'
        }`}
      >
        <div className="flex items-center gap-2">
          <span
            className={`material-symbols-outlined text-[20px] ${
              isRecording ? 'animate-pulse text-red-500' : 'text-[var(--color-brand)]'
            }`}
          >
            fiber_manual_record
          </span>
          <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
            {isRecording
              ? t('computerUse.record.recording')
              : t('computerUse.record.title')}
          </div>
        </div>

        {isRecording && (
          <div className="flex items-center gap-3 text-[12px] text-[var(--color-text-secondary)]">
            <span className="inline-flex items-center gap-1 tabular-nums">
              <span className="material-symbols-outlined text-[15px]">timer</span>
              {elapsed}
            </span>
            <span className="inline-flex items-center gap-1">
              <span className="material-symbols-outlined text-[15px]">list_alt</span>
              {t('computerUse.record.stepsRecorded', { count: steps.length })}
            </span>
          </div>
        )}

        <div className="ml-auto flex items-center gap-2">
          {isRecording ? (
            <>
              {narrationEnabled && (
                <button
                  type="button"
                  onClick={toggleNarrationMuted}
                  title={
                    narrationMuted
                      ? t('computerUse.narration.unmute')
                      : t('computerUse.narration.mute')
                  }
                  className="inline-flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                >
                  <span className="material-symbols-outlined text-[16px]">
                    {narrationMuted ? 'mic_off' : 'mic'}
                  </span>
                  {narrationMuted
                    ? t('computerUse.narration.unmute')
                    : t('computerUse.narration.mute')}
                </button>
              )}
              <button
                type="button"
                onClick={stopRecording}
                className="inline-flex items-center gap-1.5 rounded-lg bg-red-500 px-3 py-1.5 text-[12px] font-semibold text-white transition-opacity hover:opacity-90"
              >
                <span className="material-symbols-outlined text-[16px]">stop</span>
                {t('computerUse.record.stop')}
              </button>
              <button
                type="button"
                onClick={discardRecording}
                className="inline-flex items-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              >
                {t('computerUse.record.discard')}
              </button>
            </>
          ) : (
            <button
              type="button"
              onClick={handleClose}
              disabled={!canClose}
              className="inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] disabled:opacity-50 dark:hover:bg-white/[0.08]"
              aria-label={t('computerUse.skills.close')}
            >
              <span className="material-symbols-outlined text-[16px]">close</span>
            </button>
          )}
        </div>
      </div>

      {status === 'idle' && steps.length === 0 ? (
        <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 px-6 py-8">
          <div className="w-full max-w-lg space-y-3">
            <textarea
              value={task}
              onChange={(e) => setTask(e.target.value)}
              placeholder={t('computerUse.record.taskPlaceholder')}
              rows={3}
              className="w-full resize-none rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] px-3 py-2 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
            />
            <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
              <label className="flex items-center gap-2 text-[12px] font-medium text-[var(--color-text-primary)]">
                <input
                  type="checkbox"
                  checked={narrationEnabled}
                  onChange={(e) => setNarrationEnabled(e.target.checked)}
                  className="h-3.5 w-3.5 accent-[var(--color-brand)]"
                />
                <span className="material-symbols-outlined text-[15px] text-[var(--color-brand)]">mic</span>
                {t('computerUse.narration.enable')}
              </label>
              {narrationEnabled && (
                <div className="mt-2 flex flex-col gap-2">
                  <label className="flex items-center justify-between gap-2 text-[11px] text-[var(--color-text-secondary)]">
                    {t('computerUse.narration.language')}
                    <select
                      value={narrationLanguage}
                      onChange={(e) => setNarrationLanguage(e.target.value)}
                      className="rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[11px] text-[var(--color-text-primary)] outline-none"
                    >
                      {NARRATION_LANGUAGES.map((lang) => (
                        <option key={lang.code} value={lang.code}>
                          {lang.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label className="flex items-center justify-between gap-2 text-[11px] text-[var(--color-text-secondary)]">
                    {t('computerUse.narration.microphone')}
                    <select
                      value={micDeviceId ?? ''}
                      onChange={(e) => setMicDevice(e.target.value || null)}
                      className="min-w-0 max-w-[180px] rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[11px] text-[var(--color-text-primary)] outline-none"
                    >
                      <option value="">{t('computerUse.narration.defaultMic')}</option>
                      {micDevices.map((device) => (
                        <option key={device.id} value={device.id}>
                          {device.label}
                        </option>
                      ))}
                    </select>
                  </label>
                  <p className="text-[10px] leading-snug text-[var(--color-text-tertiary)]">
                    {t('computerUse.narration.hint')}
                  </p>
                </div>
              )}
            </div>
            <button
              type="button"
              onClick={handleStart}
              className="flex w-full items-center justify-center gap-1.5 rounded-lg bg-red-500 px-3 py-2 text-[12px] font-semibold text-white transition-opacity hover:opacity-90"
            >
              <span className="material-symbols-outlined text-[16px]">fiber_manual_record</span>
              {t('computerUse.record.start')}
            </button>
            <p className="flex items-start gap-1 text-[11px] leading-snug text-[var(--color-text-tertiary)]">
              <span className="material-symbols-outlined text-[14px] text-[var(--color-brand)]">
                info
              </span>
              {t('computerUse.record.hint')}
            </p>
            <p className="flex items-start gap-1 text-[10px] leading-snug text-[var(--color-text-tertiary)]">
              <span className="material-symbols-outlined text-[13px] text-amber-500">
                warning
              </span>
              {t('computerUse.record.windowsOnly')}
            </p>
            <p className="flex items-start gap-1 text-[10px] leading-snug text-[var(--color-text-tertiary)]">
              <span className="material-symbols-outlined text-[13px] text-amber-500">
                shield_lock
              </span>
              {t('computerUse.record.privacyHint')}
            </p>
            {narrationError && (
              <p className="flex items-start gap-1 text-[10px] leading-snug text-[var(--color-error)]">
                <span className="material-symbols-outlined text-[13px]">error</span>
                {t('computerUse.narration.error')}: {narrationError}
              </p>
            )}
          </div>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 overflow-hidden">
          <div className="flex min-h-0 basis-2/5 flex-col border-r border-[var(--color-border)]">
            <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
              {steps.length === 0 ? (
                <div className="px-3 py-8 text-center text-[12px] text-[var(--color-text-secondary)]">
                  {t('computerUse.record.hint')}
                </div>
              ) : (
                <ol className="flex flex-col gap-2">
                  {steps.map((step, idx) => {
                    const active = (selectedStepIndex ?? steps.length - 1) === idx
                    return (
                      <li key={step.index}>
                        <button
                          type="button"
                          onClick={() => selectStep(idx)}
                          className={`w-full rounded-lg border px-3 py-2 text-left transition-colors ${
                            active
                              ? 'border-[var(--color-brand)] bg-[var(--color-brand)]/8'
                              : 'border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-brand)]/50'
                          }`}
                        >
                          <div className="flex items-center gap-2">
                            <span className="inline-flex items-center gap-1 rounded-md bg-[var(--color-brand)]/12 px-1.5 py-0.5 text-[10px] font-semibold text-[var(--color-brand)]">
                              <span className="material-symbols-outlined text-[13px]">
                                {actionIcon(step.actionType)}
                              </span>
                              {step.actionType}
                            </span>
                            {step.elementDescription && (
                              <span className="truncate text-[11px] text-[var(--color-text-secondary)]">
                                {step.elementDescription}
                              </span>
                            )}
                          </div>
                          {step.value && (
                            <p className="mt-1 truncate text-[11px] text-[var(--color-text-secondary)]">
                              {step.value}
                            </p>
                          )}
                        </button>
                      </li>
                    )
                  })}
                </ol>
              )}
            </div>

            {error && (
              <div className="mx-3 mb-2 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-[11px] text-red-600 dark:text-red-400">
                {error}
              </div>
            )}

            {!isRecording && (
              <div className="shrink-0 border-t border-[var(--color-border)] p-3">
                {savedRecordingName ? (
                  <div className="flex flex-col gap-2">
                    <div className="flex items-center gap-2 rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-[12px] text-emerald-600 dark:text-emerald-400">
                      <span className="material-symbols-outlined text-[16px]">check_circle</span>
                      <span className="truncate">
                        {t('computerUse.record.autoSaved', { name: savedRecordingName })}
                      </span>
                    </div>
                    <button
                      type="button"
                      onClick={() => {
                        startComputerRun({
                          replayRecording: savedRecordingName,
                          smart: hasVisionModel,
                        })
                        reset()
                        onClose()
                        void enterMinimalMode('computer')
                      }}
                      title={
                        hasVisionModel
                          ? t('computerUse.record.executeNowSmartHint')
                          : t('computerUse.skills.exactReplayHint')
                      }
                      className="flex items-center justify-center gap-1.5 rounded-lg bg-[var(--color-brand)] px-3 py-2 text-[12px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90 disabled:opacity-50"
                    >
                      <span className="material-symbols-outlined text-[16px]">
                        {hasVisionModel ? 'auto_awesome' : 'play_arrow'}
                      </span>
                      {hasVisionModel
                        ? t('computerUse.record.executeNowSmart')
                        : t('computerUse.record.executeNow')}
                    </button>
                    {savedSkillName ? (
                      <div className="flex items-center gap-2 rounded-lg border border-emerald-500/30 bg-emerald-500/10 px-3 py-2 text-[12px] text-emerald-600 dark:text-emerald-400">
                        <span className="material-symbols-outlined text-[16px]">auto_awesome</span>
                        {t('computerUse.record.skillReady')}
                      </div>
                    ) : (
                      <button
                        type="button"
                        onClick={generateSkill}
                        disabled={isGenerating}
                        className="flex items-center justify-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-2 text-[12px] font-medium text-[var(--color-text-primary)] transition-colors hover:border-[var(--color-brand)]/50 hover:text-[var(--color-brand)] disabled:opacity-50"
                      >
                        {isGenerating ? (
                          <span className="material-symbols-outlined animate-spin text-[16px]">
                            progress_activity
                          </span>
                        ) : (
                          <span className="material-symbols-outlined text-[16px]">
                            auto_awesome
                          </span>
                        )}
                        {isGenerating
                          ? t('computerUse.record.generating')
                          : t('computerUse.record.generateOptional')}
                      </button>
                    )}
                    <button
                      type="button"
                      onClick={() => reset()}
                      className="flex items-center justify-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-2 text-[12px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                    >
                      <span className="material-symbols-outlined text-[16px]">
                        fiber_manual_record
                      </span>
                      {t('computerUse.record.newRecording')}
                    </button>
                    {statusMessage && (
                      <p className="text-[11px] text-[var(--color-text-tertiary)]">
                        {statusMessage}
                      </p>
                    )}
                  </div>
                ) : (
                  <div className="flex flex-col gap-2">
                    <p className="text-[11px] text-[var(--color-text-secondary)]">
                      {t('computerUse.record.summary')}
                    </p>
                    {statusMessage && (
                      <p className="text-[11px] text-[var(--color-text-tertiary)]">
                        {statusMessage}
                      </p>
                    )}
                  </div>
                )}
              </div>
            )}
          </div>

          <div className="flex min-h-0 basis-3/5 items-center justify-center overflow-auto bg-[var(--color-surface-container)] p-4">
            {selectedStep && selectedStep.screenshotBase64 ? (
              <div className="relative inline-block max-h-full max-w-full">
                <img
                  src={`data:image/jpeg;base64,${selectedStep.screenshotBase64}`}
                  alt={t('computerUse.screenshot')}
                  className="max-h-full max-w-full rounded-lg border border-[var(--color-border)] object-contain shadow-sm"
                />
                {selectedStep.targetXNorm !== undefined &&
                  selectedStep.targetYNorm !== undefined && (
                    <div
                      className="pointer-events-none absolute -translate-x-1/2 -translate-y-1/2"
                      style={{
                        left: `${(selectedStep.targetXNorm / 1000) * 100}%`,
                        top: `${(selectedStep.targetYNorm / 1000) * 100}%`,
                      }}
                    >
                      <span className="block h-6 w-6 animate-ping rounded-full bg-[var(--color-brand)]/40" />
                      <span className="absolute left-1/2 top-1/2 h-3 w-3 -translate-x-1/2 -translate-y-1/2 rounded-full border-2 border-white bg-[var(--color-brand)] shadow" />
                    </div>
                  )}
              </div>
            ) : (
              <div className="text-center text-[12px] text-[var(--color-text-secondary)]">
                {t('computerUse.record.hint')}
              </div>
            )}
          </div>
        </div>
      )}

      {showPrivacy && (
        <div className="absolute inset-0 z-40 flex items-center justify-center bg-black/40 p-6">
          <div className="w-[min(440px,94vw)] overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-2xl">
            <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-4 py-3">
              <span className="material-symbols-outlined text-[18px] text-amber-500">shield_lock</span>
              <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
                {t('computerUse.privacy.title')}
              </div>
            </div>
            <div className="px-4 py-3">
              <p className="text-[12px] leading-relaxed text-[var(--color-text-secondary)]">
                {t('computerUse.privacy.body')}
              </p>
              <button
                type="button"
                onClick={() => setShowWhatsRecorded(true)}
                className="mt-2 text-[11px] text-[var(--color-brand)] hover:opacity-80"
              >
                {t('computerUse.privacy.whatsRecorded')}
              </button>
            </div>
            <div className="flex items-center justify-end gap-2 border-t border-[var(--color-border)] px-4 py-3">
              <button
                type="button"
                onClick={() => setShowPrivacy(false)}
                className="rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              >
                {t('computerUse.privacy.cancel')}
              </button>
              <button
                type="button"
                onClick={beginRecording}
                className="inline-flex items-center gap-1.5 rounded-md bg-red-500 px-3 py-1.5 text-[11px] font-semibold text-white transition-opacity hover:opacity-90"
              >
                <span className="material-symbols-outlined text-[14px]">fiber_manual_record</span>
                {t('computerUse.privacy.start')}
              </button>
            </div>
          </div>
        </div>
      )}

      {showWhatsRecorded && (
        <WhatsRecordedSheet onClose={() => setShowWhatsRecorded(false)} />
      )}
    </div>
  )
}

export default RecorderPanel
