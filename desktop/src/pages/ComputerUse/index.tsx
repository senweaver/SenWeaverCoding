// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { memo, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation, type TranslationKey } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import {
  useComputerUseStore,
  type ComputerStatus,
  type ComputerStep,
} from '../../stores/computerUseStore'
import { useComputerRecorderStore } from '../../stores/computerRecorderStore'
import { draftPlan, type VisionModel } from '../../api/computer'
import { enterMinimalMode } from '../../lib/minimalMode'
import {
  clipboardImageFiles,
  fileToAttachment,
  toComputerAttachments,
  type LocalAttachment,
} from '../../lib/computerAttachments'
import { RecorderPanel } from './RecorderPanel'
import { SkillLibrary } from './SkillLibrary'

type VisionProviderGroup = {
  providerId: string
  providerName: string
  models: VisionModel[]
}

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
    case 'move_mouse':
      return 'mouse'
    case 'wait':
      return 'hourglass_empty'
    case 'finished':
      return 'check_circle'
    case 'call_user':
      return 'help'
    default:
      return 'bolt'
  }
}

function statusKey(status: ComputerStatus): TranslationKey {
  switch (status) {
    case 'idle':
      return 'computerUse.status.idle'
    case 'connecting':
      return 'computerUse.connecting'
    case 'running':
      return 'computerUse.status.running'
    case 'thinking':
      return 'computerUse.status.thinking'
    case 'finished':
      return 'computerUse.status.finished'
    case 'call_user':
      return 'computerUse.status.callUser'
    case 'error':
      return 'computerUse.status.error'
    case 'stopped':
      return 'computerUse.status.stopped'
    default:
      return 'computerUse.status.idle'
  }
}

function statusColor(status: ComputerStatus): string {
  switch (status) {
    case 'running':
    case 'thinking':
    case 'connecting':
      return 'bg-[var(--color-brand)]/15 text-[var(--color-brand)]'
    case 'finished':
      return 'bg-emerald-500/15 text-emerald-600 dark:text-emerald-400'
    case 'call_user':
      return 'bg-amber-500/15 text-amber-600 dark:text-amber-400'
    case 'error':
      return 'bg-red-500/15 text-red-600 dark:text-red-400'
    default:
      return 'bg-[var(--color-text-secondary)]/12 text-[var(--color-text-secondary)]'
  }
}

const StepListItem = memo(function StepListItem({
  step,
  idx,
  active,
  onSelect,
}: {
  step: ComputerStep
  idx: number
  active: boolean
  onSelect: (index: number) => void
}) {
  const t = useTranslation()
  if (step.kind === 'user_update') {
    return (
      <li>
        <div className="w-full rounded-lg border border-amber-500/40 bg-amber-500/10 px-3 py-2">
          <div className="mb-1 flex items-center gap-2">
            <span className="inline-flex items-center gap-1 rounded-md bg-amber-500/20 px-1.5 py-0.5 text-[10px] font-semibold text-amber-600 dark:text-amber-400">
              <span className="material-symbols-outlined text-[13px]">
                record_voice_over
              </span>
              {t('computerUse.userUpdate')}
            </span>
          </div>
          <p className="text-[12px] leading-snug text-[var(--color-text-primary)]">
            {step.thought}
          </p>
        </div>
      </li>
    )
  }
  return (
    <li>
      <button
        type="button"
        onClick={() => onSelect(idx)}
        className={`w-full rounded-lg border px-3 py-2 text-left transition-colors ${
          active
            ? 'border-[var(--color-brand)] bg-[var(--color-brand)]/8'
            : 'border-[var(--color-border)] bg-[var(--color-surface)] hover:border-[var(--color-brand)]/50'
        }`}
      >
        <div className="mb-1 flex items-center gap-2">
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
          {step.success === false && (
            <span className="material-symbols-outlined ml-auto text-[14px] text-red-500">
              error
            </span>
          )}
        </div>
        {step.thought && (
          <p className="text-[12px] leading-snug text-[var(--color-text-primary)]">
            {step.thought}
          </p>
        )}
        {step.value && (
          <p className="mt-1 truncate text-[11px] text-[var(--color-text-secondary)]">
            {step.value}
          </p>
        )}
      </button>
    </li>
  )
})

