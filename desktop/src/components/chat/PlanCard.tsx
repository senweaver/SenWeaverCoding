// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useChatStore } from '../../stores/chatStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import { useUIStore } from '../../stores/uiStore'
import { workspaceFilesApi } from '../../api/workspaceFiles'
import { resolveWorkspaceFile } from '../../lib/workspacePath'
import { useTranslation } from '../../i18n'
import { Modal } from '../shared/Modal'
import { Button } from '../shared/Button'
import { UnsavedChangesDialog } from '../shared/UnsavedChangesDialog'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import {
  selectPlanCardExecutionStateCached,
  type PlanExecutionState,
} from '../../utils/activePlanSelector'

type Todo = {
  id: string
  content: string
  status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
}

type Props = {
  messageId: string
  planPath: string
  fileName: string
  title: string
  overview: string
  todos: Todo[]
  markdown?: string
  modelLabel?: string
  status: 'writing' | 'completed' | 'failed'
  error?: string
  superseded?: boolean
  sessionId?: string | null
}

export function PlanCard({
  messageId,
  planPath,
  fileName,
  title,
  overview,
  todos,
  markdown,
  modelLabel,
  status,
  error,
  superseded,
  sessionId,
}: Props) {
  const t = useTranslation()
  const [viewOpen, setViewOpen] = useState(false)
  const [confirmClose, setConfirmClose] = useState(false)
  const [pathCopied, setPathCopied] = useState(false)
  const [mode, setMode] = useState<'preview' | 'edit'>('preview')
  const [draft, setDraft] = useState('')
  const [baseline, setBaseline] = useState('')
  const [docReady, setDocReady] = useState(false)
  const [saving, setSaving] = useState(false)
  const requestModeSwitch = useChatStore((s) => s.requestModeSwitch)
  const applyPlanCardDocument = useChatStore((s) => s.applyPlanCardDocument)
  const resumePlanExecution = useChatStore((s) => s.resumePlanExecution)
  const filesRoot = useWorkspaceFilesStore((s) => s.root)
  const sessionWorkDir = useSessionStore((s) => {
    if (!sessionId) return null
    const entry = s.sessions.find((item) => item.id === sessionId)
    return entry?.workDir?.trim() || null
  })
  const addToast = useUIStore((s) => s.addToast)
  const cardRef = useRef<HTMLDivElement>(null)
  const markdownRef = useRef(markdown)
  markdownRef.current = markdown
  const fileTarget = useMemo(
    () => resolveWorkspaceFile([sessionWorkDir, filesRoot], planPath),
    [sessionWorkDir, filesRoot, planPath],
  )

  const execState = useChatStore((s) => {
    const session = sessionId ? s.sessions[sessionId] : undefined
    if (!session) return 'idle' as PlanExecutionState
    return selectPlanCardExecutionStateCached(
      session.messages,
      messageId,
      session.chatState ?? 'idle',
    )
  })

  const completed = status === 'completed'
  const failed = status === 'failed'
  const visibleTodos = todos.slice(0, 3)
  const moreCount = Math.max(0, todos.length - visibleTodos.length)
  const showMoreLabel = moreCount > 0 ? t('plan.todosShowMore', { count: moreCount }) : ''
  const canEdit =
    completed &&
    !failed &&
    !superseded &&
    Boolean(fileTarget) &&
    docReady &&
    execState !== 'executing' &&
    execState !== 'pending_switch'
  const dirty = draft !== baseline

  useEffect(() => {
    if (!viewOpen) {
      setDocReady(false)
      setMode('preview')
      setSaving(false)
      setConfirmClose(false)
      return
    }
    let cancelled = false
    const fallback = markdownRef.current ?? ''
    setMode('preview')
    setDraft(fallback)
    setBaseline(fallback)
    setDocReady(false)
    const target = fileTarget
    void (async () => {
      if (!target) {
        if (!cancelled) setDocReady(true)
        return
      }
      try {
        const res = await workspaceFilesApi.readFile({
          root: target.root,
          path: target.relPath,
        })
        if (cancelled) return
        if (!res.isBinary && res.encoding === 'utf8') {
          setDraft(res.content)
          setBaseline(res.content)
          if (sessionId && res.content !== fallback) {
            applyPlanCardDocument(sessionId, messageId, res.content)
          }
        }
      } catch {
        if (!cancelled) {
          addToast({ type: 'error', message: t('plan.reloadFailed') })
        }
      } finally {
        if (!cancelled) setDocReady(true)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [viewOpen, fileTarget, sessionId, messageId, applyPlanCardDocument, addToast, t])

  useEffect(() => {
    if (!canEdit && mode === 'edit') setMode('preview')
  }, [canEdit, mode])

  function handleBuild() {
    if (!completed || !sessionId || !planPath || superseded) return
    requestModeSwitch(sessionId, planPath)
  }

  const closeView = useCallback(() => {
    setConfirmClose(false)
    setViewOpen(false)
  }, [])

  const handleCloseView = useCallback(() => {
    if (saving) return
    if (confirmClose) return
    if (dirty && canEdit) {
      setConfirmClose(true)
      return
    }
    closeView()
  }, [saving, confirmClose, dirty, canEdit, closeView])

  const handleSave = useCallback(async (): Promise<boolean> => {
    if (!canEdit || !fileTarget || saving || !dirty) return false
    setSaving(true)
    try {
      await workspaceFilesApi.writeFile({
        root: fileTarget.root,
        path: fileTarget.relPath,
        content: draft,
      })
      if (sessionId) {
        applyPlanCardDocument(sessionId, messageId, draft)
      }
      setBaseline(draft)
      addToast({ type: 'success', message: t('plan.saved') })
      return true
    } catch {
      addToast({ type: 'error', message: t('plan.saveFailed') })
      return false
    } finally {
      setSaving(false)
    }
  }, [
    canEdit,
    fileTarget,
    saving,
    dirty,
    draft,
    sessionId,
    messageId,
    applyPlanCardDocument,
    addToast,
    t,
  ])

  useEffect(() => {
    if (!viewOpen) return
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== 's') return
      e.preventDefault()
      void handleSave()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [viewOpen, handleSave])

  async function handleCopyPath() {
    if (!planPath) return
    try {
      if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
        await navigator.clipboard.writeText(planPath)
      } else {
        const ta = document.createElement('textarea')
        ta.value = planPath
        ta.style.position = 'fixed'
        ta.style.opacity = '0'
        document.body.appendChild(ta)
        ta.select()
        document.execCommand('copy')
        document.body.removeChild(ta)
      }
      setPathCopied(true)
      setTimeout(() => setPathCopied(false), 1600)
    } catch {
      setPathCopied(false)
    }
  }

  const containerEmphasis = completed
    ? 'border-[var(--color-plan-accent)]/55 ring-1 ring-[var(--color-plan-accent)]/25 shadow-[0_2px_18px_-8px_var(--color-plan-accent)]'
    : 'border-[var(--color-outline-variant)]/40'

  return (
    <div
      ref={cardRef}
      className={`mb-3 ${superseded ? 'opacity-60 saturate-50' : ''}`}
    >
      <div
        className={`rounded-[var(--radius-lg)] border ${containerEmphasis} bg-[var(--color-surface-container-lowest)] overflow-hidden transition-all`}
      >
        <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)]">
          <span className="material-symbols-outlined text-[14px] text-[var(--color-plan-accent)]">
            description
          </span>
          <span
            className="font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)] truncate"
            title={planPath || fileName}
          >
            {fileName}
          </span>
          {planPath && (
            <button
              type="button"
              onClick={() => void handleCopyPath()}
              title={pathCopied ? t('plan.copyPathDone') : t('plan.copyPath', { path: planPath })}
              aria-label={t('plan.copyPath', { path: planPath })}
              className="inline-flex items-center justify-center rounded-[var(--radius-sm)] px-1 py-0.5 text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-container)] hover:text-[var(--color-text-primary)] transition-colors"
            >
              <span className="material-symbols-outlined text-[14px]">
                {pathCopied ? 'check' : 'content_copy'}
              </span>
            </button>
          )}
          <span className="ml-auto flex items-center gap-1.5">
            {failed ? (
              <span
                className="text-[11px] text-[var(--color-error)] flex items-center gap-1"
                title={error || t('plan.failedHint')}
              >
                <span className="material-symbols-outlined text-[12px]">error</span>
                {t('plan.failed')}
              </span>
            ) : !completed ? (
              <span className="text-[11px] text-[var(--color-text-tertiary)] flex items-center gap-1">
                <span className="material-symbols-outlined text-[12px] animate-spin">progress_activity</span>
                {t('plan.writingPlan')}
              </span>
            ) : null}
          </span>
        </div>

        <div className="px-3 py-2.5">
          <div className="text-[14px] font-bold text-[var(--color-text-primary)] leading-tight">
            {title || t('plan.untitledPlan')}
          </div>
          {overview && (
            <div className="mt-1 text-[12px] text-[var(--color-text-secondary)] leading-relaxed line-clamp-3">
              {overview}
            </div>
          )}

          {completed && todos.length > 0 && (
            <div className="mt-2.5 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] px-2 py-1.5">
              <div className="text-[11px] font-medium text-[var(--color-text-tertiary)] mb-1">
                {t('plan.todosCount', { count: todos.length })}
              </div>
              <ul className="space-y-1">
                {visibleTodos.map((todo) => (
                  <li key={todo.id} className="flex items-start gap-1.5 text-[12px]">
                    <span className={`shrink-0 mt-[3px] inline-block h-3 w-3 rounded-full border ${todoStatusClass(todo.status)}`} />
                    <span className="leading-snug text-[var(--color-text-primary)]">
                      {todo.content}
                    </span>
                  </li>
                ))}
                {moreCount > 0 && (
                  <li className="pl-[18px] text-[11px] text-[var(--color-text-tertiary)]">
                    {showMoreLabel}
                  </li>
                )}
              </ul>
            </div>
          )}
        </div>

        <div className="flex items-center justify-between gap-2 border-t border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)] px-3 py-1.5">
          <button
            type="button"
            onClick={() => setViewOpen(true)}
            disabled={!markdown && !planPath}
            className="flex items-center gap-1 text-[11px] font-medium text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)] disabled:opacity-40 transition-colors"
          >
            <span className="material-symbols-outlined text-[12px]">description</span>
            {t('plan.viewPlan')}
          </button>
          <div className="flex items-center gap-2">
            {modelLabel && (
              <span className="text-[11px] text-[var(--color-text-tertiary)] truncate max-w-[120px]">
                {modelLabel}
              </span>
            )}
            {execState === 'executing' ? (
              <span
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-surface-container-low)] text-[var(--color-text-tertiary)] cursor-not-allowed select-none"
                aria-label={t('plan.executing')}
              >
                <span className="material-symbols-outlined text-[14px] animate-spin">
                  progress_activity
                </span>
                {t('plan.executing')}
              </span>
            ) : execState === 'incomplete_run' ? (
              <button
                type="button"
                onClick={() => {
                  if (!sessionId || !planPath) return
                  resumePlanExecution(sessionId, planPath)
                }}
                disabled={!sessionId || !planPath || superseded}
                title={t('plan.resumeTitle')}
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-plan-accent)] text-[var(--color-on-plan-accent-container)] hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed transition-all"
              >
                <span className="material-symbols-outlined text-[14px]">
                  play_arrow
                </span>
                {t('plan.resume')}
              </button>
            ) : execState === 'completed_run' ? (
              <span
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-success)]/15 text-[var(--color-success)] cursor-default select-none"
                aria-label={t('plan.completed')}
              >
                <span className="material-symbols-outlined text-[14px]">
                  check_circle
                </span>
                {t('plan.completed')}
              </span>
            ) : failed ? (
              <span
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-error)]/15 text-[var(--color-error)] cursor-default select-none"
                title={error || t('plan.failedHint')}
                aria-label={t('plan.failed')}
              >
                <span className="material-symbols-outlined text-[14px]">error</span>
                {t('plan.failed')}
              </span>
            ) : (
              <button
                type="button"
                onClick={handleBuild}
                disabled={
                  !completed || !planPath || superseded || execState === 'pending_switch'
                }
                className="flex items-center gap-1 rounded-[var(--radius-md)] px-3 py-1 text-[11px] font-semibold bg-[var(--color-plan-accent)] text-[var(--color-on-plan-accent-container)] hover:brightness-110 disabled:opacity-40 disabled:cursor-not-allowed transition-all"
              >
                {t('plan.build')}
                <span className="text-[10px] px-1 py-0.5 rounded bg-[var(--color-plan-accent-hover)]/20">
                  {t('plan.buildShortcut')}
                </span>
              </button>
            )}
          </div>
        </div>
      </div>

      <Modal
        open={viewOpen}
        onClose={handleCloseView}
        title={fileName}
        width={760}
        bodyClassName={mode === 'edit' && canEdit ? 'overflow-hidden' : undefined}
        footer={
          <div className="flex w-full items-center justify-between gap-2">
            <div
              className="flex items-center rounded-xl border border-[var(--color-outline-variant)]/40 p-0.5"
              role="tablist"
              aria-label={t('plan.viewPlan')}
            >
              <button
                type="button"
                role="tab"
                aria-selected={mode === 'preview'}
                onClick={() => setMode('preview')}
                className={`rounded-[10px] px-2.5 py-1 text-[11px] font-medium transition-colors ${
                  mode === 'preview'
                    ? 'bg-[var(--color-surface-container)] text-[var(--color-text-primary)]'
                    : 'text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]'
                }`}
              >
                {t('plan.preview')}
              </button>
              <button
                type="button"
                role="tab"
                aria-selected={mode === 'edit'}
                disabled={!canEdit}
                onClick={() => setMode('edit')}
                className={`rounded-[10px] px-2.5 py-1 text-[11px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${
                  mode === 'edit'
                    ? 'bg-[var(--color-surface-container)] text-[var(--color-text-primary)]'
                    : 'text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]'
                }`}
              >
                {t('plan.edit')}
              </button>
            </div>
            <div className="flex items-center gap-2">
              {canEdit && (
                <Button
                  size="sm"
                  variant="primary"
                  onClick={() => void handleSave()}
                  disabled={!dirty || saving}
                  loading={saving}
                >
                  {saving ? t('plan.saving') : t('plan.save')}
                </Button>
              )}
              <Button size="sm" variant="secondary" onClick={handleCloseView} disabled={saving}>
                {t('common.close')}
              </Button>
            </div>
          </div>
        }
      >
        <div className={mode === 'edit' && canEdit ? 'min-h-0' : 'max-h-[70vh] min-h-0 px-1'}>
          {mode === 'edit' && canEdit ? (
            <textarea
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              spellCheck={false}
              autoFocus
              className="h-[58vh] min-h-[50vh] w-full resize-none overflow-auto rounded-xl border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container-lowest)] px-3 py-2.5 font-mono text-xs leading-5 text-[var(--color-text-primary)] focus:border-[var(--color-plan-accent)] focus:outline-none"
            />
          ) : draft ? (
            <div className="markdown-prose prose prose-sm max-h-[70vh] max-w-none overflow-y-auto">
              <MarkdownRenderer content={draft} />
            </div>
          ) : (
            <div className="break-all py-2 text-[12px] text-[var(--color-text-tertiary)]">
              {planPath}
            </div>
          )}
        </div>
      </Modal>

      <UnsavedChangesDialog
        open={confirmClose}
        title={t('editor.unsavedClose.title')}
        body={t('editor.unsavedClose.body', { name: fileName })}
        saveLabel={t('editor.unsavedClose.save')}
        discardLabel={t('editor.unsavedClose.discard')}
        cancelLabel={t('editor.unsavedClose.cancel')}
        busy={saving}
        onSave={async () => {
          const ok = await handleSave()
          if (ok) closeView()
        }}
        onDiscard={() => {
          setDraft(baseline)
          closeView()
        }}
        onCancel={() => setConfirmClose(false)}
      />
    </div>
  )
}

function todoStatusClass(status: Todo['status']): string {
  switch (status) {
    case 'completed':
      return 'border-[var(--color-success)] bg-[var(--color-success)]'
    case 'in_progress':
      return 'border-[var(--color-plan-accent)] bg-[var(--color-plan-accent)]/30'
    case 'cancelled':
      return 'border-[var(--color-text-tertiary)] bg-[var(--color-text-tertiary)]/30'
    case 'pending':
    default:
      return 'border-[var(--color-outline-variant)] bg-transparent'
  }
}
