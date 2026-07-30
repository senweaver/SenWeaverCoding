// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useChatStore } from '../../stores/chatStore'
import { useSessionStore } from '../../stores/sessionStore'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import { useTranslation } from '../../i18n'
import { Modal } from '../shared/Modal'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import { revealInExplorer } from '../../lib/revealInExplorer'
import { useUIStore } from '../../stores/uiStore'
import {
  extractMermaidBlocks,
  renderCuratorDiagrams,
  regenerateCuratorDocxWithDiagrams,
} from '../../lib/mermaidToImage'
import {
  selectCuratorCardExecutionState,
  type CuratorExecutionState,
} from '../../utils/activeCuratorSelector'

function cleanWinPath(path: string): string {
  if (!path) return path
  if (path.startsWith('\\\\?\\UNC\\')) return '\\\\' + path.slice('\\\\?\\UNC\\'.length)
  if (path.startsWith('\\\\?\\')) return path.slice('\\\\?\\'.length)
  return path
}

function absPathToRel(workDir: string, absPath: string): string | null {
  const root = cleanWinPath(workDir).replace(/\\/g, '/').replace(/\/+$/, '')
  const file = cleanWinPath(absPath).replace(/\\/g, '/')
  if (!root || !file) return null
  const rootLower = root.toLowerCase()
  const fileLower = file.toLowerCase()
  if (fileLower === rootLower) return ''
  const prefix = `${rootLower}/`
  if (!fileLower.startsWith(prefix)) return null
  return file.slice(root.length + 1)
}

function sameWorkspaceRoot(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false
  const na = cleanWinPath(a).replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
  const nb = cleanWinPath(b).replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase()
  return na === nb
}

const docxDiagramsProcessed = new Set<string>()

type Props = {
  messageId: string
  slug: string
  template: string
  finalMdPath: string
  implBlueprintPath: string
  docxPath?: string
  title: string
  body: string
  sessionId?: string | null
  status?: 'writing' | 'completed' | 'failed'
  error?: string
}

