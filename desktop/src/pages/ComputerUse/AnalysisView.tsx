// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useComputerAnalysisStore } from '../../stores/computerAnalysisStore'
import type { AnalysisStep } from '../../api/computer'
import { SensitiveReviewCard } from './SensitiveReviewCard'
import { BuilderView } from './BuilderView'

export function AnalysisView({ name, onClose }: { name: string; onClose: () => void }) {
  const t = useTranslation()
  const open = useComputerAnalysisStore((s) => s.open)
  const close = useComputerAnalysisStore((s) => s.close)
  const analysis = useComputerAnalysisStore((s) => s.analysis)
  const sensitiveReport = useComputerAnalysisStore((s) => s.sensitiveReport)
  const analyzePhase = useComputerAnalysisStore((s) => s.analyzePhase)
  const analyzeMessage = useComputerAnalysisStore((s) => s.analyzeMessage)
  const analyzeError = useComputerAnalysisStore((s) => s.analyzeError)
  const startAnalyze = useComputerAnalysisStore((s) => s.startAnalyze)
  const cancelAnalyze = useComputerAnalysisStore((s) => s.cancelAnalyze)
  const sendFeedback = useComputerAnalysisStore((s) => s.sendFeedback)
  const persistAnalysisEdit = useComputerAnalysisStore((s) => s.persistAnalysisEdit)
  const approveAnalysis = useComputerAnalysisStore((s) => s.approveAnalysis)

  const [feedback, setFeedback] = useState('')
  const [showBuilder, setShowBuilder] = useState(false)

  useEffect(() => {
    void open(name)
    return () => close()
  }, [name, open, close])

  const running = analyzePhase === 'running'
  const narrationStale = useMemo(() => {
    if (!analysis) return false
    const updated = analysis.narrationSourceUpdatedAt
    return updated !== null && updated > analysis.createdAt
  }, [analysis])

  return (
    <div className="absolute inset-0 z-40 flex justify-center bg-black/40 p-6" onClick={onClose}>
      <div
        className="flex h-full w-[min(720px,96vw)] flex-col overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-4 py-3">
          <span className="material-symbols-outlined text-[20px] text-[var(--color-brand)]">
            insights
          </span>
          <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
            {t('computerUse.analyze.title')} · {name}
          </div>
          <button
            type="button"
            onClick={onClose}
            className="ml-auto inline-flex items-center justify-center rounded-md border border-[var(--color-border)] px-2 py-1 text-[var(--color-text-secondary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
          >
            <span className="material-symbols-outlined text-[16px]">close</span>
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          {sensitiveReport && sensitiveReport.totalFindings > 0 && (
            <SensitiveReviewCard report={sensitiveReport} />
          )}

          {analyzeError && (
            <div className="mb-3 rounded-lg border border-[var(--color-error)]/30 bg-[var(--color-error)]/10 px-3 py-2 text-[12px] text-[var(--color-error)]">
              {analyzeError}
            </div>
          )}

          {!analysis && !running && (
            <div className="flex flex-col items-center gap-3 py-10 text-center">
              <p className="text-[12px] text-[var(--color-text-secondary)]">
                {t('computerUse.analyze.empty')}
              </p>
              <button
                type="button"
                onClick={startAnalyze}
                className="inline-flex items-center gap-1.5 rounded-lg bg-[var(--color-brand)] px-4 py-2 text-[12px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90"
              >
                <span className="material-symbols-outlined text-[16px]">insights</span>
                {t('computerUse.analyze.cta')}
              </button>
            </div>
          )}

          {running && (
            <div className="flex items-center gap-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] px-3 py-3">
              <span className="material-symbols-outlined animate-spin text-[16px] text-[var(--color-brand)]">
                progress_activity
              </span>
              <span className="text-[12px] text-[var(--color-text-secondary)]">
                {analyzeMessage || t('computerUse.analyze.running')}
              </span>
              <button
                type="button"
                onClick={cancelAnalyze}
                className="ml-auto text-[11px] text-[var(--color-text-tertiary)] hover:text-[var(--color-error)]"
              >
                {t('computerUse.analyze.cancel')}
              </button>
            </div>
          )}

          {analysis && !running && (
            <div className="flex flex-col gap-4">
              {narrationStale && (
                <div className="rounded-lg border border-[var(--color-warning)]/30 bg-[var(--color-warning)]/10 px-3 py-2 text-[12px] text-[var(--color-text-primary)]">
                  {t('computerUse.analyze.narrationStale')}
                </div>
              )}

              <IntentCard
                title={analysis.title}
                intent={analysis.intent}
                rationale={analysis.intentRationale}
                onSave={(patch) => void persistAnalysisEdit(patch)}
              />

              <StepList
                steps={analysis.steps}
                onSave={(steps) => void persistAnalysisEdit({ steps })}
              />

              <FeedbackBox
                value={feedback}
                onChange={setFeedback}
                onSend={() => {
                  if (!feedback.trim()) return
                  sendFeedback(feedback.trim(), [])
                  setFeedback('')
                }}
              />

              <div className="flex flex-wrap items-center gap-2 border-t border-[var(--color-border)] pt-3">
                <button
                  type="button"
                  onClick={startAnalyze}
                  className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
                >
                  <span className="material-symbols-outlined text-[14px]">refresh</span>
                  {t('computerUse.analyze.reanalyze')}
                </button>
                <button
                  type="button"
                  onClick={() => void approveAnalysis()}
                  className={`inline-flex items-center gap-1 rounded-md px-3 py-1.5 text-[11px] font-semibold transition-opacity hover:opacity-90 ${
                    analysis.approved
                      ? 'bg-[var(--color-success)]/15 text-[var(--color-success)]'
                      : 'border border-[var(--color-border)] text-[var(--color-text-primary)]'
                  }`}
                >
                  <span className="material-symbols-outlined text-[14px]">check_circle</span>
                  {analysis.approved
                    ? t('computerUse.analyze.approved')
                    : t('computerUse.analyze.approve')}
                </button>
                <button
                  type="button"
                  onClick={() => setShowBuilder(true)}
                  className="ml-auto inline-flex items-center gap-1 rounded-md bg-[var(--color-brand)] px-3 py-1.5 text-[11px] font-semibold text-[var(--color-on-primary)] transition-opacity hover:opacity-90"
                >
                  <span className="material-symbols-outlined text-[14px]">build</span>
                  {t('computerUse.analyze.create')}
                </button>
              </div>
            </div>
          )}
        </div>
      </div>
      {showBuilder && <BuilderView onClose={() => setShowBuilder(false)} />}
    </div>
  )
}

