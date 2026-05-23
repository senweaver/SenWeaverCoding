import { useState } from 'react'
import { useChatStore } from '../../stores/chatStore'
import { useTranslation } from '../../i18n'
import { Modal } from '../shared/Modal'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import {
  selectCuratorCardExecutionState,
  type CuratorExecutionState,
} from '../../utils/activeCuratorSelector'

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

  const execState = useChatStore((s): CuratorExecutionState => {
    if (!sessionId) return 'idle'
    const session = s.sessions[sessionId]
    if (!session) return 'idle'
    return selectCuratorCardExecutionState(session.messages, messageId, session.chatState)
  })

  const isWriting = status === 'writing'
  const isExecuting = execState === 'executing'
  const isPendingSwitch = execState === 'pending_switch'
  const isBuilt = execState === 'completed_run'
  const buildDisabled = !sessionId || isWriting || isExecuting || isBuilt || isPendingSwitch

  function handleBuild() {
    if (buildDisabled) return
    requestModeSwitch(sessionId!, implBlueprintPath || finalMdPath, {
      slug,
      template,
      finalMdPath,
    })
  }

  async function handleCopyPath() {
    const target = finalMdPath || implBlueprintPath
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
            title={finalMdPath || implBlueprintPath}
          >
            {(finalMdPath || implBlueprintPath).split(/[\\/]/).slice(-1)[0] || 'final.md'}
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
            <div className="truncate" title={finalMdPath}>
              <span className="opacity-70">final.md:</span>{' '}
              <code className="text-[10px]">{finalMdPath}</code>
            </div>
            <div className="truncate" title={implBlueprintPath}>
              <span className="opacity-70">impl_blueprint.md:</span>{' '}
              <code className="text-[10px]">{implBlueprintPath}</code>
            </div>
            {docxPath && (
              <div className="truncate" title={docxPath}>
                <span className="opacity-70">final.docx:</span>{' '}
                <code className="text-[10px]">{docxPath}</code>
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
          {isExecuting ? (
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