export function ComputerUsePage() {
  const t = useTranslation()

  const models = useComputerUseStore((s) => s.models)
  const modelsLoaded = useComputerUseStore((s) => s.modelsLoaded)
  const provider = useComputerUseStore((s) => s.provider)
  const model = useComputerUseStore((s) => s.model)
  const maxSteps = useComputerUseStore((s) => s.maxSteps)
  const stepDelayMs = useComputerUseStore((s) => s.stepDelayMs)
  const task = useComputerUseStore((s) => s.task)
  const status = useComputerUseStore((s) => s.status)
  const statusMessage = useComputerUseStore((s) => s.statusMessage)
  const error = useComputerUseStore((s) => s.error)
  const steps = useComputerUseStore((s) => s.steps)
  const selectedStepIndex = useComputerUseStore((s) => s.selectedStepIndex)
  const pendingSteer = useComputerUseStore((s) => s.pendingSteer)

  const loadModels = useComputerUseStore((s) => s.loadModels)
  const setSelection = useComputerUseStore((s) => s.setSelection)
  const setMaxSteps = useComputerUseStore((s) => s.setMaxSteps)
  const setStepDelayMs = useComputerUseStore((s) => s.setStepDelayMs)
  const setTask = useComputerUseStore((s) => s.setTask)
  const selectStep = useComputerUseStore((s) => s.selectStep)
  const send = useComputerUseStore((s) => s.send)
  const stop = useComputerUseStore((s) => s.stop)
  const sendReply = useComputerUseStore((s) => s.sendReply)
  const resetRun = useComputerUseStore((s) => s.reset)

  const [reply, setReply] = useState('')
  const [showRecorder, setShowRecorder] = useState(false)
  const [showSkills, setShowSkills] = useState(false)
  const [attachments, setAttachments] = useState<LocalAttachment[]>([])
  const [draftBusy, setDraftBusy] = useState(false)
  const [draftError, setDraftError] = useState<string | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)
  const stepListRef = useRef<HTMLOListElement>(null)
  const prevSelectedRef = useRef<number | null>(null)

  useEffect(() => {
    void loadModels()
  }, [loadModels])

  useEffect(() => {
    const idx = selectedStepIndex ?? steps.length - 1
    if (idx < 0) return
    const selectionChanged = prevSelectedRef.current !== selectedStepIndex
    prevSelectedRef.current = selectedStepIndex
    const following =
      selectedStepIndex === null || selectedStepIndex >= steps.length - 1
    if (!selectionChanged && !following) return
    const el = stepListRef.current?.children[idx] as HTMLElement | undefined
    el?.scrollIntoView({ block: 'nearest' })
  }, [selectedStepIndex, steps.length])

  const recorderStatus = useComputerRecorderStore((s) => s.status)
  const recorderRecording = recorderStatus === 'recording'

  const busy = status === 'running' || status === 'thinking' || status === 'connecting'
  const finishedRun =
    steps.length > 0 &&
    (status === 'finished' || status === 'stopped' || status === 'error')
  const noModels = modelsLoaded && models.length === 0

  const selectedStep: ComputerStep | null = useMemo(() => {
    if (selectedStepIndex === null) return steps.length > 0 ? steps[steps.length - 1] ?? null : null
    return steps[selectedStepIndex] ?? null
  }, [steps, selectedStepIndex])

  const addFiles = async (files: File[]) => {
    for (const file of files) {
      const attachment = await fileToAttachment(file)
      if (attachment) {
        setAttachments((prev) => [...prev, attachment])
      } else {
        setDraftError(t('computerUse.attachmentRejected'))
      }
    }
  }

  const handlePaste = (event: React.ClipboardEvent) => {
    const files = clipboardImageFiles(event)
    if (files.length === 0) return
    event.preventDefault()
    void addFiles(files)
  }

  const removeAttachment = (id: string) => {
    setAttachments((prev) => prev.filter((a) => a.id !== id))
  }

  const handleSend = () => {
    if (recorderRecording) return
    setDraftError(null)
    const text = task.trim()
    const hasAttachments = attachments.length > 0
    if (busy) {
      if (!text && !hasAttachments) return
      const ok = send(text, hasAttachments ? toComputerAttachments(attachments) : undefined)
      if (ok) {
        setTask('')
        setAttachments([])
      }
      return
    }
    if (!text || !provider || !model) return
    const ok = send(text, hasAttachments ? toComputerAttachments(attachments) : undefined)
    if (!ok) return
    setTask('')
    setAttachments([])
    void enterMinimalMode('computer')
  }

  const handleDraftPlan = async () => {
    if (draftBusy || busy || recorderRecording) return
    if (!provider || !model) return
    if (!task.trim() && attachments.length === 0) return
    setDraftBusy(true)
    setDraftError(null)
    try {
      const steps = await draftPlan(
        task.trim(),
        toComputerAttachments(attachments),
        provider,
        model,
      )
      if (steps.trim()) setTask(steps.trim())
    } catch (err) {
      setDraftError(err instanceof Error ? err.message : String(err))
    } finally {
      setDraftBusy(false)
    }
  }

  const selectValue = provider && model ? `${provider}::${model}` : ''

  const groupedVisionModels = useMemo((): VisionProviderGroup[] => {
    const byProvider = new Map<string, VisionProviderGroup>()
    for (const m of models) {
      const existing = byProvider.get(m.provider)
      if (existing) {
        existing.models.push(m)
      } else {
        byProvider.set(m.provider, {
          providerId: m.provider,
          providerName: m.provider_name?.trim() || m.provider,
          models: [m],
        })
      }
    }
    return Array.from(byProvider.values()).sort((a, b) =>
      a.providerName.localeCompare(b.providerName, undefined, { sensitivity: 'base' }),
    )
  }, [models])

  return (
    <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden bg-[var(--color-background)]">
      <div className="flex shrink-0 flex-wrap items-center gap-3 border-b border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-2.5">
        <div className="flex items-center gap-2">
          <span className="material-symbols-outlined text-[20px] text-[var(--color-brand)]">
            desktop_windows
          </span>
          <div className="leading-tight">
            <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
              {t('computerUse.title')}
            </div>
            <div className="text-[11px] text-[var(--color-text-secondary)]">
              {t('computerUse.subtitle')}
            </div>
          </div>
        </div>

        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => setShowRecorder(true)}
            disabled={busy || status === 'call_user'}
            className="inline-flex items-center gap-1.5 rounded-md border border-[var(--color-border)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:border-red-500/50 hover:text-red-500 disabled:opacity-50"
          >
            <span className="material-symbols-outlined text-[15px]">fiber_manual_record</span>
            {t('computerUse.record.button')}
          </button>
          <button
            type="button"
            onClick={() => setShowSkills(true)}
            className="inline-flex items-center gap-1.5 rounded-md border border-[var(--color-border)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:border-[var(--color-brand)]/50 hover:text-[var(--color-brand)]"
          >
            <span className="material-symbols-outlined text-[15px]">auto_awesome_motion</span>
            {t('computerUse.skills.button')}
          </button>
        </div>

        <div className="flex items-center gap-2">
          <span className="text-[11px] text-[var(--color-text-secondary)]">
            {t('computerUse.model')}
          </span>
          <select
            value={selectValue}
            onChange={(e) => {
              const [p, m] = e.target.value.split('::')
              if (p && m) setSelection(p, m)
            }}
            disabled={busy || noModels}
            className="max-w-[260px] rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[12px] text-[var(--color-text-primary)] outline-none disabled:opacity-60"
          >
            {noModels && <option value="">{t('computerUse.noVisionModels')}</option>}
            {groupedVisionModels.map((group) => (
              <optgroup key={group.providerId} label={group.providerName}>
                {group.models.map((m) => (
                  <option key={`${m.provider}::${m.model}`} value={`${m.provider}::${m.model}`}>
                    {m.model}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </div>

        <label className="flex items-center gap-1.5 text-[11px] text-[var(--color-text-secondary)]">
          {t('computerUse.maxSteps')}
          <input
            type="number"
            min={1}
            max={200}
            value={maxSteps}
            disabled={busy}
            onChange={(e) => setMaxSteps(Number(e.target.value))}
            className="w-16 rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[12px] text-[var(--color-text-primary)] outline-none disabled:opacity-60"
          />
        </label>

        <label className="flex items-center gap-1.5 text-[11px] text-[var(--color-text-secondary)]">
          {t('computerUse.stepDelay')}
          <input
            type="number"
            min={0}
            max={10000}
            step={100}
            value={stepDelayMs}
            disabled={busy}
            onChange={(e) => setStepDelayMs(Number(e.target.value))}
            className="w-20 rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[12px] text-[var(--color-text-primary)] outline-none disabled:opacity-60"
          />
        </label>

        <div className="ml-auto flex items-center gap-2">
          <span
            className={`inline-flex items-center gap-1 rounded-full px-2.5 py-1 text-[11px] font-medium ${statusColor(
              status,
            )}`}
          >
            {busy && (
              <span className="material-symbols-outlined animate-spin text-[14px]">
                progress_activity
              </span>
            )}
            {t(statusKey(status))}
          </span>
          <button
            type="button"
            title={t('computerUse.minimize')}
            aria-label={t('computerUse.minimize')}
            onClick={() => void enterMinimalMode('computer')}
            className="inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
          >
            <span className="material-symbols-outlined text-[16px]">picture_in_picture_alt</span>
          </button>
        </div>
      </div>

      <div className="flex min-h-0 flex-1 overflow-hidden">
        <div className="flex min-h-0 basis-2/5 flex-col border-r border-[var(--color-border)]">
          <div className="flex items-center gap-1.5 border-b border-[var(--color-border)] px-4 py-2 text-[12px] font-medium text-[var(--color-text-secondary)]">
            <span className="material-symbols-outlined text-[16px]">list_alt</span>
            {t('computerUse.steps')}
            {steps.length > 0 && (
              <span className="text-[var(--color-text-tertiary)]">({steps.length})</span>
            )}
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto px-3 py-3">
            {steps.length === 0 ? (
              <div className="px-3 py-8 text-center text-[12px] leading-relaxed text-[var(--color-text-secondary)]">
                {t('computerUse.empty')}
              </div>
            ) : (
              <ol ref={stepListRef} className="flex flex-col gap-2">
                {steps.map((step, idx) => (
                  <StepListItem
                    key={step.uid}
                    step={step}
                    idx={idx}
                    active={(selectedStepIndex ?? steps.length - 1) === idx}
                    onSelect={selectStep}
                  />
                ))}
              </ol>
            )}
          </div>

          {noModels && (
            <div className="mx-3 mb-2 flex items-center gap-2 rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2">
              <span className="material-symbols-outlined shrink-0 text-[15px] text-amber-600 dark:text-amber-400">
                warning
              </span>
              <span className="min-w-0 flex-1 text-[11px] leading-snug text-amber-700 dark:text-amber-400">
                {t('computerUse.noVisionModels')}
              </span>
              <button
                type="button"
                onClick={() => {
                  useUIStore.getState().setAppMode('code')
                  useUIStore.getState().openSettingsOverlay('providers')
                }}
                className="shrink-0 rounded-md border border-amber-500/50 px-2 py-1 text-[11px] font-medium text-amber-700 transition-colors hover:bg-amber-500/15 dark:text-amber-400"
              >
                {t('computerUse.openSettings')}
              </button>
            </div>
          )}

          {error && (
            <div className="mx-3 mb-2 rounded-md border border-red-500/30 bg-red-500/10 px-3 py-2 text-[11px] text-red-600 dark:text-red-400">
              {error}
            </div>
          )}

          {statusMessage && status !== 'error' && (
            <div className="mx-3 mb-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-[11px] text-[var(--color-text-secondary)]">
              {statusMessage}
            </div>
          )}

          <div className="shrink-0 border-t border-[var(--color-border)] p-3">
            {status === 'call_user' ? (
              <div className="flex flex-col gap-2">
                <textarea
                  value={reply}
                  onChange={(e) => setReply(e.target.value)}
                  placeholder={t('computerUse.replyPlaceholder')}
                  rows={2}
                  className="resize-none rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] px-3 py-2 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
                />
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => {
                      sendReply(reply)
                      setReply('')
                    }}
                    disabled={!reply.trim()}
                    className="flex-1 rounded-lg bg-[var(--color-brand)] px-3 py-2 text-[12px] font-medium text-[var(--color-on-primary)] transition-opacity hover:opacity-90 disabled:opacity-50"
                  >
                    {t('computerUse.send')}
                  </button>
                  <button
                    type="button"
                    onClick={stop}
                    className="rounded-lg border border-red-500/40 px-3 py-2 text-[12px] font-medium text-red-600 transition-colors hover:bg-red-500/10 dark:text-red-400"
                  >
                    {t('computerUse.stop')}
                  </button>
                </div>
              </div>
            ) : (
              <div className="flex flex-col gap-2">
                {finishedRun && (
                  <button
                    type="button"
                    onClick={() => {
                      resetRun()
                      setTask('')
                    }}
                    className="flex items-center justify-center gap-1.5 rounded-lg border border-[var(--color-border)] px-3 py-2 text-[12px] font-medium text-[var(--color-text-primary)] transition-colors hover:border-[var(--color-brand)]/50 hover:text-[var(--color-brand)]"
                  >
                    <span className="material-symbols-outlined text-[16px]">restart_alt</span>
                    {t('computerUse.newTask')}
                  </button>
                )}
                {attachments.length > 0 && (
                  <div className="flex flex-wrap gap-1.5">
                    {attachments.map((a) => (
                      <span
                        key={a.id}
                        className="inline-flex max-w-[180px] items-center gap-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-secondary)]"
                      >
                        <span className="material-symbols-outlined text-[12px]">
                          {a.dataBase64 ? 'image' : 'description'}
                        </span>
                        <span className="truncate">{a.name}</span>
                        <button
                          type="button"
                          onClick={() => removeAttachment(a.id)}
                          className="inline-flex items-center justify-center text-[var(--color-text-tertiary)] hover:text-red-500"
                          aria-label={t('common.delete')}
                        >
                          <span className="material-symbols-outlined text-[12px]">close</span>
                        </button>
                      </span>
                    ))}
                  </div>
                )}
                <textarea
                  value={task}
                  onChange={(e) => setTask(e.target.value)}
                  onPaste={handlePaste}
                  placeholder={
                    busy ? t('computerUse.steerPlaceholder') : t('computerUse.taskPlaceholder')
                  }
                  rows={3}
                  disabled={recorderRecording}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                      e.preventDefault()
                      handleSend()
                    }
                  }}
                  className="resize-none rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] px-3 py-2 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)] disabled:opacity-60"
                />
                <input
                  ref={fileInputRef}
                  type="file"
                  multiple
                  accept="image/png,image/jpeg,image/webp,image/gif,.txt,.md,.markdown,.json,.csv,.log,.yaml,.yml"
                  className="hidden"
                  onChange={(e) => {
                    const files = Array.from(e.target.files ?? [])
                    e.target.value = ''
                    void addFiles(files)
                  }}
                />
                <div className="flex items-center gap-2">
                  <button
                    type="button"
                    onClick={() => fileInputRef.current?.click()}
                    disabled={recorderRecording}
                    title={t('computerUse.attach')}
                    aria-label={t('computerUse.attach')}
                    className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-[var(--color-border)] text-[var(--color-text-secondary)] transition-colors hover:border-[var(--color-brand)]/50 hover:text-[var(--color-brand)] disabled:opacity-50"
                  >
                    <span className="material-symbols-outlined text-[16px]">attach_file</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => void handleDraftPlan()}
                    disabled={
                      busy ||
                      draftBusy ||
                      recorderRecording ||
                      !provider ||
                      !model ||
                      (!task.trim() && attachments.length === 0)
                    }
                    title={t('computerUse.generateStepsHint')}
                    className="inline-flex h-8 shrink-0 items-center gap-1 rounded-lg border border-[var(--color-border)] px-2.5 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:border-[var(--color-brand)]/50 hover:text-[var(--color-brand)] disabled:opacity-50"
                  >
                    {draftBusy ? (
                      <span className="material-symbols-outlined animate-spin text-[14px]">
                        progress_activity
                      </span>
                    ) : (
                      <span className="material-symbols-outlined text-[14px]">
                        format_list_numbered
                      </span>
                    )}
                    {draftBusy
                      ? t('computerUse.generatingSteps')
                      : t('computerUse.generateSteps')}
                  </button>
                  {busy ? (
                    <>
                      <button
                        type="button"
                        onClick={handleSend}
                        disabled={!task.trim() && attachments.length === 0}
                        className="flex h-8 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-lg bg-[var(--color-brand)] px-3 text-[12px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90 disabled:opacity-50"
                      >
                        <span className="material-symbols-outlined text-[16px]">send</span>
                        {t('computerUse.sendSteer')}
                      </button>
                      <button
                        type="button"
                        onClick={stop}
                        className="flex h-8 shrink-0 items-center justify-center gap-1.5 rounded-lg bg-red-500 px-3 text-[12px] font-semibold text-white transition-opacity hover:opacity-90"
                      >
                        <span className="material-symbols-outlined text-[16px]">stop</span>
                        {t('computerUse.stop')}
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      onClick={handleSend}
                      disabled={!task.trim() || !provider || !model || recorderRecording}
                      className="flex h-8 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-lg bg-[var(--color-brand)] px-3 text-[12px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90 disabled:opacity-50"
                    >
                      <span className="material-symbols-outlined text-[16px]">play_arrow</span>
                      {t('computerUse.run')}
                    </button>
                  )}
                </div>
                {pendingSteer && busy && (
                  <p className="flex items-start gap-1 text-[10px] leading-snug text-amber-600 dark:text-amber-400">
                    <span className="material-symbols-outlined text-[13px]">schedule_send</span>
                    {t('computerUse.steerPending')}
                  </p>
                )}
                {draftError && (
                  <p className="text-[10px] leading-snug text-red-500">{draftError}</p>
                )}
                <p className="flex items-start gap-1 text-[10px] leading-snug text-[var(--color-text-tertiary)]">
                  <span className="material-symbols-outlined text-[13px] text-amber-500">
                    warning
                  </span>
                  {t('computerUse.warning')}
                </p>
              </div>
            )}
          </div>
        </div>

        <div className="flex min-h-0 basis-3/5 flex-col">
          <div className="flex items-center gap-1.5 border-b border-[var(--color-border)] px-4 py-2 text-[12px] font-medium text-[var(--color-text-secondary)]">
            <span className="material-symbols-outlined text-[16px]">screenshot_monitor</span>
            {t('computerUse.screenshot')}
          </div>

          <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto bg-[var(--color-surface-container)] p-4">
            {selectedStep && selectedStep.screenshotBase64 ? (
              <div className="relative inline-block max-h-full max-w-full">
                <img
                  src={`data:${selectedStep.screenshotMime ?? 'image/png'};base64,${selectedStep.screenshotBase64}`}
                  alt={t('computerUse.screenshot')}
                  decoding="async"
                  className="max-h-full max-w-full rounded-lg border border-[var(--color-border)] object-contain shadow-sm"
                />
                {selectedStep.targetXNorm !== undefined &&
                  selectedStep.targetYNorm !== undefined &&
                  selectedStep.toXNorm !== undefined &&
                  selectedStep.toYNorm !== undefined && (
                    <svg
                      className="pointer-events-none absolute inset-0 h-full w-full"
                      aria-hidden="true"
                    >
                      <line
                        x1={`${(selectedStep.targetXNorm / 1000) * 100}%`}
                        y1={`${(selectedStep.targetYNorm / 1000) * 100}%`}
                        x2={`${(selectedStep.toXNorm / 1000) * 100}%`}
                        y2={`${(selectedStep.toYNorm / 1000) * 100}%`}
                        stroke="var(--color-brand)"
                        strokeWidth={2}
                        strokeDasharray="5 4"
                      />
                    </svg>
                  )}
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
                {selectedStep.toXNorm !== undefined &&
                  selectedStep.toYNorm !== undefined && (
                    <div
                      className="pointer-events-none absolute -translate-x-1/2 -translate-y-1/2"
                      style={{
                        left: `${(selectedStep.toXNorm / 1000) * 100}%`,
                        top: `${(selectedStep.toYNorm / 1000) * 100}%`,
                      }}
                    >
                      <span className="block h-3.5 w-3.5 rounded-sm border-2 border-white bg-amber-500 shadow" />
                    </div>
                  )}
              </div>
            ) : (
              <div className="text-center text-[12px] text-[var(--color-text-secondary)]">
                {t('computerUse.empty')}
              </div>
            )}
          </div>

          {steps.length > 0 && (
            <div className="flex shrink-0 items-center gap-3 border-t border-[var(--color-border)] px-4 py-2.5">
              <button
                type="button"
                onClick={() => selectStep(Math.max(0, (selectedStepIndex ?? steps.length - 1) - 1))}
                className="inline-flex h-7 w-7 items-center justify-center rounded-md border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-brand)]/10"
              >
                <span className="material-symbols-outlined text-[18px]">chevron_left</span>
              </button>
              <input
                type="range"
                min={0}
                max={steps.length - 1}
                value={selectedStepIndex ?? steps.length - 1}
                onChange={(e) => selectStep(Number(e.target.value))}
                className="flex-1 accent-[var(--color-brand)]"
              />
              <button
                type="button"
                onClick={() =>
                  selectStep(Math.min(steps.length - 1, (selectedStepIndex ?? steps.length - 1) + 1))
                }
                className="inline-flex h-7 w-7 items-center justify-center rounded-md border border-[var(--color-border)] text-[var(--color-text-secondary)] hover:bg-[var(--color-brand)]/10"
              >
                <span className="material-symbols-outlined text-[18px]">chevron_right</span>
              </button>
              <span className="w-16 text-right text-[11px] tabular-nums text-[var(--color-text-secondary)]">
                {(selectedStepIndex ?? steps.length - 1) + 1} / {steps.length}
              </span>
            </div>
          )}
        </div>
      </div>

      {showRecorder && <RecorderPanel onClose={() => setShowRecorder(false)} />}
      {showSkills && <SkillLibrary onClose={() => setShowSkills(false)} />}
    </div>
  )
}

export default ComputerUsePage