function IntentCard({
  title,
  intent,
  rationale,
  onSave,
}: {
  title: string
  intent: string
  rationale: string
  onSave: (patch: { title?: string; intent?: string }) => void
}) {
  const t = useTranslation()
  const [editing, setEditing] = useState(false)
  const [draftTitle, setDraftTitle] = useState(title)
  const [draftIntent, setDraftIntent] = useState(intent)

  useEffect(() => {
    setDraftTitle(title)
    setDraftIntent(intent)
  }, [title, intent])

  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] p-3">
      <div className="mb-2 flex items-center gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
          {t('computerUse.analyze.intent')}
        </span>
        <button
          type="button"
          onClick={() => {
            if (editing) {
              onSave({ title: draftTitle.trim(), intent: draftIntent.trim() })
            }
            setEditing(!editing)
          }}
          className="ml-auto text-[11px] text-[var(--color-brand)] hover:opacity-80"
        >
          {editing ? t('computerUse.analyze.save') : t('computerUse.analyze.edit')}
        </button>
      </div>
      {editing ? (
        <div className="flex flex-col gap-2">
          <input
            value={draftTitle}
            onChange={(e) => setDraftTitle(e.target.value)}
            placeholder={t('computerUse.analyze.titleLabel')}
            className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[12px] font-semibold text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
          />
          <textarea
            value={draftIntent}
            onChange={(e) => setDraftIntent(e.target.value)}
            placeholder={t('computerUse.analyze.intentPlaceholder')}
            rows={2}
            className="resize-none rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
          />
        </div>
      ) : (
        <div>
          {title && (
            <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">{title}</div>
          )}
          <div className="mt-0.5 text-[12px] text-[var(--color-text-secondary)]">{intent}</div>
          {rationale && (
            <div className="mt-1 text-[11px] italic text-[var(--color-text-tertiary)]">{rationale}</div>
          )}
        </div>
      )}
    </div>
  )
}

