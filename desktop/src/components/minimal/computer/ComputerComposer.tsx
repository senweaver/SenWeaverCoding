// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../../i18n'
import { useComputerUseStore } from '../../../stores/computerUseStore'
import { useMinimalComputerStore, isComputerBusy } from '../../../stores/minimalComputerStore'
import { useMinimalRecorderStore } from '../../../stores/minimalRecorderStore'
import type { VisionModel } from '../../../api/computer'
import {
  clipboardImageFiles,
  fileToAttachment,
  toComputerAttachments,
  type LocalAttachment,
} from '../../../lib/computerAttachments'
import {
  MINIMAL_EVENT_COMPUTER_REPLY,
  MINIMAL_EVENT_COMPUTER_START,
  MINIMAL_EVENT_COMPUTER_STEER,
  MINIMAL_EVENT_COMPUTER_STOP,
} from '../../../lib/minimalMode'

type ComputerComposerProps = {
  onHeightChange: (height: number) => void
  onSubmitted: () => void
}

type VisionProviderGroup = {
  providerId: string
  providerName: string
  models: VisionModel[]
}

async function emitEvent(name: string, payload?: unknown): Promise<void> {
  try {
    const { emit } = await import('@tauri-apps/api/event')
    await emit(name, payload)
  } catch (err) {
    console.warn(`[minimal] emit ${name} failed`, err)
  }
}

