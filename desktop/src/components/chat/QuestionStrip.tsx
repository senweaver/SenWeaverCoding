// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useChatStore, isAskQuestionToolName } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation } from '../../i18n'

type RawOption = {
  id?: string
  label?: string
  text?: string
  description?: string
}

type RawQuestion = {
  id?: string
  question?: string
  prompt?: string
  header?: string
  options?: Array<RawOption | string>
  allow_multiple?: boolean
}

type QuestionOption = {
  id: string
  label: string
  description?: string
}

type Question = {
  id: string
  prompt: string
  header?: string
  options: QuestionOption[]
  allowMultiple: boolean
}

const LETTERS = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J']

function normalizeQuestions(input: unknown): Question[] {
  if (!input || typeof input !== 'object') return []
  const obj = input as Record<string, unknown>
  const raw = Array.isArray(obj.questions)
    ? (obj.questions as RawQuestion[])
    : typeof obj.question === 'string' || typeof obj.prompt === 'string'
      ? [obj as RawQuestion]
      : []
  return raw
    .map((q, idx): Question | null => {
      const prompt = (q.prompt ?? q.question ?? '').trim()
      if (!prompt) return null
      const optsRaw = Array.isArray(q.options) ? q.options : []
      const options: QuestionOption[] = optsRaw
        .map((opt, optIdx): QuestionOption | null => {
          if (typeof opt === 'string') {
            return { id: `opt-${optIdx}`, label: opt }
          }
          if (opt && typeof opt === 'object') {
            const o = opt as RawOption
            const label =
              (typeof o.label === 'string' && o.label) ||
              (typeof o.text === 'string' && o.text) ||
              (typeof o.id === 'string' && o.id) ||
              ''
            if (!label) return null
            const description =
              typeof o.description === 'string' && o.description ? o.description : undefined
            return {
              id: typeof o.id === 'string' && o.id ? o.id : `opt-${optIdx}`,
              label,
              ...(description ? { description } : {}),
            }
          }
          return null
        })
        .filter((o): o is QuestionOption => !!o)
      const header = typeof q.header === 'string' ? q.header : undefined
      const built: Question = {
        id: typeof q.id === 'string' && q.id ? q.id : `q-${idx}`,
        prompt,
        options,
        allowMultiple: !!q.allow_multiple,
        ...(header ? { header } : {}),
      }
      return built
    })
    .filter((q): q is Question => q !== null)
}

