import { useMemo, useState } from 'react'
import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { MarkdownRenderer } from '../../markdown/MarkdownRenderer'
import { extractTextContent, truncate } from '../../../utils/toolFormatters'
import { useTabStore } from '../../../stores/tabStore'
import { useWorkersStore } from '../../../stores/workersStore'
import { useTranslation } from '../../../i18n'
import type { WorkerSnapshot } from '../../../types/chat'

function statusDotColor(workers: WorkerSnapshot[], parentHasResult: boolean): string {
  if (workers.length === 0)
    return parentHasResult ? 'bg-[var(--color-success)]' : 'bg-[var(--color-outline)]'
  const anyFailed = workers.some((w) => w.status === 'failed')
  if (anyFailed) return 'bg-[var(--color-error)]'
  const anyRunning = workers.some(
    (w) => w.status === 'running' || w.status === 'pending',
  )
  if (anyRunning && !parentHasResult) return 'bg-[var(--color-warning)] animate-pulse'
  return 'bg-[var(--color-success)]'
}

function statusBadge(status: WorkerSnapshot['status']): {
  label: string
  className: string
  dot: string
} {
  switch (status) {
    case 'completed':
      return {
        label: 'completed',
        className: 'bg-[var(--color-success)]/15 text-[var(--color-success)]',
        dot: 'bg-[var(--color-success)]',
      }
    case 'failed':
      return {
        label: 'failed',
        className: 'bg-[var(--color-error)]/15 text-[var(--color-error)]',
        dot: 'bg-[var(--color-error)]',
      }
    case 'stopped':
      return {
        label: 'stopped',
        className:
          'bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)]',
        dot: 'bg-[var(--color-text-tertiary)]',
      }
    default:
      return {
        label: status,
        className: 'bg-[var(--color-warning)]/15 text-[var(--color-warning)]',
        dot: 'bg-[var(--color-warning)] animate-pulse',
      }
  }
}

function useWorkersForSpawnCard(
  toolUseId: string,
  parentSessionId: string | null | undefined,
  toolTimestamp?: number,
): WorkerSnapshot[] {
  const resolveForSpawnCard = useWorkersStore((s) => s.resolveForSpawnCard)
  const workersById = useWorkersStore((s) => s.workersById)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessionId = parentSessionId ?? activeTabId
  return useMemo(() => {
    if (!sessionId) return []
    return resolveForSpawnCard(toolUseId, sessionId, toolTimestamp)
  }, [resolveForSpawnCard, workersById, toolUseId, sessionId, toolTimestamp])
}

export function SpawnWorkersHeader({
  toolName,
  toolUseId,
  result,
  parentSessionId,
  toolTimestamp,
}: ToolViewProps) {
  const workers = useWorkersForSpawnCard(toolUseId, parentSessionId, toolTimestamp)
  const parentHasResult = Boolean(result)
  const color = statusDotColor(workers, parentHasResult)
  const completed = workers.filter((w) => w.status === 'completed').length
  const failed = workers.filter((w) => w.status === 'failed').length
  const running = workers.filter(
    (w) => w.status === 'running' || w.status === 'pending',
  )
  const latestProgress = running
    .map((w) => {
      const parts = [w.lastAction, w.lastDetail].filter(Boolean)
      return parts.length ? `${w.title}: ${parts.join(' · ')}` : null
    })
    .filter(Boolean)
    .slice(-2)

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 truncate text-[12px] text-[var(--color-text-secondary)]">
      <span className={`shrink-0 size-2 rounded-full ${color}`} aria-hidden />
      <span
        className="min-w-0 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]"
        title={toolName}
      >
        {workers.length > 0
          ? `Spawn ${workers.length} worker${workers.length === 1 ? '' : 's'}`
          : 'Spawn workers'}
      </span>
      {workers.length > 0 && (
        <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-secondary)]">
          {completed}/{workers.length}
        </span>
      )}
      {failed > 0 && (
        <span className="shrink-0 rounded-full bg-[var(--color-error)]/15 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-error)]">
          {failed} fail
        </span>
      )}
      {latestProgress.length > 0 && (
        <span
          className="hidden md:inline min-w-0 truncate font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]"
          title={latestProgress.join(' | ')}
        >
          {latestProgress.join(' · ')}
        </span>
      )}
    </span>
  )
}