export function CuratorCard({
  messageId,
  slug,
  template,
  finalMdPath,
  implBlueprintPath,
  docxPath,
  title,
  body,
  sessionId,
  status = 'completed',
  error,
}: Props) {
  const t = useTranslation()
  const [viewOpen, setViewOpen] = useState(false)
  const [pathCopied, setPathCopied] = useState(false)
  const requestModeSwitch = useChatStore((s) => s.requestCuratorModeSwitch)
  const resumeCuratorExecution = useChatStore((s) => s.resumeCuratorExecution)
  const continueCuratorWriting = useChatStore((s) => s.continueCuratorWriting)

  const curatorInputs = useChatStore(
    useShallow((s) => {
      const session = sessionId ? s.sessions[sessionId] : undefined
      return {
        messages: session?.messages,
        chatState: session?.chatState,
      }
    }),
  )
  const workDir = useSessionStore((s) => {
    if (!sessionId) return null
    const entry = s.sessions.find((item) => item.id === sessionId)
    return entry?.workDir?.trim() || null
  })
  const execState = useMemo<CuratorExecutionState>(() => {
    if (!sessionId || !curatorInputs.messages) return 'idle'
    return selectCuratorCardExecutionState(
      curatorInputs.messages,
      messageId,
      curatorInputs.chatState ?? 'idle',
    )
  }, [sessionId, curatorInputs.messages, curatorInputs.chatState, messageId])

  const sessionIsLive = (curatorInputs.chatState ?? 'idle') !== 'idle'
  const interruptedWhileWriting = status === 'writing' && !sessionIsLive
  const isWriting = status === 'writing' && sessionIsLive
  const isFailed = status === 'failed' || interruptedWhileWriting
  const resolvedError =
    error ||
    (interruptedWhileWriting
      ? t('curator.interrupted') ||
        'The turn ended before the document was finalized. Click "Continue writing".'
      : undefined)
  const isExecuting = execState === 'executing'
  const isPendingSwitch = execState === 'pending_switch'
  const isIncomplete = execState === 'incomplete_run'
  const isBuilt = execState === 'completed_run'
  const buildDisabled =
    !sessionId || isWriting || isFailed || isExecuting || isBuilt || isPendingSwitch || isIncomplete

  function handleBuild() {
    if (buildDisabled) return
    requestModeSwitch(sessionId!, implBlueprintPath || finalMdPath, {
      slug,
      template,
      finalMdPath,
    })
  }

  function handleResume() {
    if (!sessionId) return
    resumeCuratorExecution(sessionId, implBlueprintPath || finalMdPath)
  }

  function handleContinueWriting() {
    if (!sessionId) return
    continueCuratorWriting(sessionId)
  }

  async function handleRevealDocx() {
    const target = docxDisplay
    if (!target) return
    try {
      await revealInExplorer(target)
    } catch (err) {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('files.preview.revealFailed', {
          message: err instanceof Error ? err.message : String(err),
        }),
        duration: 5000,
      })
    }
  }

  async function handleCopyPath() {
    const target = cleanWinPath(finalMdPath || implBlueprintPath)
    if (!target) return
    try {
      if (navigator.clipboard && typeof navigator.clipboard.writeText === 'function') {
        await navigator.clipboard.writeText(target)
      } else {
        const ta = document.createElement('textarea')
        ta.value = target
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

  async function handleOpenFinalMd() {
    const abs = cleanWinPath(finalMdPath)
    if (!abs) {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('files.outsideWorkspace'),
        duration: 5000,
      })
      return
    }
    if (!workDir) {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('files.workspaceMissing'),
        duration: 5000,
      })
      return
    }
    const relPath = absPathToRel(workDir, abs)
    if (relPath === null || relPath === '') {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('files.outsideWorkspace'),
        duration: 5000,
      })
      return
    }
    try {
      const files = useWorkspaceFilesStore.getState()
      if (!sameWorkspaceRoot(files.root, workDir)) {
        files.setRoot(workDir)
      }
      useUIStore.getState().setRightSidebarOpen(true)
      await files.selectFile(relPath)
      const after = useWorkspaceFilesStore.getState()
      const buf = after.files[`${after.root ?? ''}::${relPath}`]
      if (buf?.error) {
        useUIStore.getState().addToast({
          type: 'error',
          message: t('files.errorLoading', { message: buf.error }),
          duration: 5000,
        })
      }
    } catch (err) {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('files.errorLoading', {
          message: err instanceof Error ? err.message : String(err),
        }),
        duration: 5000,
      })
    }
  }

  const subtitle = `${template} · ${slug}`
  const finalMdDisplay = cleanWinPath(finalMdPath)
  const implBlueprintDisplay = cleanWinPath(implBlueprintPath)
  const docxDisplay = docxPath ? cleanWinPath(docxPath) : undefined
  const headerName =
    (finalMdDisplay || implBlueprintDisplay).split(/[\\/]/).slice(-1)[0] || 'final.md'
  const bodyTrimmed = body.trim()

  const [diagramState, setDiagramState] = useState<'idle' | 'rendering' | 'done'>('idle')
  const diagramRanRef = useRef(false)

  useEffect(() => {
    if (!isTauriRuntime()) return
    if (isWriting) return
    if (!docxPath || !finalMdPath) return
    if (diagramRanRef.current) return
    const processKey = `${messageId}::${docxPath}`
    if (docxDiagramsProcessed.has(processKey)) return
    if (extractMermaidBlocks(body).length === 0) return

    diagramRanRef.current = true
    docxDiagramsProcessed.add(processKey)
    let cancelled = false
    setDiagramState('rendering')
    void (async () => {
      try {
        const diagrams = await renderCuratorDiagrams(body)
        if (cancelled) return
        if (diagrams.length > 0) {
          await regenerateCuratorDocxWithDiagrams({
            finalMdPath: cleanWinPath(finalMdPath),
            template,
            diagrams,
          })
        }
      } finally {
        if (!cancelled) setDiagramState('done')
      }
    })()
    return () => {
      cancelled = true
    }
  }, [messageId, docxPath, finalMdPath, body, template, isWriting])

  return (
    <div className="mb-3">
      <div
        className="rounded-[var(--radius-lg)] border border-[var(--color-curator-accent)]/55 ring-1 ring-[var(--color-curator-accent)]/25 shadow-[0_2px_18px_-8px_var(--color-curator-accent)] bg-[var(--color-surface-container-lowest)] overflow-hidden transition-all"
      >
        <div className="flex items-center gap-2 px-3 py-1.5 border-b border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)]">
          <span className="material-symbols-outlined text-[14px] text-[var(--color-curator-accent)]">
            auto_stories
          </span>
          <button
            type="button"
            onClick={() => void handleOpenFinalMd()}
            disabled={!finalMdDisplay}
            className="min-w-0 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)] truncate hover:text-[var(--color-curator-accent)] hover:underline cursor-pointer bg-transparent border-0 p-0 text-left disabled:cursor-default disabled:hover:no-underline disabled:hover:text-[var(--color-text-secondary)]"
            title={finalMdDisplay || implBlueprintDisplay}
          >
            {headerName}
          </button>
          <span className="text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded-full bg-[var(--color-curator-accent-container)] text-[var(--color-on-curator-accent-container)]">
            curator
          </span>
          {isWriting && (
            <span className="ml-auto inline-flex items-center gap-1 text-[10px] text-[var(--color-curator-accent)]">
              <span className="material-symbols-outlined text-[12px] animate-spin">progress_activity</span>
              {t('curator.statusWriting') || 'Writing…'}
            </span>
          )}
          {isFailed && (
            <span className="ml-auto inline-flex items-center gap-1 text-[10px] text-[var(--color-error)]">
              <span className="material-symbols-outlined text-[12px]">error</span>
              {t('curator.statusFailed') || 'Not finalized'}
            </span>
          )}
        </div>
        <div className="px-3 py-3">
          <div className="text-sm font-semibold text-[var(--color-text-primary)] truncate" title={title}>
            {title}
          </div>
          <div className="text-[11px] text-[var(--color-text-tertiary)] mt-0.5">{subtitle}</div>
          {bodyTrimmed ? (
            <div className="mt-2 max-h-[18rem] overflow-y-auto rounded-md border border-[var(--color-outline-variant)]/25 bg-[var(--color-surface)]/40 px-2.5 py-2 text-[12px] leading-relaxed text-[var(--color-text-secondary)]">
              <MarkdownRenderer content={body} />
            </div>
          ) : null}
          {isFailed && (
            <div className="mt-2 rounded-md border border-[var(--color-error)]/40 bg-[var(--color-error)]/10 px-2 py-1.5">
              <div className="text-[11px] font-medium text-[var(--color-error)]">
                {t('curator.failedHint') || 'The deliverable was not finalized. Tell the assistant to continue or fix the issue below.'}
              </div>
              {resolvedError && (
                <div className="mt-1 max-h-[120px] overflow-auto whitespace-pre-wrap break-words text-[10px] leading-relaxed text-[var(--color-text-secondary)]">
                  {resolvedError}
                </div>
              )}
            </div>
          )}
        </div>
        <div className="flex items-center justify-end gap-1.5 px-3 py-2 border-t border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)]">
          <button
            onClick={() => setViewOpen(true)}
            className="text-[11px] px-2 py-1 rounded-md text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            {t('curator.viewDocument')}
          </button>
          <button
            onClick={handleCopyPath}
            className="text-[11px] px-2 py-1 rounded-md text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            {pathCopied ? t('plan.copyPathDone') : t('curator.copyPath')}
          </button>
          {docxDisplay && (
            <button
              onClick={() => void handleRevealDocx()}
              disabled={diagramState === 'rendering'}
              title={diagramState === 'rendering' ? t('curator.renderingDiagrams') || docxDisplay : docxDisplay}
              className="flex items-center gap-1 text-[11px] px-2 py-1 rounded-md text-[var(--color-curator-accent)] hover:bg-[var(--color-surface-hover)] disabled:opacity-60 disabled:cursor-not-allowed"
            >
              <span
                className={`material-symbols-outlined text-[14px]${diagramState === 'rendering' ? ' animate-spin' : ''}`}
              >
                {diagramState === 'rendering' ? 'progress_activity' : 'folder_open'}
              </span>
              {t('curator.revealDocx')}
            </button>
          )}
          {isIncomplete ? (
            <button
              onClick={handleResume}
              disabled={!sessionId}
              className="flex items-center gap-1 text-[11px] font-semibold px-2.5 py-1 rounded-md bg-[var(--color-curator-accent)] text-white hover:bg-[var(--color-curator-accent-hover)] disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined text-[14px]">play_arrow</span>
              {t('plan.resume')}
            </button>
          ) : isExecuting ? (
            <span
              className="flex items-center gap-1 rounded-md px-2.5 py-1 text-[11px] font-semibold bg-[var(--color-surface-container-low)] text-[var(--color-text-tertiary)] cursor-not-allowed select-none"
              aria-label={t('plan.executing') || 'Executing'}
            >
              <span className="material-symbols-outlined text-[14px] animate-spin">
                progress_activity
              </span>
              {t('plan.executing') || 'Executing'}
            </span>
          ) : isBuilt ? (
            <span
              className="flex items-center gap-1 rounded-md px-2.5 py-1 text-[11px] font-semibold bg-[var(--color-success)]/15 text-[var(--color-success)] cursor-default select-none"
              aria-label={t('plan.completed') || 'Completed'}
            >
              <span className="material-symbols-outlined text-[14px]">
                check_circle
              </span>
              {t('plan.completed') || 'Completed'}
            </span>
          ) : isFailed ? (
            <button
              onClick={handleContinueWriting}
              disabled={!sessionId || sessionIsLive}
              className="flex items-center gap-1 text-[11px] font-semibold px-2.5 py-1 rounded-md bg-[var(--color-curator-accent)] text-white hover:bg-[var(--color-curator-accent-hover)] disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined text-[14px]">edit_note</span>
              {t('curator.continueWriting') || 'Continue writing'}
            </button>
          ) : (
            <button
              onClick={handleBuild}
              disabled={buildDisabled}
              className="text-[11px] font-semibold px-2.5 py-1 rounded-md bg-[var(--color-curator-accent)] text-white hover:bg-[var(--color-curator-accent-hover)] disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {t('curator.actions.buildSwitch') || 'Build → Agent'}
            </button>
          )}
        </div>
      </div>
      {viewOpen && (
        <Modal open={viewOpen} onClose={() => setViewOpen(false)} title={title} width={920}>
          <div className="max-h-[70vh] overflow-auto p-1">
            <MarkdownRenderer content={body} />
          </div>
        </Modal>
      )}
    </div>
  )
}
