import { useMemo, useState } from 'react'
import type { ToolViewProps } from './ToolViewProps'
import { CodeViewer } from '../CodeViewer'
import { MarkdownRenderer } from '../../markdown/MarkdownRenderer'
import { extractTextContent, truncate } from '../../../utils/toolFormatters'
import { useChatStore } from '../../../stores/chatStore'
import { useTabStore } from '../../../stores/tabStore'
import { useTranslation } from '../../../i18n'
import type { AgentTimeline, AgentTimelineEntry } from '../../../types/chat'
import { getCategoryIcon, getToolCategory } from '../../../utils/toolCategory'

function readString(input: unknown, keys: string[]): string {
  if (!input || typeof input !== 'object') return ''
  const obj = input as Record<string, unknown>
  for (const k of keys) {
    const v = obj[k]
    if (typeof v === 'string' && v.trim()) return v
  }
  return ''
}

function useTimelineBucket(toolUseId: string) {
  const activeTabId = useTabStore((s) => s.activeTabId)
  return useChatStore((s) => {
    const session = activeTabId ? s.sessions[activeTabId] : undefined
    if (!session) return null
    return session.subagentTimelines[toolUseId] ?? null
  })
}

function useChatState(): { isIdle: boolean; stop: () => void } {
  const activeTabId = useTabStore((s) => s.activeTabId)
  const chatState = useChatStore((s) =>
    activeTabId ? s.sessions[activeTabId]?.chatState ?? 'idle' : 'idle',
  )
  const stopGeneration = useChatStore((s) => s.stopGeneration)
  return {
    isIdle: chatState === 'idle',
    stop: () => {
      if (activeTabId) stopGeneration(activeTabId)
    },
  }
}

type AgentStats = {
  total: number
  completed: number
  errored: number
  running: number
}

function summarizeBucket(agents: Record<string, AgentTimeline>): AgentStats {
  let completed = 0
  let errored = 0
  let running = 0
  for (const tl of Object.values(agents)) {
    if (tl.status === 'completed') completed += 1
    else if (tl.status === 'error') errored += 1
    else running += 1
  }
  return {
    total: Object.keys(agents).length,
    completed,
    errored,
    running,
  }
}

function statusDotColor(stats: AgentStats, parentHasResult: boolean): string {
  if (stats.errored > 0) return 'bg-[var(--color-error)]'
  if (stats.running > 0 && !parentHasResult)
    return 'bg-[var(--color-warning)] animate-pulse'
  if (stats.total === 0 && !parentHasResult)
    return 'bg-[var(--color-outline)]'
  return 'bg-[var(--color-success)]'
}

export function TaskHeader({ toolName, toolUseId, input, result }: ToolViewProps) {
  const bucket = useTimelineBucket(toolUseId)
  const agents = bucket?.agents ?? {}
  const stats = summarizeBucket(agents)
  const prompt = readString(input, [
    'prompt',
    'instructions',
    'message',
    'description',
    'task',
    'title',
    'name',
  ])
  const parentHasResult = Boolean(result)
  const colour = statusDotColor(stats, parentHasResult)

  return (
    <span className="min-w-0 flex-1 flex items-center gap-2 truncate text-[12px] text-[var(--color-text-secondary)]">
      <span
        className={`shrink-0 size-2 rounded-full ${colour}`}
        aria-hidden
      />
      <span
        className="min-w-0 truncate font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]"
        title={prompt || toolName}
      >
        {truncate(prompt || toolName, 80)}
      </span>
      {stats.total > 0 && (
        <span className="shrink-0 rounded-full bg-[var(--color-surface-container-high)] px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-text-secondary)]">
          {stats.completed}/{stats.total}
        </span>
      )}
      {stats.errored > 0 && (
        <span className="shrink-0 rounded-full bg-[var(--color-error)]/15 px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] text-[var(--color-error)]">
          {stats.errored} fail
        </span>
      )}
    </span>
  )
}