export function ComputerComposer({ onHeightChange, onSubmitted }: ComputerComposerProps) {
  const t = useTranslation()
  const wrapRef = useRef<HTMLDivElement>(null)

  const models = useComputerUseStore((s) => s.models)
  const modelsLoaded = useComputerUseStore((s) => s.modelsLoaded)
  const provider = useComputerUseStore((s) => s.provider)
  const model = useComputerUseStore((s) => s.model)
  const loadModels = useComputerUseStore((s) => s.loadModels)
  const setSelection = useComputerUseStore((s) => s.setSelection)

  const status = useMinimalComputerStore((s) => s.status)
  const pendingSteer = useMinimalComputerStore((s) => s.pendingSteer)
  const recorderStatus = useMinimalRecorderStore((s) => s.status)
  const computerBusy = isComputerBusy(status)
  const recording = recorderStatus === 'recording'
  const replyMode = status === 'call_user'

  const [task, setTask] = useState('')
  const [attachments, setAttachments] = useState<LocalAttachment[]>([])

  useEffect(() => {
    void loadModels()
  }, [loadModels])

  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const report = () => {
      const h = Math.round(el.getBoundingClientRect().height)
      if (h > 0) onHeightChange(h)
    }
    report()
    const ro = new ResizeObserver(report)
    ro.observe(el)
    return () => ro.disconnect()
  }, [onHeightChange])

  const noModels = modelsLoaded && models.length === 0
  const selectValue = provider && model ? `${provider}::${model}` : ''

  const groupedVisionModels = useMemo((): VisionProviderGroup[] => {
    const byProvider = new Map<string, VisionProviderGroup>()
    for (const m of models) {
      const existing = byProvider.get(m.provider)
      if (existing) existing.models.push(m)
      else
        byProvider.set(m.provider, {
          providerId: m.provider,
          providerName: m.provider_name?.trim() || m.provider,
          models: [m],
        })
    }
    return Array.from(byProvider.values()).sort((a, b) =>
      a.providerName.localeCompare(b.providerName, undefined, { sensitivity: 'base' }),
    )
  }, [models])

  const hasContent = task.trim().length > 0 || attachments.length > 0
  const canSubmit = replyMode
    ? task.trim().length > 0
    : computerBusy
      ? hasContent && !recording
      : task.trim().length > 0 && !!provider && !!model && !recording

  const addFiles = async (files: File[]) => {
    for (const file of files) {
      const attachment = await fileToAttachment(file)
      if (attachment) setAttachments((prev) => [...prev, attachment])
    }
  }

  const handlePaste = (event: React.ClipboardEvent) => {
    const files = clipboardImageFiles(event)
    if (files.length === 0) return
    event.preventDefault()
    void addFiles(files)
  }

  const submit = () => {
    const text = task.trim()
    if (replyMode) {
      if (!text) return
      void emitEvent(MINIMAL_EVENT_COMPUTER_REPLY, { text })
    } else if (computerBusy) {
      if (!hasContent || recording) return
      void emitEvent(MINIMAL_EVENT_COMPUTER_STEER, {
        text,
        ...(attachments.length > 0
          ? { attachments: toComputerAttachments(attachments) }
          : {}),
      })
    } else {
      if (!text || !provider || !model || recording) return
      void emitEvent(MINIMAL_EVENT_COMPUTER_START, {
        task: text,
        provider,
        model,
        ...(attachments.length > 0
          ? { attachments: toComputerAttachments(attachments) }
          : {}),
      })
    }
    setTask('')
    setAttachments([])
    onSubmitted()
  }

  const stop = () => {
    void emitEvent(MINIMAL_EVENT_COMPUTER_STOP)
  }

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey) {
      event.preventDefault()
      submit()
    }
  }

  return (
    <div
      ref={wrapRef}
      className="flex flex-col gap-2 overflow-hidden rounded-2xl border border-white/50 bg-[var(--color-surface)]/95 p-2.5 shadow-[0_10px_40px_rgba(30,58,95,0.28)] backdrop-blur-md"
      data-minimal-computer-composer
    >
      {attachments.length > 0 && (
        <div className="flex flex-wrap gap-1">
          {attachments.map((a) => (
            <span
              key={a.id}
              className="inline-flex max-w-[140px] items-center gap-1 rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-1.5 py-0.5 text-[9.5px] text-[var(--color-text-secondary)]"
            >
              <span className="material-symbols-outlined text-[11px]">
                {a.dataBase64 ? 'image' : 'description'}
              </span>
              <span className="truncate">{a.name}</span>
              <button
                type="button"
                onClick={() => setAttachments((prev) => prev.filter((x) => x.id !== a.id))}
                className="inline-flex items-center justify-center text-[var(--color-text-tertiary)] hover:text-red-500"
                aria-label={t('common.delete')}
              >
                <span className="material-symbols-outlined text-[11px]">close</span>
              </button>
            </span>
          ))}
        </div>
      )}
      <textarea
        value={task}
        onChange={(e) => setTask(e.target.value)}
        onKeyDown={onKeyDown}
        onPaste={handlePaste}
        rows={2}
        disabled={recording}
        placeholder={
          replyMode
            ? t('minimal.computer.reply')
            : computerBusy
              ? t('minimal.computer.steerPlaceholder')
              : t('minimal.computer.taskPlaceholder')
        }
        className="max-h-[120px] min-h-[48px] w-full resize-none rounded-xl border border-[var(--color-border)] bg-[var(--color-background)] px-3 py-2 text-[12px] text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] disabled:opacity-60"
      />

      {pendingSteer && computerBusy && !replyMode && (
        <div className="flex items-center gap-1 text-[9.5px] text-amber-600 dark:text-amber-400">
          <span className="material-symbols-outlined text-[11px]">schedule_send</span>
          <span className="truncate">{t('minimal.computer.steerPending')}</span>
        </div>
      )}

      <div className="flex items-center gap-2">
        {!replyMode && !computerBusy && (
          <select
            value={selectValue}
            onChange={(e) => {
              const [p, m] = e.target.value.split('::')
              if (p && m) setSelection(p, m)
            }}
            disabled={noModels || recording}
            className="min-w-0 flex-1 rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[11px] text-[var(--color-text-primary)] outline-none disabled:opacity-60"
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
        )}
        {(replyMode || computerBusy) && (
          <div className="flex min-w-0 flex-1 items-center">
            <button
              type="button"
              onClick={stop}
              className="inline-flex shrink-0 items-center gap-1 rounded-lg border border-[#e5484d]/40 px-3 py-1.5 text-[12px] font-medium text-[#e5484d] transition-colors hover:bg-[#e5484d]/10"
            >
              <span className="material-symbols-outlined text-[15px]">stop</span>
              {t('minimal.computer.stop')}
            </button>
          </div>
        )}

        <button
          type="button"
          onClick={submit}
          disabled={!canSubmit}
          className="inline-flex shrink-0 items-center gap-1 rounded-lg bg-[var(--color-brand)] px-3 py-1.5 text-[12px] font-medium text-[var(--color-on-primary)] transition-opacity hover:opacity-90 disabled:opacity-50"
        >
          <span className="material-symbols-outlined text-[15px]">
            {replyMode || computerBusy ? 'send' : 'play_arrow'}
          </span>
          {replyMode || computerBusy
            ? t('minimal.computer.send')
            : t('minimal.computer.start')}
        </button>
      </div>
    </div>
  )
}
