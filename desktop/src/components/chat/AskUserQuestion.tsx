// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { focusSession } from '../../lib/focusSession'
import { useTranslation } from '../../i18n'
import { Button } from '../shared/Button'

type QuestionOption = {
  id?: string
  label: string
  description?: string
}

type Question = {
  id: string
  question: string
  header?: string
  options?: QuestionOption[]
  allowMultiple: boolean
}

type Props = {
  toolUseId: string
  input: unknown
  result?: unknown
  sessionId?: string | null
}

function parseInput(input: unknown): Question[] {
  if (!input || typeof input !== 'object') return []
  const obj = input as Record<string, unknown>

  const normalizeOption = (opt: unknown): QuestionOption | null => {
    if (typeof opt === 'string') return { label: opt }
    if (opt && typeof opt === 'object') {
      const o = opt as Record<string, unknown>
      const label =
        typeof o.label === 'string'
          ? o.label
          : typeof o.text === 'string'
            ? (o.text as string)
            : typeof o.id === 'string'
              ? (o.id as string)
              : ''
      const id = typeof o.id === 'string' ? (o.id as string) : undefined
      const description = typeof o.description === 'string' ? (o.description as string) : undefined
      if (!label) return null
      return { id, label, ...(description ? { description } : {}) }
    }
    return null
  }

  const normalizeOptions = (raw: unknown): QuestionOption[] | undefined => {
    if (!Array.isArray(raw)) return undefined
    const out = raw.map(normalizeOption).filter((x): x is QuestionOption => !!x)
    return out.length > 0 ? out : undefined
  }

  const fromQuestionLike = (q: unknown, idx: number): Question | null => {
    if (!q || typeof q !== 'object') return null
    const o = q as Record<string, unknown>
    const text =
      typeof o.question === 'string'
        ? (o.question as string)
        : typeof o.prompt === 'string'
          ? (o.prompt as string)
          : ''
    if (!text.trim()) return null
    const id = typeof o.id === 'string' && o.id ? (o.id as string) : `q-${idx}`
    const header = typeof o.header === 'string' ? (o.header as string) : undefined
    const options = normalizeOptions(o.options)
    const allowMultiple = o.allow_multiple === true || o.allowMultiple === true
    return { id, question: text, header, options, allowMultiple }
  }

  if (Array.isArray(obj.questions)) {
    return obj.questions
      .map((q, i) => fromQuestionLike(q, i))
      .filter((q): q is Question => !!q)
  }

  if (typeof obj.question === 'string' || typeof obj.prompt === 'string') {
    const single = fromQuestionLike(obj, 0)
    return single ? [single] : []
  }

  return []
}

function readDetailsFromComposer(): string {
  if (typeof document === 'undefined') return ''
  const el = document.querySelector<HTMLElement>('[data-role="chat-composer"]')
  if (!el) return ''
  if (el instanceof HTMLTextAreaElement) return el.value.trim()
  return (el.textContent ?? '').trim()
}