export function TaskDetail({ toolName, toolUseId, input, result }: ToolViewProps) {
  const bucket = useTimelineBucket(toolUseId)
  const { isIdle, stop } = useChatState()
  const t = useTranslation()
  const agents = useMemo(() => {
    const list = bucket ? Object.values(bucket.agents) : []
    return list.sort((a, b) => a.startedAt - b.startedAt)
  }, [bucket])
  const prompt = readString(input, ['prompt', 'instructions', 'message', 'description'])
  const inputJson = JSON.stringify(input ?? null, null, 2)
  const finalText = result ? extractTextContent(result.content) : ''

  const hasAgents = agents.length > 0

  return (
    <div className="space-y-2">
      {prompt ? (
        <div className="rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2 text-[12px] leading-snug text-[var(--color-text-secondary)] whitespace-pre-wrap">
          {prompt}
        </div>
      ) : (
        <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]">
          <CodeViewer code={inputJson} language="json" maxLines={8} />
        </div>
      )}

      {hasAgents && (
        <div className="space-y-1.5">
          {agents.map((tl, idx) => (
            <AgentPanel
              key={tl.agentId}
              agent={tl}
              defaultOpen={idx === 0 && tl.status === 'running'}
            />
          ))}
        </div>
      )}

      {!hasAgents && !finalText && !result && (
        <div className="rounded-md border border-dashed border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] px-3 py-3 text-center font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
          {t('task.subagent.waiting')}
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
            {t('task.subagent.finalOutput')}
          </div>
          <MarkdownRenderer content={finalText} />
        </div>
      )}

      {!isIdle && (
        <div className="flex justify-end">
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation()
              stop()
            }}
            className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-2 py-1 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            <span className="material-symbols-outlined text-[14px]">stop_circle</span>
            {t('task.subagent.stop')}
          </button>
        </div>
      )}

      {}
      <span className="sr-only">{toolName}</span>
    </div>
  )
}