function StepList({
  steps,
  onSave,
}: {
  steps: AnalysisStep[]
  onSave: (steps: AnalysisStep[]) => void
}) {
  const t = useTranslation()
  const [local, setLocal] = useState<AnalysisStep[]>(steps)
  const timer = useRef<number | null>(null)

  useEffect(() => {
    setLocal(steps)
  }, [steps])

  const scheduleSave = (next: AnalysisStep[]) => {
    setLocal(next)
    if (timer.current) window.clearTimeout(timer.current)
    timer.current = window.setTimeout(() => onSave(next), 500)
  }

  useEffect(
    () => () => {
      if (timer.current) window.clearTimeout(timer.current)
    },
    [],
  )

  return (
    <div>
      <div className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
        {t('computerUse.analyze.steps')}
      </div>
      <ol className="flex flex-col gap-2">
        {local.map((step, idx) => (
          <li
            key={step.id}
            className="rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] p-2.5"
          >
            <div className="flex items-start gap-2">
              <span className="mt-1 inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full bg-[var(--color-brand)]/12 text-[10px] font-semibold text-[var(--color-brand)]">
                {idx + 1}
              </span>
              <div className="flex min-w-0 flex-1 flex-col gap-1">
                <input
                  value={step.title}
                  onChange={(e) => {
                    const next = [...local]
                    next[idx] = { ...step, title: e.target.value }
                    scheduleSave(next)
                  }}
                  placeholder={t('computerUse.analyze.stepTitle')}
                  className="rounded-md border border-transparent bg-transparent px-1 py-0.5 text-[12px] font-medium text-[var(--color-text-primary)] outline-none hover:border-[var(--color-border)] focus:border-[var(--color-brand)]"
                />
                <textarea
                  value={step.detail}
                  onChange={(e) => {
                    const next = [...local]
                    next[idx] = { ...step, detail: e.target.value }
                    scheduleSave(next)
                  }}
                  placeholder={t('computerUse.analyze.stepDetail')}
                  rows={2}
                  className="resize-none rounded-md border border-transparent bg-transparent px-1 py-0.5 text-[11px] text-[var(--color-text-secondary)] outline-none hover:border-[var(--color-border)] focus:border-[var(--color-brand)]"
                />
                {step.apps.length > 0 && (
                  <div className="text-[10px] text-[var(--color-text-tertiary)]">
                    {step.apps.join(' · ')}
                  </div>
                )}
              </div>
            </div>
          </li>
        ))}
      </ol>
    </div>
  )
}

function FeedbackBox({
  value,
  onChange,
  onSend,
}: {
  value: string
  onChange: (value: string) => void
  onSend: () => void
}) {
  const t = useTranslation()
  return (
    <div className="rounded-lg border border-[var(--color-border)] bg-[var(--color-background)] p-3">
      <div className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
        {t('computerUse.analyze.feedback')}
      </div>
      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={t('computerUse.analyze.feedbackPlaceholder')}
        rows={2}
        className="w-full resize-none rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-brand)]"
      />
      <button
        type="button"
        onClick={onSend}
        disabled={!value.trim()}
        className="mt-2 inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-3 py-1.5 text-[11px] font-medium text-[var(--color-text-primary)] transition-colors hover:bg-black/[0.06] disabled:opacity-50 dark:hover:bg-white/[0.08]"
      >
        <span className="material-symbols-outlined text-[14px]">send</span>
        {t('computerUse.analyze.feedbackSend')}
      </button>
    </div>
  )
}

export default AnalysisView
