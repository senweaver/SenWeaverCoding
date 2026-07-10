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
  MINIMAL_EVENT_COMPUTER_REPLY,
  MINIMAL_EVENT_COMPUTER_START,
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
  const recorderStatus = useMinimalRecorderStore((s) => s.status)
  const computerBusy = isComputerBusy(status)
  const busy = computerBusy || recorderStatus === 'recording'
  const replyMode = status === 'call_user'

  const [task, setTask] = useState('')

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

  const canSubmit = replyMode
    ? task.trim().length > 0
    : task.trim().length > 0 && !!provider && !!model && !busy

  const submit = () => {
    const text = task.trim()
    if (!text) return
    if (replyMode) {
      void emitEvent(MINIMAL_EVENT_COMPUTER_REPLY, { text })
    } else {
      if (!provider || !model || busy) return
      void emitEvent(MINIMAL_EVENT_COMPUTER_START, { task: text, provider, model })
    }
    setTask('')
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
      <textarea
        value={task}
        onChange={(e) => setTask(e.target.value)}
        onKeyDown={onKeyDown}
        rows={2}
        disabled={busy && !replyMode}
        placeholder={
          replyMode ? t('minimal.computer.reply') : t('minimal.computer.taskPlaceholder')
        }
        className="max-h-[120px] min-h-[48px] w-full resize-none rounded-xl border border-[var(--color-border)] bg-[var(--color-background)] px-3 py-2 text-[12px] text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] disabled:opacity-60"
      />

      <div className="flex items-center gap-2">
        {!replyMode && (
          <select
            value={selectValue}
            onChange={(e) => {
              const [p, m] = e.target.value.split('::')
              if (p && m) setSelection(p, m)
            }}
            disabled={busy || noModels}
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
        {replyMode && (
          <div className="flex min-w-0 flex-1 justify-end">
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

        {computerBusy && !replyMode ? (
          <button
            type="button"
            onClick={stop}
            className="inline-flex shrink-0 items-center gap-1 rounded-lg bg-[#e5484d] px-3 py-1.5 text-[12px] font-medium text-white transition-opacity hover:opacity-90"
          >
            <span className="material-symbols-outlined text-[15px]">stop</span>
            {t('minimal.computer.stop')}
          </button>
        ) : (
          <button
            type="button"
            onClick={submit}
            disabled={!canSubmit}
            className="inline-flex shrink-0 items-center gap-1 rounded-lg bg-[var(--color-brand)] px-3 py-1.5 text-[12px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
          >
            <span className="material-symbols-outlined text-[15px]">
              {replyMode ? 'send' : 'play_arrow'}
            </span>
            {replyMode ? t('minimal.computer.send') : t('minimal.computer.start')}
          </button>
        )}
      </div>
    </div>
  )
}
