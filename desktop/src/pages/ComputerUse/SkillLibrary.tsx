// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import { enterMinimalMode } from '../../lib/minimalMode'
import { useComputerRecorderStore } from '../../stores/computerRecorderStore'
import { useComputerUseStore } from '../../stores/computerUseStore'

export function SkillLibrary({ onClose }: { onClose: () => void }) {
  const t = useTranslation()

  const recordings = useComputerRecorderStore((s) => s.recordings)
  const recordingsLoaded = useComputerRecorderStore((s) => s.recordingsLoaded)
  const loadRecordings = useComputerRecorderStore((s) => s.loadRecordings)
  const removeRecording = useComputerRecorderStore((s) => s.removeRecording)
  const renameRecording = useComputerRecorderStore((s) => s.renameRecording)
  const generateForRecording = useComputerRecorderStore((s) => s.generateForRecording)
  const generatingNames = useComputerRecorderStore((s) => s.generatingNames)
  const recorderStatus = useComputerRecorderStore((s) => s.status)
  const recorderBusy = recorderStatus === 'recording'

  const provider = useComputerUseStore((s) => s.provider)
  const model = useComputerUseStore((s) => s.model)
  const start = useComputerUseStore((s) => s.start)

  const [expanded, setExpanded] = useState<string | null>(null)
  const [inputs, setInputs] = useState('')
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState('')

  const submitRename = (name: string) => {
    const next = renameValue.trim()
    if (!next || next === name) {
      setRenaming(null)
      return
    }
    void renameRecording(name, next).then((ok) => {
      if (ok) setRenaming(null)
    })
  }

  useEffect(() => {
    void loadRecordings()
  }, [loadRecordings])

  const hasModel = Boolean(provider && model)

  const smartReplay = (name: string) => {
    if (!hasModel || recorderBusy) return
    start({ replayRecording: name, smart: true })
    setInputs('')
    setExpanded(null)
    onClose()
    void enterMinimalMode('computer')
  }

  const skillReplay = (name: string) => {
    if (!hasModel || recorderBusy) return
    start({ skill: name, taskOverride: inputs })
    setInputs('')
    setExpanded(null)
    onClose()
    void enterMinimalMode('computer')
  }

  const exactReplay = (name: string) => {
    if (recorderBusy) return
    start({ replayRecording: name })
    onClose()
    void enterMinimalMode('computer')
  }

  return (
    <div className="absolute inset-0 z-30 flex justify-end bg-black/30" onClick={onClose}>
      <div
        className="flex h-full w-[min(440px,92vw)] flex-col border-l border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-4 py-3">
          <span className="material-symbols-outlined text-[20px] text-[var(--color-brand)]">
            auto_awesome_motion
          </span>
          <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
            {t('computerUse.skills.title')}
          </div>
          <div className="ml-auto flex items-center gap-1">
            <button
              type="button"
              onClick={() => void loadRecordings()}
              className="inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              aria-label={t('computerUse.skills.refresh')}
            >
              <span className="material-symbols-outlined text-[16px]">refresh</span>
            </button>
            <button
              type="button"
              onClick={onClose}
              className="inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
              aria-label={t('computerUse.skills.close')}
            >
              <span className="material-symbols-outlined text-[16px]">close</span>
            </button>
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          {recordingsLoaded && recordings.length === 0 ? (
            <div className="px-3 py-10 text-center text-[12px] leading-relaxed text-[var(--color-text-secondary)]">
              {t('computerUse.skills.empty')}
            </div>
          ) : (
            <ul className="flex flex-col gap-2">
              {recordings.map((rec) => {
                const isExpanded = expanded === rec.name
                const isGeneratingThis = generatingNames.includes(rec.name)
                return (
                  <li
                    key={rec.name}
                    className="rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] p-3"
                  >
                    <div className="flex items-start gap-2">
                      <div className="min-w-0 flex-1">
                        {renaming === rec.name ? (
                          <div className="flex items-center gap-1">
                            <input
                              type="text"
                              value={renameValue}
                              onChange={(e) => setRenameValue(e.target.value)}
                              placeholder={t('computerUse.skills.renamePlaceholder')}
                              autoFocus
                              onKeyDown={(e) => {
                                if (e.key === 'Enter') submitRename(rec.name)
                                if (e.key === 'Escape') setRenaming(null)
                              }}
                              className="min-w-0 flex-1 rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
                            />
                            <button
                              type="button"
                              onClick={() => submitRename(rec.name)}
                              className="inline-flex items-center justify-center rounded-md bg-[var(--color-brand)] px-2 py-1 text-[10px] font-medium text-white transition-opacity hover:opacity-90"
                            >
                              {t('common.save')}
                            </button>
                            <button
                              type="button"
                              onClick={() => setRenaming(null)}
                              className="inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[10px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                            >
                              {t('common.cancel')}
                            </button>
                          </div>
                        ) : (
                          <div className="flex items-center gap-1">
                            <div className="truncate text-[13px] font-medium text-[var(--color-text-primary)]">
                              {rec.name}
                            </div>
                            <button
                              type="button"
                              onClick={() => {
                                setRenaming(rec.name)
                                setRenameValue(rec.name)
                                setConfirmDelete(null)
                              }}
                              className="inline-flex shrink-0 items-center justify-center rounded-md px-1 py-0.5 text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-brand)]"
                              aria-label={t('computerUse.skills.rename')}
                              title={t('computerUse.skills.rename')}
                            >
                              <span className="material-symbols-outlined text-[14px]">edit</span>
                            </button>
                          </div>
                        )}
                        {rec.task && (
                          <div className="mt-0.5 line-clamp-2 text-[11px] text-[var(--color-text-secondary)]">
                            {rec.task}
                          </div>
                        )}
                        <div className="mt-1 flex flex-wrap items-center gap-2 text-[10px] text-[var(--color-text-tertiary)]">
                          <span>{t('computerUse.skills.steps', { count: rec.step_count })}</span>
                          {!rec.has_skill && (
                            <span className="rounded bg-amber-500/15 px-1.5 py-0.5 text-amber-600 dark:text-amber-400">
                              {t('computerUse.skills.notGenerated')}
                            </span>
                          )}
                        </div>
                      </div>
                      {confirmDelete === rec.name ? (
                        <div className="flex shrink-0 items-center gap-1">
                          <button
                            type="button"
                            onClick={() => {
                              setConfirmDelete(null)
                              void removeRecording(rec.name)
                            }}
                            className="inline-flex items-center justify-center rounded-md bg-red-500 px-2 py-1 text-[10px] font-medium text-white transition-opacity hover:opacity-90"
                          >
                            {t('computerUse.skills.deleteConfirmYes')}
                          </button>
                          <button
                            type="button"
                            onClick={() => setConfirmDelete(null)}
                            className="inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[10px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                          >
                            {t('common.cancel')}
                          </button>
                        </div>
                      ) : (
                        <button
                          type="button"
                          onClick={() => setConfirmDelete(rec.name)}
                          className="inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-1.5 py-1 text-[var(--color-text-tertiary)] transition-colors hover:border-red-500/50 hover:text-red-500"
                          aria-label={t('computerUse.skills.delete')}
                          title={t('computerUse.skills.deleteConfirm')}
                        >
                          <span className="material-symbols-outlined text-[15px]">delete</span>
                        </button>
                      )}
                    </div>

                    <div className="mt-2 flex flex-wrap gap-2">
                      <button
                        type="button"
                        onClick={() => smartReplay(rec.name)}
                        disabled={!hasModel || rec.step_count === 0 || recorderBusy}
                        title={t('computerUse.skills.smartReplayHint')}
                        className="inline-flex items-center gap-1 rounded-md bg-[var(--color-brand)] px-2.5 py-1.5 text-[11px] font-medium text-white transition-opacity hover:opacity-90 disabled:opacity-50"
                      >
                        <span className="material-symbols-outlined text-[14px]">
                          auto_awesome
                        </span>
                        {t('computerUse.skills.smartReplay')}
                      </button>
                      <button
                        type="button"
                        onClick={() => exactReplay(rec.name)}
                        disabled={rec.step_count === 0 || recorderBusy}
                        title={t('computerUse.skills.exactReplayHint')}
                        className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:bg-black/[0.06] disabled:opacity-50 dark:hover:bg-white/[0.08]"
                      >
                        <span className="material-symbols-outlined text-[14px]">replay</span>
                        {t('computerUse.skills.exactReplay')}
                      </button>
                      {rec.has_skill ? (
                        <button
                          type="button"
                          onClick={() => setExpanded(isExpanded ? null : rec.name)}
                          disabled={!hasModel || recorderBusy}
                          title={t('computerUse.skills.variableReplayHint')}
                          className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:bg-black/[0.06] disabled:opacity-50 dark:hover:bg-white/[0.08]"
                        >
                          <span className="material-symbols-outlined text-[14px]">tune</span>
                          {t('computerUse.skills.variableReplay')}
                        </button>
                      ) : (
                        <button
                          type="button"
                          onClick={() => void generateForRecording(rec.name)}
                          disabled={!hasModel || recorderBusy || isGeneratingThis}
                          title={t('computerUse.skills.generateHint')}
                          className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2.5 py-1.5 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:bg-black/[0.06] disabled:opacity-50 dark:hover:bg-white/[0.08]"
                        >
                          {isGeneratingThis ? (
                            <span className="material-symbols-outlined animate-spin text-[14px]">
                              progress_activity
                            </span>
                          ) : (
                            <span className="material-symbols-outlined text-[14px]">
                              auto_awesome
                            </span>
                          )}
                          {isGeneratingThis
                            ? t('computerUse.record.generating')
                            : t('computerUse.skills.generate')}
                        </button>
                      )}
                    </div>

                    {isExpanded && (
                      <div className="mt-2 flex flex-col gap-2 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] p-2">
                        <label className="text-[11px] text-[var(--color-text-secondary)]">
                          {t('computerUse.skills.replayInputs')}
                        </label>
                        <textarea
                          value={inputs}
                          onChange={(e) => setInputs(e.target.value)}
                          placeholder={t('computerUse.skills.replayInputsPlaceholder')}
                          rows={2}
                          className="resize-none rounded-md border border-[var(--color-border)] bg-[var(--color-background)] px-2 py-1.5 text-[11px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
                        />
                        <button
                          type="button"
                          onClick={() => skillReplay(rec.name)}
                          disabled={!hasModel || recorderBusy}
                          className="inline-flex items-center justify-center gap-1 rounded-md bg-[var(--color-brand)] px-2.5 py-1.5 text-[11px] font-semibold text-white transition-opacity hover:opacity-90 disabled:opacity-50"
                        >
                          <span className="material-symbols-outlined text-[14px]">play_arrow</span>
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
      </div>
    </div>
  )
}

export default SkillLibrary