function clearComposer() {
  if (typeof document === 'undefined') return
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

export function AskUserQuestion({ toolUseId, input, result, sessionId: ownerSessionIdProp }: Props) {
  const respondToPermission = useChatStore((s) => s.respondToPermission)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const ownerSessionId = ownerSessionIdProp ?? activeTabId
  const pendingPermission = useChatStore((s) =>
    ownerSessionId ? s.sessions[ownerSessionId]?.pendingPermission : undefined,
  )
  const isCrossSession = Boolean(ownerSessionId && ownerSessionId !== activeTabId)
  const t = useTranslation()
  const questions = useMemo(() => parseInput(input), [input])
  const inputObject = (input && typeof input === 'object') ? input as Record<string, unknown> : {}
  const [activeTab, setActiveTab] = useState(0)
  const [selections, setSelections] = useState<Record<number, string[]>>({})
  const [hasSubmitted, setHasSubmitted] = useState(false)

  const pendingRequest = pendingPermission?.toolUseId === toolUseId ? pendingPermission : null
  const submitted = hasSubmitted
  const muted = submitted
  const canInteract = !submitted && !isCrossSession

  const anyAnswered = Object.values(selections).some((arr) => arr.length > 0)
  const allAnswered = questions.every((_, i) => (selections[i]?.length ?? 0) > 0)

  const submit = (skipped: boolean) => {
    if (submitted || isCrossSession) return
    if (!ownerSessionId || !pendingRequest) return

    const answers: Record<string, string | string[]> = {}
    if (!skipped) {
      questions.forEach((question, index) => {
        const labels = selections[index] ?? []
        if (labels.length === 0) return
        answers[question.id] = question.allowMultiple ? labels : (labels[0] ?? '')
      })
    }
    const details = readDetailsFromComposer()

    if (!skipped && Object.keys(answers).length === 0 && !details) return

    setHasSubmitted(true)
    const ok = respondToPermission(ownerSessionId, pendingRequest.requestId, true, {
      updatedInput: {
        ...inputObject,
        answers,
        ...(details ? { details } : {}),
        ...(skipped ? { skipped: true } : {}),
      },
    })
    if (!ok) {
      setHasSubmitted(false)
      return
    }
    clearComposer()
  }

  useEffect(() => {
    if (submitted || isCrossSession || !pendingRequest) return
    const onSubmit = () => submit(false)
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      if (e.defaultPrevented || e.isComposing) return
      const target = e.target as HTMLElement | null
      if (target) {
        const tag = target.tagName
        if (
          tag === 'INPUT' ||
          tag === 'TEXTAREA' ||
          target.isContentEditable ||
          target.closest('[role="dialog"], [data-modal-root]')
        ) {
          return
        }
      }
      if (document.querySelector('[role="dialog"]')) return
      e.preventDefault()
      submit(true)
    }
    window.addEventListener('plan:question:submit', onSubmit as EventListener)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('plan:question:submit', onSubmit as EventListener)
      window.removeEventListener('keydown', onKey)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [submitted, isCrossSession, pendingRequest?.requestId, selections, questions, ownerSessionId])

  const resultText = useMemo(() => {
    if (typeof result === 'string') return result
    if (result && typeof result === 'object') {
      const out = (result as { output?: unknown }).output
      if (typeof out === 'string') return out
    }
    return ''
  }, [result])

  if (questions.length === 0) {
    const rawInput =
      typeof input === 'string' ? input : JSON.stringify(input, null, 2)
    return (
      <div className="mb-4 overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-warning)]/40 bg-[var(--color-surface-container-low)]">
        <div className="flex items-center gap-2 px-4 py-3">
          <span className="material-symbols-outlined text-[18px] text-[var(--color-warning)]">
            help
          </span>
          <span className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t('question.unparsableTitle')}
          </span>
        </div>
        <div className="border-t border-[var(--color-outline-variant)]/20 px-4 py-3">
          <pre className="max-h-[220px] overflow-auto rounded-[var(--radius-md)] bg-[var(--color-terminal-bg)] px-3 py-2.5 font-[var(--font-mono)] text-[11px] leading-[1.3] text-[var(--color-terminal-fg)] whitespace-pre-wrap break-words">
            {rawInput || resultText}
          </pre>
        </div>
        {pendingRequest && !submitted && (
          <div className="flex items-center gap-2 border-t border-[var(--color-outline-variant)]/20 bg-[var(--color-surface-container-low)] px-4 py-3">
            {isCrossSession ? (
              <Button
                variant="primary"
                size="sm"
                onClick={() => ownerSessionId && focusSession(ownerSessionId)}
                icon={<span className="material-symbols-outlined text-[14px]">arrow_forward</span>}
              >
                {t('permission.switchToSession')}
              </Button>
            ) : (
              <Button
                variant="primary"
                size="sm"
                onClick={() =>
                  ownerSessionId &&
                  pendingRequest &&
                  respondToPermission(ownerSessionId, pendingRequest.requestId, true)
                }
                icon={<span className="material-symbols-outlined text-[14px]">check</span>}
              >
                {t('plan.continue')}
              </Button>
            )}
          </div>
        )}
      </div>
    )
  }

  const handleSelect = (qIndex: number, opt: QuestionOption) => {
    if (!canInteract) return
    const allowMultiple = questions[qIndex]?.allowMultiple ?? false
    setSelections((prev) => {
      const current = prev[qIndex] ?? []
      if (allowMultiple) {
        const exists = current.includes(opt.label)
        const next = exists
          ? current.filter((x) => x !== opt.label)
          : [...current, opt.label]
        return { ...prev, [qIndex]: next }
      }
      if (current.length === 1 && current[0] === opt.label) {
        const next = { ...prev }
        delete next[qIndex]
        return next
      }
      return { ...prev, [qIndex]: [opt.label] }
    })
  }

  const safeActiveTab = Math.min(activeTab, questions.length - 1)
  const activeQuestion = questions[safeActiveTab]
  if (!activeQuestion) return null

  return (
    <div className={`mb-4 rounded-[var(--radius-lg)] border overflow-hidden ${
      muted
        ? 'border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-low)] opacity-80'
        : 'border-[var(--color-secondary)] bg-[var(--color-surface-container-lowest)]'
    }`}>
      <div className={`flex items-center gap-3 px-4 py-3 ${
        muted ? 'bg-[var(--color-surface-container-low)]' : 'bg-[var(--color-surface-container)]'
      }`}>
        <div className="flex items-center justify-center w-8 h-8 rounded-[var(--radius-md)] bg-[var(--color-secondary)]/10">
          <span className="material-symbols-outlined text-[18px] text-[var(--color-secondary)]">
            help
          </span>
        </div>
        <div className="flex-1 min-w-0">
          <span className="text-sm font-semibold text-[var(--color-text-primary)]">
            {t('question.needsInput')}
          </span>
          {muted && (
            <span className="ml-2 inline-flex items-center px-2 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)]">
              {t('question.answered')}
            </span>
          )}
        </div>
      </div>

      {questions.length > 1 && (
        <div className="flex px-4 border-b border-[var(--color-outline-variant)]/20 bg-[var(--color-surface-container-low)] overflow-x-auto">
          {questions.map((q, i) => {
            const isActive = safeActiveTab === i
            const isAnswered = (selections[i]?.length ?? 0) > 0
            const tabLabel = q.header || `Q${i + 1}`
            return (
              <button
                key={i}
                onClick={() => setActiveTab(i)}
                className={`relative flex items-center gap-1.5 px-4 py-2.5 text-xs font-medium whitespace-nowrap transition-colors ${
                  isActive
                    ? 'text-[var(--color-secondary)]'
                    : 'text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]'
                }`}
              >
                {isAnswered && (
                  <span className="material-symbols-outlined text-[14px] text-[var(--color-success)]">check_circle</span>
                )}
                {tabLabel}
                {isActive && (
                  <div className="absolute bottom-0 left-2 right-2 h-[2px] bg-[var(--color-secondary)] rounded-t" />
                )}
              </button>
            )
          })}
        </div>
      )}

      <div className="px-4 py-3">
        <p className="text-sm font-medium text-[var(--color-text-primary)] mb-1 flex flex-wrap items-baseline gap-x-2">
          <span>{activeQuestion.question}</span>
          {activeQuestion.allowMultiple && !muted && (
            <span className="text-[10px] font-medium px-1.5 py-0.5 rounded bg-[var(--color-secondary)]/12 text-[var(--color-secondary)] uppercase tracking-wide">
              {t('plan.multiSelectHint')}
            </span>
          )}
        </p>

        {activeQuestion.options && activeQuestion.options.length > 0 && (
          <div className="space-y-2 mt-3">
            {activeQuestion.options.map((opt, optIndex) => {
              const isSelected = (selections[safeActiveTab] ?? []).includes(opt.label)
              return (
                <button
                  key={opt.id ?? optIndex}
                  onClick={() => handleSelect(safeActiveTab, opt)}
                  disabled={!canInteract}
                  className={`w-full text-left px-4 py-3 rounded-[var(--radius-md)] border transition-all duration-150 ${
                    canInteract ? 'cursor-pointer' : 'cursor-default'
                  } ${
                    isSelected
                      ? 'border-[var(--color-secondary)] bg-[var(--color-secondary)]/8 ring-1 ring-[var(--color-secondary)]/30'
                      : 'border-[var(--color-outline-variant)]/40 bg-[var(--color-surface)] hover:border-[var(--color-outline-variant)] hover:bg-[var(--color-surface-container-low)]'
                  }`}
                >
                  <div className="flex items-start gap-3">
                    <div className={`mt-0.5 flex-shrink-0 w-4 h-4 border-2 flex items-center justify-center transition-colors ${
                      activeQuestion.allowMultiple ? 'rounded-[4px]' : 'rounded-full'
                    } ${
                      isSelected
                        ? 'border-[var(--color-secondary)] bg-[var(--color-secondary)]'
                        : 'border-[var(--color-outline)]'
                    }`}>
                      {isSelected && (
                        <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                          <polyline points="20 6 9 17 4 12" />
                        </svg>
                      )}
                    </div>
                    <div className="flex-1 min-w-0">
                      <span className={`text-sm font-medium ${
                        isSelected
                          ? 'text-[var(--color-secondary)]'
                          : 'text-[var(--color-text-primary)]'
                      }`}>
                        {opt.label}
                      </span>
                      {opt.description && (
                        <p className="text-xs text-[var(--color-text-secondary)] mt-0.5">
                          {opt.description}
                        </p>
                      )}
                    </div>
                  </div>
                </button>
              )
            })}
          </div>
        )}

      </div>

      {!submitted && (
        <div className="flex items-center justify-between gap-2 px-4 py-3 border-t border-[var(--color-outline-variant)]/20 bg-[var(--color-surface-container-low)]">
          {isCrossSession ? (
            <Button
              variant="primary"
              size="sm"
              onClick={() => ownerSessionId && focusSession(ownerSessionId)}
              icon={<span className="material-symbols-outlined text-[14px]">arrow_forward</span>}
            >
              {t('permission.switchToSession')}
            </Button>
          ) : (
            <>
              {pendingRequest ? (
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => submit(true)}
                >
                  {t('plan.skip')}
                </Button>
              ) : (
                <span />
              )}
              <Button
                variant="primary"
                size="sm"
                disabled={(!allAnswered && !anyAnswered) || !pendingRequest}
                onClick={() => submit(false)}
                icon={<span className="material-symbols-outlined text-[14px]">send</span>}
              >
                {t('question.submit')}
              </Button>
            </>
          )}
        </div>
      )}
    </div>
  )
}
