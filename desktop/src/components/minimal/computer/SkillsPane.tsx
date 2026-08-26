// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from '../../../i18n'
import { useComputerUseStore } from '../../../stores/computerUseStore'
import { useMinimalComputerStore, isComputerBusy } from '../../../stores/minimalComputerStore'
import { useMinimalRecorderStore } from '../../../stores/minimalRecorderStore'
import {
  deleteRecording,
  generateRecordingSkill,
  listRecordings,
  type RecordingSummary,
} from '../../../api/computer'
import {
  MINIMAL_EVENT_COMPUTER_REPLAY,
  emitMinimalEvent,
  type MinimalComputerReplay,
} from '../../../lib/minimalMode'

type SkillsPaneProps = {
  onReplayStarted: () => void
}

export function SkillsPane({ onReplayStarted }: SkillsPaneProps) {
  const t = useTranslation()

  const provider = useComputerUseStore((s) => s.provider)
  const model = useComputerUseStore((s) => s.model)
  const loadModels = useComputerUseStore((s) => s.loadModels)

  const computerStatus = useMinimalComputerStore((s) => s.status)
  const recorderStatus = useMinimalRecorderStore((s) => s.status)
  const busy =
    isComputerBusy(computerStatus) ||
    computerStatus === 'call_user' ||
    recorderStatus === 'recording'

  const [recordings, setRecordings] = useState<RecordingSummary[]>([])
  const [loaded, setLoaded] = useState(false)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [expandedSkill, setExpandedSkill] = useState<string | null>(null)
  const [inputs, setInputs] = useState('')
  const [generatingNames, setGeneratingNames] = useState<string[]>([])
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null)
  const mountedRef = useRef(true)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
    }
  }, [])

  const refresh = useCallback(async () => {
    setLoadError(null)
    try {
      const list = await listRecordings()
      setRecordings(list)
    } catch (err) {
      setLoadError(err instanceof Error ? err.message : 'failed to load recordings')
    } finally {
      setLoaded(true)
    }
  }, [])

  useEffect(() => {
    void loadModels()
    void refresh()
  }, [loadModels, refresh])

  const hasModel = Boolean(provider && model)

  const generateSkill = (name: string) => {
    if (generatingNames.includes(name)) return
    setGeneratingNames((prev) => [...prev, name])
    void (async () => {
      try {
        await generateRecordingSkill(name, provider ?? undefined, model ?? undefined)
        const deadline = Date.now() + 5 * 60_000
        while (Date.now() < deadline && mountedRef.current) {
          await new Promise((resolve) => setTimeout(resolve, 3_000))
          try {
            const list = await listRecordings()
            const rec = list.find((r) => r.name === name)
            if (!rec || rec.has_skill) break
          } catch {
          }
        }
      } catch (err) {
        if (mountedRef.current) {
          setLoadError(err instanceof Error ? err.message : 'failed to start skill generation')
        }
      } finally {
        if (mountedRef.current) {
          setGeneratingNames((prev) => prev.filter((n) => n !== name))
          void refresh()
        }
      }
    })()
  }

  const replay = (payload: MinimalComputerReplay) => {
    void emitMinimalEvent(MINIMAL_EVENT_COMPUTER_REPLAY, payload)
    setExpandedSkill(null)
    setInputs('')
    onReplayStarted()
  }

  const remove = (name: string) => {
    setConfirmDelete(null)
    void (async () => {
      try {
        await deleteRecording(name)
        setRecordings((prev) => prev.filter((r) => r.name !== name))
      } catch (err) {
        setLoadError(err instanceof Error ? err.message : 'failed to delete recording')
      }
    })()
  }

  return (
    <div
      className="flex flex-col gap-2 overflow-hidden rounded-2xl border border-white/50 bg-[var(--color-surface)]/95 p-2.5 shadow-[0_10px_40px_rgba(30,58,95,0.28)] backdrop-blur-md"
      data-minimal-computer-skills
    >
      <div className="flex items-center justify-between">
        <span className="text-[11px] font-semibold text-[var(--color-text-secondary)]">
          {t('computerUse.skills.title')}
        </span>
        <button
          type="button"
          onClick={() => void refresh()}
          className="inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-1.5 py-0.5 text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-brand)]"
          aria-label={t('computerUse.skills.refresh')}
          title={t('computerUse.skills.refresh')}
        >
          <span className="material-symbols-outlined text-[14px]">refresh</span>
        </button>
      </div>

      {loadError && (
        <div className="rounded-lg border border-red-500/30 bg-red-500/10 px-2.5 py-1.5 text-[11px] text-red-600 dark:text-red-400">
          {loadError}
        </div>
      )}

      {loaded && recordings.length === 0 && !loadError ? (
        <p className="px-1 py-2 text-[11px] leading-snug text-[var(--color-text-tertiary)]">
          {t('computerUse.skills.empty')}
        </p>
      ) : (
        <ul className="flex max-h-[220px] flex-col gap-1.5 overflow-y-auto">
          {recordings.map((rec) => {
            const expanded = expandedSkill === rec.name
            return (
              <li
                key={rec.name}
                className="rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5"
              >
                <div className="flex items-center gap-1.5">
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-[11.5px] font-medium text-[var(--color-text-primary)]">
                      {rec.name}
                    </p>
                    <p className="truncate text-[10px] text-[var(--color-text-tertiary)]">
                      {t('computerUse.skills.steps', { count: rec.step_count })}
                      {!rec.has_skill && ` · ${t('computerUse.skills.notGenerated')}`}
                    </p>
                  </div>
                  {confirmDelete === rec.name ? (
                    <div className="flex shrink-0 items-center gap-1">
                      <button
                        type="button"
                        onClick={() => remove(rec.name)}
                        className="inline-flex items-center justify-center rounded-md bg-red-500 px-1.5 py-0.5 text-[10px] font-medium text-white transition-opacity hover:opacity-90"
                      >
                        {t('computerUse.skills.deleteConfirmYes')}
                      </button>
                      <button
                        type="button"
                        onClick={() => setConfirmDelete(null)}
                        className="inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-1.5 py-0.5 text-[10px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                      >
                        {t('common.cancel')}
                      </button>
                    </div>
                  ) : (
                    <button
                      type="button"
                      onClick={() => setConfirmDelete(rec.name)}
                      className="inline-flex shrink-0 items-center justify-center rounded-md px-1 py-0.5 text-[var(--color-text-tertiary)] transition-colors hover:text-red-500"
                      aria-label={t('computerUse.skills.delete')}
                      title={t('computerUse.skills.deleteConfirm')}
                    >
                      <span className="material-symbols-outlined text-[14px]">delete</span>
                    </button>
                  )}
                </div>

                <div className="mt-1.5 flex items-center gap-1.5">
                  <button
                    type="button"
                    onClick={() =>
                      replay({
                        name: rec.name,
                        mode: 'smart',
                        provider: provider ?? undefined,
                        model: model ?? undefined,
                      })
                    }
                    disabled={busy || !hasModel || rec.step_count === 0}
                    title={t('computerUse.skills.smartReplayHint')}
                    className="inline-flex flex-1 items-center justify-center gap-1 rounded-md bg-[var(--color-brand)] px-2 py-1 text-[10.5px] font-medium text-[var(--color-on-primary)] transition-opacity hover:opacity-90 disabled:opacity-50"
                  >
                    <span className="material-symbols-outlined text-[13px]">auto_awesome</span>
                    {t('computerUse.skills.smartReplay')}
                  </button>
                  {rec.has_trace && (
                    <button
                      type="button"
                      onClick={() => replay({ name: rec.name, mode: 'exact' })}
                      disabled={busy || rec.step_count === 0}
                      title={t('computerUse.skills.exactReplayHint')}
                      className="inline-flex flex-1 items-center justify-center gap-1 rounded-md border border-[var(--color-border)] px-2 py-1 text-[10.5px] font-medium text-[var(--color-text-primary)] transition-colors hover:border-[var(--color-brand)]/50 hover:text-[var(--color-brand)] disabled:opacity-50"
                    >
                      <span className="material-symbols-outlined text-[13px]">replay</span>
                      {t('computerUse.skills.exactReplay')}
                    </button>
                  )}
                  {rec.has_skill ? (
                    <button
                      type="button"
                      onClick={() => {
                        if (expanded) {
                          setExpandedSkill(null)
                        } else {
                          setExpandedSkill(rec.name)
                          setInputs('')
                        }
                      }}
                      disabled={busy || !hasModel}
                      title={t('computerUse.skills.variableReplayHint')}
                      aria-label={t('computerUse.skills.variableReplay')}
                      className="inline-flex shrink-0 items-center justify-center rounded-md border border-[var(--color-border)] px-1.5 py-1 text-[var(--color-text-primary)] transition-colors hover:border-[var(--color-brand)]/50 hover:text-[var(--color-brand)] disabled:opacity-50"
                    >
                      <span className="material-symbols-outlined text-[13px]">tune</span>
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => generateSkill(rec.name)}
                      disabled={busy || !hasModel || generatingNames.includes(rec.name)}
                      title={t('computerUse.skills.generateHint')}
                      aria-label={t('computerUse.skills.generate')}
                      className="inline-flex shrink-0 items-center justify-center rounded-md border border-[var(--color-border)] px-1.5 py-1 text-[var(--color-text-primary)] transition-colors hover:border-[var(--color-brand)]/50 hover:text-[var(--color-brand)] disabled:opacity-50"
                    >
                      {generatingNames.includes(rec.name) ? (
                        <span className="material-symbols-outlined animate-spin text-[13px]">
                          progress_activity
                        </span>
                      ) : (
                        <span className="material-symbols-outlined text-[13px]">
                          construction
                        </span>
                      )}
                    </button>
                  )}
                </div>

                {expanded && (
                  <div className="mt-1.5 flex flex-col gap-1.5">
                    <textarea
                      value={inputs}
                      onChange={(e) => setInputs(e.target.value)}
                      rows={2}
                      placeholder={t('computerUse.skills.replayInputsPlaceholder')}
                      className="w-full resize-none rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[11px] text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)] focus:border-[var(--color-brand)]"
                    />
                    <button
                      type="button"
                      onClick={() =>
                        replay({
                          name: rec.name,
                          mode: 'smart',
                          useSkill: true,
                          inputs: inputs.trim(),
                          provider: provider ?? undefined,
                          model: model ?? undefined,
                        })
                      }
                      disabled={busy || !hasModel}
                      className="inline-flex items-center justify-center gap-1 rounded-md bg-[var(--color-brand)] px-2 py-1 text-[10.5px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90 disabled:opacity-50"
                    >
                      <span className="material-symbols-outlined text-[13px]">play_arrow</span>
                      {t('computerUse.skills.replayStart')}
                    </button>
                  </div>
                )}
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}
