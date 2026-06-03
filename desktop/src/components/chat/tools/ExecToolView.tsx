// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import type { ToolViewProps } from './ToolViewProps'
import { TerminalChrome } from '../TerminalChrome'
import { CodeViewer } from '../CodeViewer'
import { extractCommand, extractTextContent, firstWord, truncate } from '../../../utils/toolFormatters'
import { useTerminalPanelStore } from '../../../stores/terminalPanelStore'
import { useTabStore } from '../../../stores/tabStore'
import { useChatStore } from '../../../stores/chatStore'
import { useTranslation } from '../../../i18n'

const EXEC_MAX_LINES = 32

function formatElapsed(ms: number): string {
  const totalSec = Math.max(0, Math.floor(ms / 1000))
  if (totalSec < 60) return `${totalSec}s`
  const m = Math.floor(totalSec / 60)
  const s = totalSec % 60
  return `${m}m${s.toString().padStart(2, '0')}s`
}

function RunningTimer({ start }: { start?: number }) {
  const [now, setNow] = useState(() => Date.now())
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), 1000)
    return () => clearInterval(id)
  }, [])
  const base = typeof start === 'number' && start > 0 ? start : now
  return (
    <span className="shrink-0 font-[var(--font-mono)] text-[10px] tabular-nums text-[var(--color-text-tertiary)]">
      {formatElapsed(now - base)}
    </span>
  )
}

export function ExecHeader({
  input,
  isStreaming,
  toolTimestamp,
  parentSessionId,
  toolUseId,
}: ToolViewProps) {
  const t = useTranslation()
  const cancelTool = useChatStore((s) => s.cancelTool)
  const command = extractCommand(input)
  const stopLabel = (t('execTool.stop' as never) as string) || 'Stop this command'

  const stopControl =
    isStreaming && parentSessionId ? (
      <span
        role="button"
        tabIndex={0}
        title={stopLabel}
        aria-label={stopLabel}
        onClick={(e) => {
          e.stopPropagation()
          e.preventDefault()
          cancelTool(parentSessionId, toolUseId)
        }}
        onKeyDown={(e) => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.stopPropagation()
            e.preventDefault()
            cancelTool(parentSessionId, toolUseId)
          }
        }}
        className="shrink-0 inline-flex h-[18px] w-[18px] items-center justify-center rounded-full border border-[var(--color-error)]/40 text-[var(--color-error)] transition-colors hover:bg-[var(--color-error)]/12"
      >
        <span className="material-symbols-outlined text-[12px]">stop</span>
      </span>
    ) : null

  if (!command) {
    return (
      <span className="flex min-w-0 flex-1 items-center gap-2 text-[11px] text-[var(--color-text-tertiary)]">
        <span className="min-w-0 flex-1 truncate">(no command)</span>
        {isStreaming && <RunningTimer start={toolTimestamp} />}
        {stopControl}
      </span>
    )
  }
  const head = firstWord(command)
  return (
    <span
      className="flex min-w-0 flex-1 items-baseline gap-2 text-[12px] text-[var(--color-text-secondary)]"
      title={command}
    >
      {head && (
        <span className="shrink-0 font-[var(--font-mono)] text-[12px] text-[var(--color-text-primary)]">
          {head}
        </span>
      )}
      <span className="min-w-0 flex-1 truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]">
        {truncate(command.replace(/\s+/g, ' '), 120)}
      </span>
      {isStreaming && <RunningTimer start={toolTimestamp} />}
      {stopControl}
    </span>
  )
}

export function ExecDetail({ input, result, parentSessionId }: ToolViewProps) {
  const command = extractCommand(input)
  const text = result ? extractTextContent(result.content) : ''
  const t = useTranslation()
  const setOpen = useTerminalPanelStore((s) => s.setOpen)
  const ensureAgentMirrorTab = useTerminalPanelStore((s) => s.ensureAgentMirrorTab)
  const syncAgentMirrorForChatSession = useTerminalPanelStore(
    (s) => s.syncAgentMirrorForChatSession,
  )
  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessionForMirror = parentSessionId || activeTabId || null

  const openInTerminalPanel = () => {
    setOpen(true)
    ensureAgentMirrorTab(sessionForMirror)
    syncAgentMirrorForChatSession(sessionForMirror)
  }

  const fullLogLabel =
    (t('execTool.openFullLog' as never) as string) || 'Open in terminal panel'

  return (
    <div className="space-y-2">
      {command && (
        <TerminalChrome title={firstWord(command) || 'shell'}>
          <div className="px-3 py-2 font-[var(--font-mono)] text-[11px] leading-[1.45] text-[var(--color-terminal-fg)] whitespace-pre-wrap break-words">
            <span className="text-[var(--color-terminal-accent)]">$</span> {command}
          </div>
        </TerminalChrome>
      )}
      {text && (
        <div
          className={`overflow-hidden rounded-md border ${
            result?.isError
              ? 'border-[var(--color-error)]/30 bg-[var(--color-error-container)]/40'
              : 'border-[var(--color-border)] bg-[var(--color-surface)]'
          }`}
        >
          <CodeViewer code={text} language="plaintext" maxLines={EXEC_MAX_LINES} />
        </div>
      )}
      {sessionForMirror && (
        <button
          type="button"
          onClick={openInTerminalPanel}
          className="inline-flex items-center gap-1.5 rounded-md border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] px-2 py-1 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]"
          title={fullLogLabel}
        >
          <span className="material-symbols-outlined text-[14px]">terminal</span>
          <span>{fullLogLabel}</span>
        </button>
      )}
    </div>
  )
}