export function QuestionStrip() {
  const t = useTranslation()
  const activeTabId = useTabStore((s) => s.activeTabId)
  const respondToPermission = useChatStore((s) => s.respondToPermission)
  const pendingPermission = useChatStore((s) =>
    activeTabId ? s.sessions[activeTabId]?.pendingPermission ?? null : null,
  )
  const isQuestion = isAskQuestionToolName(pendingPermission?.toolName)

  const questions = useMemo(
    () => (isQuestion && pendingPermission ? normalizeQuestions(pendingPermission.input) : []),
    [isQuestion, pendingPermission],
  )

  const [activeIdx, setActiveIdx] = useState(0)
  const [selections, setSelections] = useState<Record<string, string[]>>({})
  const submittedRef = useRef(false)

  useEffect(() => {
    setActiveIdx(0)
    setSelections({})
    submittedRef.current = false
  }, [pendingPermission?.requestId])

  useEffect(() => {
    if (!isQuestion || !pendingPermission) return
    const keyHandler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        handleSkip()
      } else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault()
        handleContinue()
      }
    }
    const submitHandler = () => {
      handleContinue()
    }
    window.addEventListener('keydown', keyHandler)
    window.addEventListener('plan:question:submit', submitHandler as EventListener)
    return () => {
      window.removeEventListener('keydown', keyHandler)
      window.removeEventListener('plan:question:submit', submitHandler as EventListener)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [isQuestion, pendingPermission?.requestId, selections, activeIdx])

  if (!isQuestion || !pendingPermission || questions.length === 0) return null

  const activeQuestion = questions[Math.min(activeIdx, questions.length - 1)]
  if (!activeQuestion) return null

  const allAnswered = questions.every((q) => (selections[q.id]?.length ?? 0) > 0)
  const anyAnswered = questions.some((q) => (selections[q.id]?.length ?? 0) > 0)

  const inputObject =
    pendingPermission.input && typeof pendingPermission.input === 'object'
      ? (pendingPermission.input as Record<string, unknown>)
      : {}

  function readDetailsFromComposer(): string {
    const el = document.querySelector<HTMLElement>('[data-role="chat-composer"]')
    if (!el) return ''
    if (el instanceof HTMLTextAreaElement) return el.value.trim()
    return (el.textContent ?? '').trim()
  }

  function clearComposer() {
    const el = document.querySelector<HTMLElement>('[data-role="chat-composer"]')
    if (!el) return
    if (el instanceof HTMLTextAreaElement) {
      const setter = Object.getOwnPropertyDescriptor(
        Object.getPrototypeOf(el),
        'value',
      )?.set
      setter?.call(el, '')
      el.dispatchEvent(new Event('input', { bubbles: true }))
      return
    }
    el.textContent = ''
    el.dispatchEvent(new Event('input', { bubbles: true }))
  }

  function toggleOption(question: Question, optId: string) {
    setSelections((prev) => {
      const current = prev[question.id] ?? []
      if (question.allowMultiple) {
        const exists = current.includes(optId)
        const next = exists ? current.filter((x) => x !== optId) : [...current, optId]
        return { ...prev, [question.id]: next }
      }
      return { ...prev, [question.id]: [optId] }
    })
  }

  function buildAnswerPayload(skipped: boolean) {
    const answers: Record<string, string | string[]> = {}
    if (!skipped) {
      for (const q of questions) {
        const optIds = selections[q.id] ?? []
        if (optIds.length === 0) continue
        const labels = optIds
          .map((id) => q.options.find((o) => o.id === id)?.label ?? id)
          .filter((label): label is string => typeof label === 'string' && label.length > 0)
        if (labels.length === 0) continue
        answers[q.id] = q.allowMultiple ? labels : (labels[0] ?? '')
      }
    }
    const details = readDetailsFromComposer()
    return {
      ...inputObject,
      answers,
      ...(details ? { details } : {}),
      ...(skipped ? { skipped: true } : {}),
    }
  }

  function handleSkip() {
    if (!activeTabId || !pendingPermission || submittedRef.current) return
    submittedRef.current = true
    const ok = respondToPermission(activeTabId, pendingPermission.requestId, true, {
      updatedInput: buildAnswerPayload(true),
    })
    if (!ok) {
      submittedRef.current = false
      return
    }
    clearComposer()
  }

  function handleContinue() {
    if (!activeTabId || !pendingPermission || submittedRef.current) return
    if (!anyAnswered) {
      const details = readDetailsFromComposer()
      if (!details) return
    }
    submittedRef.current = true
    const ok = respondToPermission(activeTabId, pendingPermission.requestId, true, {
      updatedInput: buildAnswerPayload(false),
    })
    if (!ok) {
      submittedRef.current = false
      return
    }
    clearComposer()
  }

  const activeSelections = selections[activeQuestion.id] ?? []

  return (
    <div className="shrink-0 px-8">
      <div className="mx-auto max-w-[860px] mb-2 rounded-[var(--radius-lg)] border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] overflow-hidden shadow-[var(--shadow-dropdown)]">
        <div className="flex items-center gap-2 border-b border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)] px-3 py-2">
          <span className="material-symbols-outlined text-[16px] text-[var(--color-text-secondary)]">
            quiz
          </span>
          <span className="text-[12px] font-semibold text-[var(--color-text-primary)]">
            {t('plan.questionsTitle')}
          </span>
          <div className="ml-auto flex items-center gap-1.5">
            {questions.length > 1 && (
              <>
                <button
                  type="button"
                  onClick={() => setActiveIdx((i) => Math.max(0, i - 1))}
                  disabled={activeIdx === 0}
                  className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-30"
                >
                  <span className="material-symbols-outlined text-[14px]">expand_less</span>
                </button>
                <span className="text-[11px] text-[var(--color-text-tertiary)] tabular-nums">
                  {t('plan.questionPagination', {
                    current: activeIdx + 1,
                    total: questions.length,
                  })}
                </span>
                <button
                  type="button"
                  onClick={() => setActiveIdx((i) => Math.min(questions.length - 1, i + 1))}
                  disabled={activeIdx === questions.length - 1}
                  className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-30"
                >
                  <span className="material-symbols-outlined text-[14px]">expand_more</span>
                </button>
              </>
            )}
          </div>
        </div>

        <div className="px-3 py-2.5">
          <div className="text-[12px] font-semibold text-[var(--color-text-primary)] mb-2 flex flex-wrap items-baseline gap-x-1.5 gap-y-0.5">
            <span className="text-[var(--color-text-tertiary)]">{activeIdx + 1}.</span>
            <span>{activeQuestion.prompt}</span>
            {activeQuestion.allowMultiple && (
              <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-[var(--color-plan-accent-container)] text-[var(--color-on-plan-accent-container)] uppercase tracking-wide">
                {t('plan.multiSelectHint')}
              </span>
            )}
          </div>
          <div className="flex flex-col gap-1">
            {activeQuestion.options.map((opt, idx) => {
              const isSelected = activeSelections.includes(opt.id)
              const letter = LETTERS[idx] ?? String(idx + 1)
              const indicator = activeQuestion.allowMultiple ? (
                <span
                  className={`material-symbols-outlined text-[14px] ${
                    isSelected
                      ? 'text-[var(--color-on-plan-accent-container)]'
                      : 'text-[var(--color-text-secondary)]'
                  }`}
                >
                  {isSelected ? 'check_box' : 'check_box_outline_blank'}
                </span>
              ) : (
                <span className="text-[10px] font-bold uppercase">{letter}</span>
              )
              return (
                <button
                  key={opt.id}
                  type="button"
                  onClick={() => toggleOption(activeQuestion, opt.id)}
                  className={`flex items-start gap-2 rounded-[var(--radius-md)] px-2 py-1.5 text-left transition-colors ${
                    isSelected
                      ? 'bg-[var(--color-plan-accent-container)] text-[var(--color-on-plan-accent-container)]'
                      : 'hover:bg-[var(--color-surface-container-low)] text-[var(--color-text-primary)]'
                  }`}
                >
                  <span
                    className={`flex h-4 w-4 shrink-0 items-center justify-center rounded ${
                      isSelected
                        ? 'bg-[var(--color-plan-accent)] text-[var(--color-on-plan-accent-container)]'
                        : 'bg-[var(--color-surface-container)] text-[var(--color-text-secondary)]'
                    }`}
                  >
                    {indicator}
                  </span>
                  <span className="text-[12px] leading-snug">
                    <span className="font-semibold mr-1">{letter}.</span>
                    {opt.label}
                    {opt.description && (
                      <span className="ml-1 text-[var(--color-text-tertiary)]">
                        — {opt.description}
                      </span>
                    )}
                  </span>
                </button>
              )
            })}
          </div>
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)] px-3 py-1.5">
          <button
            type="button"
            onClick={handleSkip}
            className="flex items-center gap-1 rounded-[var(--radius-md)] px-2 py-1 text-[11px] font-medium text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] transition-colors"
          >
            {t('plan.skip')}
            <span className="text-[10px] text-[var(--color-text-tertiary)] tabular-nums px-1 py-0.5 rounded bg-[var(--color-surface-container)]">
              {t('plan.skipKey')}
            </span>
          </button>
          <button
            type="button"
            onClick={handleContinue}
            disabled={!allAnswered && !anyAnswered}
            className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-plan-accent)] text-[var(--color-on-plan-accent-container)] hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed transition-all"
          >
            {t('plan.continue')}
            <span className="text-[10px] px-1 py-0.5 rounded bg-[var(--color-plan-accent-hover)]/20">
              {t('plan.continueKey')}
            </span>
          </button>
        </div>
      </div>
    </div>
  )
}
