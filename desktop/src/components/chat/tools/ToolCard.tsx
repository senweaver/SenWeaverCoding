// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState, type ComponentType, type ReactNode } from 'react'
import { useTranslation, type TranslationKey } from '../../../i18n'
import {
  getCategoryIcon,
  getToolCategory,
  type ToolCategory,
} from '../../../utils/toolCategory'
import { Popover } from '../../shared/Popover'
import { useSettingsStore } from '../../../stores/settingsStore'
import type { ToolViewProps } from './ToolViewProps'
import { ReadHeader, ReadDetail } from './ReadToolView'
import { SearchHeader, SearchDetail, getSearchHoverContent } from './SearchToolView'
import { ListHeader, ListDetail } from './ListToolView'
import { EditHeader, EditDetail } from './EditToolView'
import { ExecHeader, ExecDetail } from './ExecToolView'
import { WebHeader, WebDetail } from './WebToolView'
import { WebSearchHeader, WebSearchDetail } from './WebSearchToolView'
import { isWebSearchTool } from '../../../utils/toolFormatters'
import { MemoryRecallHeader, MemoryRecallDetail } from './MemoryRecallView'
import { PlanHeader, PlanDetail } from './PlanToolView'
import { TaskHeader, TaskDetail } from './TaskToolView'
import { SpawnWorkersHeader, SpawnWorkersDetail } from './SpawnWorkersToolView'
import { TodoListHeader, TodoListDetail } from './TodoListToolView'
import { GenericHeader, GenericDetail } from './GenericToolView'
import { GitHeader, GitDetail } from './GitToolView'
import { DiagnosticsHeader, DiagnosticsDetail } from './DiagnosticsToolView'
import { CodeIntelHeader, CodeIntelDetail } from './CodeIntelToolView'
import { SessionsHeader, SessionsDetail } from './SessionsToolView'
import { CommunicationHeader, CommunicationDetail } from './CommunicationToolView'
import { FlowHeader, FlowDetail } from './FlowToolView'
import { ModelHeader, ModelDetail } from './ModelToolView'
import { McpHeader, McpDetail } from './McpToolView'
import { MemoryOtherHeader, MemoryOtherDetail } from './MemoryOtherToolView'
import { IntegrationHeader, IntegrationDetail } from './IntegrationToolView'
import { OpsHeader, OpsDetail } from './OpsToolView'
import { HardwareHeader, HardwareDetail } from './HardwareToolView'
import { DocumentHeader, DocumentDetail } from './DocumentToolView'

type Renderer = {
  Header: ComponentType<ToolViewProps>
  Detail: ComponentType<ToolViewProps>

  alwaysExpandable?: boolean

  getHoverContent?: (props: ToolViewProps) => ReactNode | null
}

const RENDERERS: Record<ToolCategory, Renderer> = {
  read: { Header: ReadHeader, Detail: ReadDetail, alwaysExpandable: true },
  list: { Header: ListHeader, Detail: ListDetail },
  search: {
    Header: SearchHeader,
    Detail: SearchDetail,
    getHoverContent: getSearchHoverContent,
  },
  web: {
    Header: (props) =>
      isWebSearchTool(props.toolName) ? (
        <WebSearchHeader {...props} />
      ) : (
        <WebHeader {...props} />
      ),
    Detail: (props) =>
      isWebSearchTool(props.toolName) ? (
        <WebSearchDetail {...props} />
      ) : (
        <WebDetail {...props} />
      ),
    alwaysExpandable: true,
  },
  edit: { Header: EditHeader, Detail: EditDetail, alwaysExpandable: true },
  exec: { Header: ExecHeader, Detail: ExecDetail, alwaysExpandable: true },
  memory_recall: { Header: MemoryRecallHeader, Detail: MemoryRecallDetail },
  memory_other: { Header: MemoryOtherHeader, Detail: MemoryOtherDetail },
  plan: { Header: PlanHeader, Detail: PlanDetail, alwaysExpandable: true },
  tasks: { Header: TodoListHeader, Detail: TodoListDetail, alwaysExpandable: true },
  task: { Header: TaskHeader, Detail: TaskDetail, alwaysExpandable: true },
  mcp: { Header: McpHeader, Detail: McpDetail },
  git: { Header: GitHeader, Detail: GitDetail, alwaysExpandable: true },
  diagnostics: { Header: DiagnosticsHeader, Detail: DiagnosticsDetail },
  sessions: { Header: SessionsHeader, Detail: SessionsDetail },
  comm: { Header: CommunicationHeader, Detail: CommunicationDetail, alwaysExpandable: true },
  flow: { Header: FlowHeader, Detail: FlowDetail },
  code_intel: { Header: CodeIntelHeader, Detail: CodeIntelDetail },
  model: { Header: ModelHeader, Detail: ModelDetail, alwaysExpandable: true },
  integration: { Header: IntegrationHeader, Detail: IntegrationDetail, alwaysExpandable: true },
  ops: { Header: OpsHeader, Detail: OpsDetail, alwaysExpandable: true },
  hardware: { Header: HardwareHeader, Detail: HardwareDetail, alwaysExpandable: true },
  other: { Header: GenericHeader, Detail: GenericDetail },
}