function AgentPanel({
  agent,
  defaultOpen,
}: {
  agent: AgentTimeline
  defaultOpen: boolean
}) {
  const t = useTranslation()
  const [open, setOpen] = useState(defaultOpen)
  const statusChip = agentStatusChip(agent.status, t)
  const durationLabel = formatDurationMs(agent.updatedAt - agent.startedAt)

  return (
    <div className="overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)]">
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation()
          setOpen((v) => !v)
        }}
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left hover:bg-[var(--color-surface-hover)]/50"
        aria-expanded={open}
      >
        <span
          className={`shrink-0 size-2 rounded-full ${statusChip.dot}`}
          aria-hidden
        />
        <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
          {truncate(agent.agentId, 28)}
        </span>
        {agent.taskId && (
          <span className="shrink-0 font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]">
            {truncate(agent.taskId, 20)}
          </span>
        )}
        <span className={`shrink-0 rounded-full px-1.5 py-0.5 font-[var(--font-mono)] text-[10px] ${statusChip.badge}`}>
          {statusChip.label}
        </span>
        <span className="ml-auto shrink-0 font-[var(--font-mono)] text-[10px] text-[var(--color-text-tertiary)]">
          {durationLabel}
        </span>
        <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
          {open ? 'expand_less' : 'expand_more'}
        </span>
      </button>
      {open && (
        <div className="border-t border-[var(--color-border)]/60 px-3 py-2 space-y-1.5">
          {agent.entries.length === 0 ? (
            <div className="font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
              {t('task.subagent.empty')}
            </div>
          ) : (
            <ul className="space-y-1">
              {agent.entries.map((entry, idx) => (
                <li key={idx}>
                  <TimelineRow entry={entry} />
                </li>
              ))}
            </ul>
          )}
          {agent.finalOutput && (
            <div className="mt-2 rounded-md border border-[var(--color-border)]/60 bg-[var(--color-surface-container-low)] px-2 py-1.5">
              <div className="mb-0.5 text-[10px] uppercase tracking-[0.18em] text-[var(--color-outline)]">
                {t('task.subagent.agentOutput')}
              </div>
              <div className="whitespace-pre-wrap break-words font-[var(--font-mono)] text-[11px] leading-[1.45] text-[var(--color-text-secondary)]">
                {agent.finalOutput}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function TimelineRow({ entry }: { entry: AgentTimelineEntry }) {
  if (entry.kind === 'thinking') {
    return (
      <div className="flex items-start gap-2 rounded-md bg-[var(--color-surface-container-low)]/60 px-2 py-1">
        <span className="material-symbols-outlined mt-0.5 shrink-0 text-[12px] text-[var(--color-text-tertiary)]">
          psychology
        </span>
        <div className="min-w-0 flex-1 whitespace-pre-wrap break-words font-[var(--font-mono)] text-[11px] italic leading-[1.45] text-[var(--color-text-tertiary)]">
          {entry.text}
        </div>
      </div>
    )
  }
  if (entry.kind === 'tool_call') {
    const category = getToolCategory(entry.name)
    const icon = getCategoryIcon(category)
    return (
      <div className="flex items-center gap-2 rounded-md border border-[var(--color-border)]/40 bg-[var(--color-surface)] px-2 py-1">
        <span className="material-symbols-outlined shrink-0 text-[12px] text-[var(--color-outline)]">
          {icon}
        </span>
        <span className="shrink-0 font-[var(--font-mono)] text-[11px] font-semibold text-[var(--color-text-secondary)]">
          {entry.name || 'tool'}
        </span>
        {entry.summary && (
          <span
            className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
            title={entry.summary}
          >
            {entry.summary}
          </span>
        )}
      </div>
    )
  }
  if (entry.kind === 'tool_result') {
    return (
      <div
        className={`flex items-center gap-2 rounded-md border px-2 py-1 ${
          entry.isError
            ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/30'
            : 'border-[var(--color-border)]/40 bg-[var(--color-surface-container-low)]'
        }`}
      >
        <span
          className={`material-symbols-outlined shrink-0 text-[12px] ${
            entry.isError ? 'text-[var(--color-error)]' : 'text-[var(--color-success)]'
          }`}
        >
          {entry.isError ? 'error' : 'check_circle'}
        </span>
        {entry.name && (
          <span className="shrink-0 font-[var(--font-mono)] text-[11px] text-[var(--color-text-secondary)]">
            {entry.name}
          </span>
        )}
        {entry.preview && (
          <span
            className="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
            title={entry.preview}
          >
            {truncate(entry.preview, 160)}
          </span>
        )}
      </div>
    )
  }
  if (entry.kind === 'status') {
    return (
      <div className="flex items-center gap-2 px-2 py-0.5 font-[var(--font-mono)] text-[11px] italic text-[var(--color-text-tertiary)]">
        <span className="material-symbols-outlined shrink-0 text-[12px]">
          bolt
        </span>
        {entry.text}
      </div>
    )
  }
  return (
    <div className="whitespace-pre-wrap break-words rounded-md bg-[var(--color-surface-container-low)]/40 px-2 py-1 font-[var(--font-mono)] text-[11px] leading-[1.5] text-[var(--color-text-secondary)]">
      {entry.text}
    </div>
  )
}

type Chip = { label: string; badge: string; dot: string }

function agentStatusChip(
  status: AgentTimeline['status'],
  t: ReturnType<typeof useTranslation>,
): Chip {
  if (status === 'completed') {
    return {
      label: t('task.subagent.completed'),
      badge: 'bg-[var(--color-success)]/15 text-[var(--color-success)]',
      dot: 'bg-[var(--color-success)]',
    }
  }
  if (status === 'error') {
    return {
      label: t('task.subagent.failed'),
      badge: 'bg-[var(--color-error)]/15 text-[var(--color-error)]',
      dot: 'bg-[var(--color-error)]',
    }
  }
  return {
    label: t('task.subagent.running'),
    badge: 'bg-[var(--color-warning)]/15 text-[var(--color-warning)]',
    dot: 'bg-[var(--color-warning)] animate-pulse',
  }
}

function formatDurationMs(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return ''
  if (ms < 1000) return `${ms}ms`
  const seconds = ms / 1000
  if (seconds < 60) return `${seconds.toFixed(1)}s`
  const minutes = Math.floor(seconds / 60)
  const s = Math.round(seconds - minutes * 60)
  return `${minutes}m${s.toString().padStart(2, '0')}s`
}
