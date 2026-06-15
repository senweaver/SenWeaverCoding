// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useChatStore } from '../../stores/chatStore'
import { useTranslation } from '../../i18n'
import { Modal } from '../shared/Modal'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { isTauriRuntime } from '../../lib/desktopRuntime'
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

const docxDiagramsProcessed = new Set<string>()

async function openLocalPath(path: string): Promise<void> {
  const target = cleanWinPath(path)
  if (!target) return
  if (!isTauriRuntime()) {
    window.open(target, '_blank')
    return
  }
  try {
    const mod = (await import(/* @vite-ignore */ '@tauri-apps/plugin-shell')) as {
      open: (target: string) => Promise<void>
    }
    await mod.open(target)
  } catch (err) {
    console.warn('[CuratorCard] open docx failed', err)
  }
}

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
  status?: 'writing' | 'completed'
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
}: Props) {
  const t = useTranslation()
  const [viewOpen, setViewOpen] = useState(false)
  const [pathCopied, setPathCopied] = useState(false)
  const requestModeSwitch = useChatStore((s) => s.requestCuratorModeSwitch)
  const resumeCuratorExecution = useChatStore((s) => s.resumeCuratorExecution)

  const curatorInputs = useChatStore(
    useShallow((s) => {
      const session = sessionId ? s.sessions[sessionId] : undefined
      return {
        messages: session?.messages,
        chatState: session?.chatState,
      }
    }),
  )
  const execState = useMemo<CuratorExecutionState>(() => {
    if (!sessionId || !curatorInputs.messages) return 'idle'
    return selectCuratorCardExecutionState(
      curatorInputs.messages,
      messageId,
      curatorInputs.chatState ?? 'idle',
    )
  }, [sessionId, curatorInputs.messages, curatorInputs.chatState, messageId])

  const isWriting = status === 'writing'
  const isExecuting = execState === 'executing'
  const isPendingSwitch = execState === 'pending_switch'
  const isIncomplete = execState === 'incomplete_run'
  const isBuilt = execState === 'completed_run'
  const buildDisabled =
    !sessionId || isWriting || isExecuting || isBuilt || isPendingSwitch || isIncomplete

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

  const subtitle = `${template} · ${slug}`
  const finalMdDisplay = cleanWinPath(finalMdPath)
  const implBlueprintDisplay = cleanWinPath(implBlueprintPath)
  const docxDisplay = docxPath ? cleanWinPath(docxPath) : undefined

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
          <span
            className="font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)] truncate"
            title={finalMdDisplay || implBlueprintDisplay}
          >
            {(finalMdDisplay || implBlueprintDisplay).split(/[\\/]/).slice(-1)[0] || 'final.md'}
          </span>
          <span className="text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded-full bg-[var(--color-curator-accent-container)] text-[var(--color-on-curator-accent-container)]">
            curator
          </span>
          {isWriting && (
            <span className="ml-auto inline-flex items-center gap-1 text-[10px] text-[var(--color-curator-accent)]">
              <span className="material-symbols-outlined text-[12px] animate-spin">progress_activity</span>
              {t('curator.statusWriting') || 'Writing…'}
            </span>
          )}
        </div>
        <div className="px-3 py-3">
          <div className="text-sm font-semibold text-[var(--color-text-primary)] truncate" title={title}>
            {title}
          </div>
          <div className="text-[11px] text-[var(--color-text-tertiary)] mt-0.5">{subtitle}</div>
          <div className="mt-2 text-[11px] text-[var(--color-text-secondary)] space-y-0.5">
            <div className="truncate" title={finalMdDisplay}>
              <span className="opacity-70">final.md:</span>{' '}
              <code className="text-[10px]">{finalMdDisplay}</code>
            </div>
            <div className="truncate" title={implBlueprintDisplay}>
              <span className="opacity-70">impl_blueprint.md:</span>{' '}
              <code className="text-[10px]">{implBlueprintDisplay}</code>
            </div>
            {docxDisplay && (
              <div className="truncate" title={docxDisplay}>
                <span className="opacity-70">final.docx:</span>{' '}
                <code className="text-[10px]">{docxDisplay}</code>
              </div>
            )}
          </div>
        </div>
        <div className="flex items-center justify-end gap-1.5 px-3 py-2 border-t border-[var(--color-outline-variant)]/30 bg-[var(--color-surface-container-low)]">
          <button
            onClick={() => setViewOpen(true)}
            className="text-[11px] px-2 py-1 rounded-md text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            {t('plan.viewPlan')}
          </button>
          <button
            onClick={handleCopyPath}
            className="text-[11px] px-2 py-1 rounded-md text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            {pathCopied ? t('plan.copyPathDone') : t('curator.copyPath')}
          </button>
          {docxDisplay && (
            <button
              onClick={() => void openLocalPath(docxDisplay)}
              disabled={diagramState === 'rendering'}
              title={diagramState === 'rendering' ? t('curator.renderingDiagrams') || docxDisplay : docxDisplay}
              className="flex items-center gap-1 text-[11px] px-2 py-1 rounded-md text-[var(--color-curator-accent)] hover:bg-[var(--color-surface-hover)] disabled:opacity-60 disabled:cursor-not-allowed"
            >
              <span
                className={`material-symbols-outlined text-[14px]${diagramState === 'rendering' ? ' animate-spin' : ''}`}
              >
                {diagramState === 'rendering' ? 'progress_activity' : 'description'}
              </span>
              {t('curator.openDocx')}
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