const VERB_KEYS: Record<ToolCategory, TranslationKey> = {
  read: 'tool.verb.read',
  list: 'tool.verb.listed',
  search: 'tool.verb.searched',
  web: 'tool.verb.fetched',
  edit: 'tool.verb.edited',
  exec: 'tool.verb.ran',
  memory_recall: 'tool.verb.recalled',
  memory_other: 'tool.verb.memory',
  plan: 'tool.verb.planned',
  tasks: 'tool.verb.tasks',
  task: 'tool.verb.delegated',
  mcp: 'tool.verb.mcp',
  git: 'tool.verb.git',
  diagnostics: 'tool.verb.diagnosed',
  sessions: 'tool.verb.sessionsed',
  comm: 'tool.verb.communicated',
  flow: 'tool.verb.flowed',
  code_intel: 'tool.verb.codeIntel',
  model: 'tool.verb.modelConfig',
  integration: 'tool.verb.integration',
  ops: 'tool.verb.ops',
  hardware: 'tool.verb.hardware',
  other: 'tool.verb.tool',
}

type Props = ToolViewProps & {
  defaultExpanded?: boolean
}

export function ToolCard({
  toolName,
  toolUseId,
  input,
  result,
  isStreaming = false,
  compact = false,
  defaultExpanded,
  parentSessionId,
  toolTimestamp,
  childCalls,
  childResults,
}: Props) {
  const t = useTranslation()
  const category = getToolCategory(toolName)
  const isSpawnWorkers = toolName === 'spawn_workers'
  const isDocumentConvert = toolName === 'document_convert'
  const renderer: Renderer = isSpawnWorkers
    ? {
        Header: SpawnWorkersHeader,
        Detail: SpawnWorkersDetail,
        alwaysExpandable: true,
      }
    : isDocumentConvert
      ? {
          Header: DocumentHeader,
          Detail: DocumentDetail,
          alwaysExpandable: true,
        }
      : RENDERERS[category]
  const icon = isSpawnWorkers
    ? 'smart_toy'
    : isDocumentConvert
      ? 'swap_horiz'
      : getCategoryIcon(category)
  const verb = isSpawnWorkers
    ? t('chat.workers.spawnVerb') || 'spawned'
    : isDocumentConvert
      ? t('tool.verb.converted')
      : t(VERB_KEYS[category])
  const modeBadge = compact ? null : getModeBadge(useSettingsStore.getState().codingMode)
  const expandable =
    renderer.alwaysExpandable === true ||
    Boolean(result && hasMeaningfulOutput(result.content))
  const hideVerb = category === 'web'

  // Edit cards used to auto-expand, which synchronously mounts a heavy diff (word-level diff +
  // syntax highlight) the instant a tool completes — a major source of jank during active
  // editing. The collapsed header still shows the +/- line-count badge; users expand on click.
  const categoryDefaultExpanded = false
  const initialExpanded = defaultExpanded ?? categoryDefaultExpanded
  const [expanded, setExpanded] = useState<boolean>(initialExpanded)

  useEffect(() => {
    if (defaultExpanded === undefined) return
    setExpanded(defaultExpanded)
  }, [defaultExpanded])

  const suppressListCard =
    category === 'list' &&
    !(result?.isError) &&
    (result == null || !hasMeaningfulOutput(result.content))

  if (suppressListCard) {
    return null
  }

  const detailEnabled = expandable && expanded

  const childProps: ToolViewProps = {
    toolName,
    toolUseId,
    input,
    result: result ?? null,
    isStreaming,
    compact,
    parentSessionId,
    toolTimestamp,
    childCalls,
    childResults,
  }

  const containerClassName = compact
    ? 'mb-1 last:mb-0'
    : 'mb-2 overflow-hidden rounded-lg border border-[var(--color-border)]/50 bg-[var(--color-surface-container-lowest)]'

  const hoverContent = renderer.getHoverContent
    ? renderer.getHoverContent(childProps)
    : null

  const headerButton = (
    <button
      type="button"
      onClick={() => {
        if (expandable) setExpanded((v) => !v)
      }}
      aria-expanded={expanded}
      disabled={!expandable && !isStreaming}
      className={`flex w-full items-center gap-2 px-3 py-1.5 text-left transition-colors ${
        expandable || hoverContent ? 'hover:bg-[var(--color-surface-hover)]/50' : ''
      } ${compact ? 'rounded-md' : ''}`}
    >
      <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
        {icon}
      </span>
      {!hideVerb && (
        <span className="shrink-0 text-[11px] font-semibold text-[var(--color-text-secondary)]">
          {verb}
        </span>
      )}
      {modeBadge && (
        <span className={`shrink-0 rounded-full px-1.5 py-0.5 text-[9px] font-medium ${modeBadge.className}`}>
          {modeBadge.label}
        </span>
      )}
      <renderer.Header {...childProps} />
      {isStreaming && (
        <span className="shrink-0 text-[10px] text-[var(--color-text-tertiary)]">
          …
        </span>
      )}
      {result?.isError && !isStreaming && (
        <span
          className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-error)]"
          title={t('tool.error')}
        >
          error
        </span>
      )}
      {expandable && (
        <span className="material-symbols-outlined shrink-0 text-[14px] text-[var(--color-outline)]">
          {expanded ? 'expand_less' : 'expand_more'}
        </span>
      )}
    </button>
  )

  return (
    <div className={containerClassName}>
      {hoverContent ? (
        <Popover content={hoverContent} trigger="hover" minWidth={320} maxWidth={560}>
          {headerButton}
        </Popover>
      ) : (
        headerButton
      )}
      {detailEnabled && (
        <div
          className={
            compact
              ? 'mt-1.5 ml-6 rounded-md border border-[var(--color-border)]/40 bg-[var(--color-surface-container-lowest)] px-3 py-2'
              : 'border-t border-[var(--color-border)]/60 px-3 py-2.5'
          }
        >
          <renderer.Detail {...childProps} />
        </div>
      )}
    </div>
  )
}

function hasMeaningfulOutput(content: unknown): boolean {
  if (content == null) return false
  if (typeof content === 'string') return content.trim().length > 0
  if (Array.isArray(content)) return content.length > 0
  if (typeof content === 'object') return Object.keys(content as object).length > 0
  return false
}

type ModeBadge = { label: string; className: string } | null

function getModeBadge(mode: string): ModeBadge {
  switch (mode) {
    case 'agent':
    case 'harness':
      return {
        label: 'auto',
        className: 'bg-[var(--color-success)]/12 text-[var(--color-success)]',
      }
    case 'plan':
    case 'ask':
      return {
        label: 'read-only',
        className: 'bg-[var(--color-secondary)]/12 text-[var(--color-secondary)]',
      }
    case 'pair':
      return {
        label: 'checkpoint',
        className: 'bg-[var(--color-warning)]/12 text-[var(--color-warning)]',
      }
    default:
      return null
  }
}