export function SpawnWorkersDetail({
  toolName,
  toolUseId,
  input,
  result,
  parentSessionId,
  toolTimestamp,
}: ToolViewProps) {
  const t = useTranslation()
  const workers = useWorkersForSpawnCard(toolUseId, parentSessionId, toolTimestamp)
  const openWorkerTab = useTabStore((s) => s.openWorkerTab)
  const stopWorker = useWorkersStore((s) => s.stopWorker)
  const [stoppingId, setStoppingId] = useState<string | null>(null)
  const [stoppingAll, setStoppingAll] = useState(false)
  const inputJson = useMemo(
    () => JSON.stringify(input ?? null, null, 2),
    [input],
  )
  const finalText = result ? extractTextContent(result.content) : ''
  const hasWorkers = workers.length > 0
  const hasRunning = workers.some(
    (w) => w.status === 'running' || w.status === 'pending',
  )

  const stopAll = async () => {
    if (stoppingAll) return
    setStoppingAll(true)
    try {
      for (const w of workers) {
        if (w.status === 'running' || w.status === 'pending') {
          await stopWorker(w.workerId)
        }
      }
    } finally {
      setStoppingAll(false)
    }
  }

  return (
    <div className="space-y-2">
      {!hasWorkers && (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
          <CodeViewer code={inputJson} language="json" maxLines={8} />
        </div>
      )}

      {hasWorkers && (
        <div className="space-y-1.5">
          {hasRunning && (
            <div className="flex items-center justify-end">
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  void stopAll()
                }}
                disabled={stoppingAll}
                className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-2 py-1 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
              >
                <span className="material-symbols-outlined text-[14px]">stop_circle</span>
                {t('chat.workers.stopAll') || 'Stop all'}
              </button>
            </div>
          )}
          {workers.map((w) => {
            const chip = statusBadge(w.status)
            const isStoppingThis = stoppingId === w.workerId
            const isTerminal =
              w.status === 'completed' ||
              w.status === 'failed' ||
              w.status === 'stopped'
            return (
              <div
                key={w.workerId}
                className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)]"
              >
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation()
                    openWorkerTab(w.workerId, w.title || w.workerId)
                  }}
                  className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--color-surface-hover)]/50"
                >
                  <span className={`shrink-0 size-2 rounded-full ${chip.dot}`} aria-hidden />
                  <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
                    smart_toy
                  </span>
                  <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)] truncate">
                    {truncate(w.title || w.workerId, 36)}
                  </span>
                  {w.model && (
                    <span className="shrink-0 font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)] truncate">
                      {w.model}
                    </span>
                  )}
                  <span
                    className={`shrink-0 rounded-full px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] ${chip.className}`}
                  >
                    {chip.label}
                  </span>
                  {(w.lastAction || w.lastDetail) && (
                    <span
                      className="ml-auto min-w-0 truncate font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]"
                      title={`${w.lastAction ?? ''} ${w.lastDetail ?? ''}`.trim()}
                    >
                      {w.lastAction ? `${w.lastAction} · ` : ''}
                      {w.lastDetail ?? ''}
                    </span>
                  )}
                  {!isTerminal && (
                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation()
                        if (isStoppingThis) return
                        setStoppingId(w.workerId)
                        void stopWorker(w.workerId).finally(() => {
                          setStoppingId(null)
                        })
                      }}
                      disabled={isStoppingThis}
                      className="shrink-0 inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
                      title={t('chat.workers.stopWorker') || 'Stop'}
                    >
                      <span className="material-symbols-outlined text-[12px]">stop</span>
                    </button>
                  )}
                  <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
                    open_in_new
                  </span>
                </button>
              </div>
            )
          })}
        </div>
      )}

      {finalText && (
        <div
          className={`rounded-md border px-3 py-2 ${
            result?.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface-container-low)]'
          }`}
        >
          <div className="mb-1 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
            {t('task.subagent.finalOutput') || 'Final output'}
          </div>
          <MarkdownRenderer content={finalText} />
        </div>
      )}

      <span className="sr-only">{toolName}</span>
    </div>
  )
}
