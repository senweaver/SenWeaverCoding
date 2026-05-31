// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { ToolCard } from './tools/ToolCard'
import { ThinkingBlock } from './ThinkingBlock'
import { useTranslation } from '../../i18n'
import type { TranslationKey } from '../../i18n'
import type { UIMessage } from '../../types/chat'
import { getToolCategory } from '../../utils/toolCategory'

type ToolUse = Extract<UIMessage, { type: 'tool_use' }>
type ToolResult = Extract<UIMessage, { type: 'tool_result' }>
type Thinking = Extract<UIMessage, { type: 'thinking' }>

export type ExploredSummary = {
  reads: number
  lists: number
  searches: number
  recalls: number
  thinkingCount: number
}

type Props = {
  items: UIMessage[]
  resultMap: Map<string, ToolResult>
  summary: ExploredSummary

  isStreaming?: boolean

  activeThinkingId?: string | null
}

const COUNTERS: Array<{
  key: keyof ExploredSummary
  one: TranslationKey
  many: TranslationKey
}> = [
  { key: 'reads', one: 'explored.readOne', many: 'explored.readMany' },
  { key: 'lists', one: 'explored.listedOne', many: 'explored.listedMany' },
  { key: 'searches', one: 'explored.searchedOne', many: 'explored.searchedMany' },
  { key: 'recalls', one: 'explored.recalledOne', many: 'explored.recalledMany' },
  { key: 'thinkingCount', one: 'explored.thoughtOne', many: 'explored.thoughtMany' },
]

export function buildExploredSummary(items: UIMessage[]): ExploredSummary {
  let reads = 0
  let lists = 0
  let searches = 0
  let recalls = 0
  let thinkingCount = 0
  for (const item of items) {
    if (item.type === 'thinking') {
      thinkingCount++
      continue
    }
    if (item.type !== 'tool_use') continue
    const cat = getToolCategory(item.toolName)
    if (cat === 'read') reads++
    else if (cat === 'list') lists++
    else if (cat === 'search') searches++
    else if (cat === 'memory_recall') recalls++
  }
  return { reads, lists, searches, recalls, thinkingCount }
}

export function ExploredCard({
  items,
  resultMap,
  summary,
  isStreaming = false,
  activeThinkingId,
}: Props) {
  const t = useTranslation()
  const [expanded, setExpanded] = useState(false)

  useEffect(() => {
    if (isStreaming) setExpanded(true)
  }, [isStreaming])

  const summaryParts = COUNTERS.flatMap(({ key, one, many }) => {
    const n = summary[key]
    if (n <= 0) return []
    return [n === 1 ? t(one) : t(many, { count: n })]
  })
  const summaryText =
    summaryParts.length > 0 ? summaryParts.join(t('explored.join')) : t('explored.empty')

  return (
    <div className="mb-2">
      <button
        type="button"
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
        className="flex w-full items-center gap-2 rounded-lg border border-[var(--color-border)]/40 bg-[var(--color-surface-container-low)] px-3 py-1.5 text-left transition-colors hover:bg-[var(--color-surface-container-high)]"
      >
        <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
          travel_explore
        </span>
        <span className="shrink-0 text-[11px] font-semibold text-[var(--color-text-secondary)]">
          {isStreaming ? t('explored.exploring') : t('explored.prefix')}
        </span>
        <span className="min-w-0 flex-1 truncate text-[12px] text-[var(--color-text-secondary)]">
          {summaryText}
        </span>
        {isStreaming && (
          <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--color-brand)] animate-pulse-dot" />
        )}
        <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
          {expanded ? 'expand_less' : 'expand_more'}
        </span>
      </button>

      {expanded && (
        <div className="mt-1.5 ml-2 space-y-1.5 border-l border-[var(--color-border)]/40 pl-3">
          {items.map((item) => {
            if (item.type === 'thinking') {
              const thinking = item as Thinking
              return (
                <ThinkingBlock
                  key={thinking.id}
                  content={thinking.content}
                  isActive={thinking.id === activeThinkingId}
                  startedAt={thinking.startedAt}
                  completedAt={thinking.completedAt}
                  compact
                />
              )
            }
            if (item.type === 'tool_use') {
              const tu = item as ToolUse
              const r = resultMap.get(tu.toolUseId)
              return (
                <ToolCard
                  key={tu.id}
                  toolName={tu.toolName}
                  toolUseId={tu.toolUseId}
                  input={tu.input}
                  result={r ? { content: r.content, isError: r.isError } : null}
                  isStreaming={!r && isStreaming}
                  compact
                />
              )
            }
            return null
          })}
        </div>
      )}
    </div>
  )
}
