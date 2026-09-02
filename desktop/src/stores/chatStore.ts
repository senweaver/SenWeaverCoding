// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { wsManager } from '../api/websocket'
import { sessionsApi } from '../api/sessions'
import { useTeamStore } from './teamStore'
import { useSessionStore } from './sessionStore'
import {
  useWorkspaceQueueStore,
  workspaceKeyFor,
  tryDrainWorkspace,
  takeLastDrainedItem,
  requeueRejectedItem,
} from './workspaceQueueStore'
import { useSessionRunStateStore } from './sessionRunStateStore'
import { useCLITaskStore } from './cliTaskStore'
import { useSessionRuntimeStore } from './sessionRuntimeStore'
import { useSettingsStore } from './settingsStore'
import { useTabStore } from './tabStore'
import { useUsageStore } from './usageStore'
import { useWorkersStore } from './workersStore'
import { useBrowserPanelStore } from './browserPanelStore'
import { useDesignerCanvasStore } from './designerCanvasStore'
import { useLspStore } from './lspStore'
import { useDebugStore } from './debugStore'
import { useUIStore } from './uiStore'
import { t } from '../i18n'
import { isWindowBusy, onWindowIdle } from '../lib/windowBusy'
import { waitForScrollQuiet } from '../lib/scrollActivity'
import type { LspBroadcastEvent } from '../types/lsp'
import { randomSpinnerVerb } from '../config/spinnerVerbs'
import {
  hasUsableModelForSession,
  isNoModelConfiguredError,
} from '../utils/modelAvailability'
import { ensureSessionRuntimeSynced, queueSessionRuntimeSync } from '../utils/runtimeSync'
import { AGENT_LIFECYCLE_TYPES } from '../types/team'
import type { MessageEntry, PendingRewindSummary } from '../types/session'
import type { PermissionMode } from '../types/settings'
import type { CodingModeId } from '../types/codingMode'
import type { RuntimeSelection } from '../types/runtime'
import {
  parsePlanMarkdown,
  parseSavedPlanResult,
  parseExitPlanModeResult,
  planFileNameFromPath,
  type PlanTodo,
  type PlanTodoStatus,
} from '../utils/parsePlanMd'
import { parseCuratorEnvelope as parseCuratorEnvelopeForCard } from '../utils/parseCuratorMd'
import { isPlanModeAllowedTool } from '../utils/planModeTools'
import { rewritePlanMarkdownTodos } from '../utils/planMdMutate'
import type {
  AgentTaskNotification,
  AgentTimeline,
  AgentTimelineEntry,
  AttachmentRef,
  ChatState,
  DesignGenerationOptions,
  PendingEdit,
  SubagentTimelineBucket,
  UIAttachment,
  UIMessage,
  ServerMessage,
  TokenUsage,
} from '../types/chat'

type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'reconnecting'

export type PerSessionState = {
  messages: UIMessage[]
  chatState: ChatState
  connectionState: ConnectionState
  streamingText: string
  streamingToolArgs: { toolName: string; callIndex: number; argsSnapshot: string } | null
  activeToolUseId: string | null
  activeToolName: string | null
  activeThinkingId: string | null
  activeThinkingContent: string
  activeThinkingStartedAt: number | null
  activeThinkingLastChunkAt: number | null
  pendingPermission: {
    requestId: string
    toolName: string
    toolUseId?: string
    input: unknown
    description?: string
  } | null
  tokenUsage: TokenUsage

  cumulativeTokens: number

  cumulativeCostUsd: number
  elapsedSeconds: number
  planningPhaseStartedAt?: number | null
  statusVerb: string
  planningPhaseAction: string
  planningPhaseDetail: string
  slashCommands: Array<{ name: string; description: string }>
  agentTaskNotifications: Record<string, AgentTaskNotification>
  composerPrefill?: {
    text: string
    attachments?: UIAttachment[]
    nonce: number
  } | null

  composerDraft?: {
    text: string
    attachments: UIAttachment[]
    slashMenuOpen: boolean
  }

  pendingRewind?: PendingRewindSummary | null

  pendingSendAfterRewind?: {
    content: string
    attachments?: AttachmentRef[]
    options?: { displayContent?: string }
  } | null

  pendingEdits: PendingEdit[]

  keptEdits: PendingEdit[]

  subagentTimelines: Record<string, SubagentTimelineBucket>

  activeTaskToolUseId: string | null

  stopRequested: boolean

  debugPiiStats: DebugPiiStats

  pendingResourceWaits: PendingResourceWait[]

  providerRetry: ProviderRetryNotice | null

  historyLoaded?: boolean

  historyHasMore?: boolean

  historyFirstIndex?: number

  historyLoadingOlder?: boolean

  historyReloadNonce?: number
}

export type ProviderRetryNotice = {
  attempt: number
  maxAttempts: number
  waitMs: number
  waitDeadlineAt: number
  class: string
  provider: string
  model: string
  message: string
  receivedAt: number
}

export type ResourceWaitKind = 'file' | 'shell' | 'browser'

export interface PendingResourceWait {
  id: string
  kind: ResourceWaitKind
  target: string
  holderSessionId: string
  holderTitle: string
  startedAt: number
}

export type DebugPiiKind =
  | 'id_card'
  | 'phone'
  | 'email'
  | 'bank_card'
  | 'jwt'
  | 'api_key'
  | 'bearer'
  | 'auth_header'
  | 'url_password'
  | 'kv_secret'
  | 'private_key'
  | 'ipv4'
  | 'mac'
  | string

export interface DebugPiiStats {
  total: number
  counts: Record<string, number>
  lastEventAt: number | null
}

const DEFAULT_SESSION_STATE: PerSessionState = {
  messages: [],
  chatState: 'idle',
  connectionState: 'disconnected',
  streamingText: '',
  streamingToolArgs: null,
  activeToolUseId: null,
  activeToolName: null,
  activeThinkingId: null,
  activeThinkingContent: '',
  activeThinkingStartedAt: null,
  activeThinkingLastChunkAt: null,
  pendingPermission: null,
  tokenUsage: { input_tokens: 0, output_tokens: 0 },
  cumulativeTokens: 0,
  cumulativeCostUsd: 0,
  elapsedSeconds: 0,
  planningPhaseStartedAt: null,
  statusVerb: '',
  planningPhaseAction: '',
  planningPhaseDetail: '',
  slashCommands: [],
  agentTaskNotifications: {},
  composerPrefill: null,
  pendingRewind: null,
  pendingSendAfterRewind: null,
  pendingEdits: [],
  keptEdits: [],
  subagentTimelines: {},
  activeTaskToolUseId: null,
  stopRequested: false,
  debugPiiStats: { total: 0, counts: {}, lastEventAt: null },
  pendingResourceWaits: [],
  providerRetry: null,
}

function createDefaultSessionState(): PerSessionState {
  return {
    ...DEFAULT_SESSION_STATE,
    messages: [],
    tokenUsage: { input_tokens: 0, output_tokens: 0 },
    cumulativeTokens: 0,
    cumulativeCostUsd: 0,
    pendingRewind: null,
    pendingSendAfterRewind: null,
    pendingEdits: [],
    keptEdits: [],
    subagentTimelines: {},
    activeTaskToolUseId: null,
    pendingResourceWaits: [],
  }
}

function mergeKeptEdits(kept: PendingEdit[], incoming: PendingEdit[]): PendingEdit[] {
  if (incoming.length === 0) return kept
  const incomingPaths = new Set(incoming.map((e) => e.path))
  return [...kept.filter((e) => !incomingPaths.has(e.path)), ...incoming]
}

function mergePendingEdit(
  list: PendingEdit[],
  incoming: {
    path: string
    additions: number
    deletions: number
    editBatchId?: string | null
    timestamp: number
  },
): PendingEdit[] {
  const next = [...list]
  const idx = next.findIndex((e) => e.path === incoming.path)
  const batchId = incoming.editBatchId || ''
  const prev = idx >= 0 ? next[idx] : undefined
  if (prev) {
    const nextBatches = batchId && !prev.editBatchIds.includes(batchId)
      ? [...prev.editBatchIds, batchId]
      : prev.editBatchIds
    next[idx] = {
      path: prev.path,
      additions: prev.additions + incoming.additions,
      deletions: prev.deletions + incoming.deletions,
      editBatchIds: nextBatches,
      firstSeenAt: prev.firstSeenAt,
      lastSeenAt: incoming.timestamp,
    }
  } else {
    next.push({
      path: incoming.path,
      additions: incoming.additions,
      deletions: incoming.deletions,
      editBatchIds: batchId ? [batchId] : [],
      firstSeenAt: incoming.timestamp,
      lastSeenAt: incoming.timestamp,
    })
  }
  return next
}

type PendingSessionCodingMode = {
  sessionId: string
  mode: CodingModeId
  scope: 'session' | 'global'
  from?: string
}

type ChatStore = {
  sessions: Record<string, PerSessionState>
  sessionCodingMode: Record<string, CodingModeId>
  sessionAutoResolvedMode: Record<string, CodingModeId>
  pendingSessionCodingMode: PendingSessionCodingMode | null

  getSession: (sessionId: string) => PerSessionState
  connectToSession: (sessionId: string, options?: { force?: boolean }) => void
  connectToWorker: (workerId: string) => void
  disconnectSession: (sessionId: string) => void
  suspendSession: (sessionId: string) => void
  sendMessage: (
    sessionId: string,
    content: string,
    attachments?: AttachmentRef[],
    options?: {
      displayContent?: string
      __internalDrain?: boolean
      designGeneration?: DesignGenerationOptions
    },
  ) => void
  respondToPermission: (
    sessionId: string,
    requestId: string,
    allowed: boolean,
    options?: {
      rule?: string
      updatedInput?: Record<string, unknown>
    },
  ) => boolean
  setSessionRuntime: (sessionId: string, selection: RuntimeSelection, options?: { persist?: boolean }) => void

  setSessionPermissionMode: (sessionId: string, mode: PermissionMode) => void
  setSessionCodingMode: (
    sessionId: string,
    mode: CodingModeId,
    scope?: 'session' | 'global',
  ) => void
  resolveSessionCodingMode: (confirmed: boolean) => void
  setSessionDebugSubmode: (
    sessionId: string,
    submode: string,
    params: Record<string, unknown>,
  ) => void
  dismissAgentTaskNotification: (sessionId: string, toolUseId: string) => void
  stopGeneration: (sessionId: string) => void
  cancelTool: (sessionId: string, toolUseId?: string) => void
  reconcileStuckSession: (sessionId: string) => void
  loadHistory: (sessionId: string) => Promise<void>
  reloadHistory: (sessionId: string) => Promise<void>
  loadOlderHistory: (sessionId: string) => Promise<void>
  capMessageWindow: (sessionId: string) => void
  queueComposerPrefill: (
    sessionId: string,
    prefill: { text: string; attachments?: UIAttachment[] },
  ) => void

  setComposerDraft: (
    sessionId: string,
    draft: { text: string; attachments: UIAttachment[]; slashMenuOpen: boolean },
  ) => void

  clearComposerDraft: (sessionId: string) => void
  clearMessages: (sessionId: string) => void
  handleServerMessage: (sessionId: string, msg: ServerMessage) => void

  restoreRewind: (sessionId: string) => Promise<void>

  confirmSendAfterRewind: (sessionId: string) => Promise<void>

  cancelSendAfterRewind: (sessionId: string) => void

  requestModeSwitch: (sessionId: string, planPath: string) => void
  requestCuratorModeSwitch: (
    sessionId: string,
    implBlueprintPath: string,
    meta?: { slug?: string; template?: string; finalMdPath?: string },
  ) => void

  confirmModeSwitch: (sessionId: string, messageId: string) => void

  dismissModeSwitch: (sessionId: string, messageId: string) => void

  clearPendingEdits: (sessionId: string) => void

  clearKeptEdits: (sessionId: string) => void

  undoAllPendingEdits: (sessionId: string) => Promise<void>

  revertToTurnCheckpoint: (sessionId: string, suffixBatchIds: string[]) => Promise<void>

  undoPendingEditFile: (sessionId: string, path: string) => Promise<void>

  keepPendingEditFile: (sessionId: string, path: string) => void

  resumePlanExecution: (sessionId: string, planPath: string) => void

  applyPlanCardDocument: (sessionId: string, messageId: string, markdown: string) => void

  resumeCuratorExecution: (sessionId: string, implBlueprintPath: string) => void

  continueCuratorWriting: (sessionId: string) => void

  resetDebugPiiStats: (sessionId: string) => void
}

export const ASK_QUESTION_TOOL_NAMES = new Set([
  'ask_question',
  'ask_user',
  'AskQuestion',
  'AskUserQuestion',
])
export function isAskQuestionToolName(name: string | undefined | null): boolean {
  if (!name) return false
  return (
    ASK_QUESTION_TOOL_NAMES.has(name) ||
    name.toLowerCase() === 'ask_question' ||
    name.toLowerCase() === 'ask_user'
  )
}

function isPlanSaveCall(toolName: string | undefined | null, input: unknown): boolean {
  if (toolName !== 'update_plan') return false
  if (!input || typeof input !== 'object') return false
  const action = (input as Record<string, unknown>).action
  return typeof action === 'string' && action === 'save'
}

function isExitPlanModeCall(toolName: string | undefined | null): boolean {
  return toolName === 'exit_plan_mode'
}

function isUpdatePlanSetCall(
  toolName: string | undefined | null,
  input: unknown,
): boolean {
  if (toolName !== 'update_plan') return false
  if (!input || typeof input !== 'object') return false
  return (input as Record<string, unknown>).action === 'set'
}

function isUpdatePlanUpdateCall(
  toolName: string | undefined | null,
  input: unknown,
): boolean {
  if (toolName !== 'update_plan') return false
  if (!input || typeof input !== 'object') return false
  return (input as Record<string, unknown>).action === 'update'
}

function normalizeUpdatePlanStatus(raw: string): PlanTodoStatus {
  const v = raw.trim().toLowerCase()
  if (v === 'completed' || v === 'done' || v === 'finished') return 'completed'
  if (
    v === 'in_progress' ||
    v === 'in-progress' ||
    v === 'inprogress' ||
    v === 'doing'
  )
    return 'in_progress'
  if (v === 'cancelled' || v === 'canceled' || v === 'skipped')
    return 'cancelled'
  return 'pending'
}

function findLatestPlanCardIdx(messages: UIMessage[]): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    if (messages[i]!.type === 'plan_card') return i
  }
  return -1
}

function applyUpdatePlanSetToCard(
  card: Extract<UIMessage, { type: 'plan_card' }>,
  steps: unknown,
): Extract<UIMessage, { type: 'plan_card' }> {
  if (!Array.isArray(steps)) return card
  const todos: PlanTodo[] = []
  for (const s of steps) {
    if (!s || typeof s !== 'object') continue
    const o = s as Record<string, unknown>
    const id = typeof o.id === 'string' ? (o.id as string).trim() : ''

    const content =
      (typeof o.title === 'string' && (o.title as string)) ||
      (typeof o.content === 'string' && (o.content as string)) ||
      ''
    if (!id || !content) continue
    const statusRaw = typeof o.status === 'string' ? (o.status as string) : ''
    const status = statusRaw ? normalizeUpdatePlanStatus(statusRaw) : 'pending'
    todos.push({ id, content, status })
  }
  if (todos.length === 0) return card
  const merged = todos.map((todo) => {
    if (todo.status !== 'pending') return todo
    const idx = findPlanTodoIdx(card.todos, todo.id, todo.content)
    if (idx < 0) return todo
    const prev = card.todos[idx]!
    if (prev.status === 'pending') return todo
    const prevNotes = (prev as { notes?: string | null }).notes ?? null
    return {
      ...todo,
      status: prev.status,
      ...(prevNotes !== null ? { notes: prevNotes } : {}),
    }
  })
  const markdown = card.markdown
    ? rewritePlanMarkdownTodos(card.markdown, merged)
    : card.markdown
  return {
    ...card,
    todos: merged,
    markdown,
    pendingHydration: false,
    wasExecuted: true,
  }
}

function normalizePlanMatchKey(value: string): string {
  return value
    .toLowerCase()
    .replace(/[\p{P}\p{S}]/gu, ' ')
    .replace(/\s+/g, ' ')
    .trim()
}

function findPlanTodoIdx(
  todos: Extract<UIMessage, { type: 'plan_card' }>['todos'],
  stepId: string,
  fallbackContent: string,
): number {
  if (stepId) {
    const direct = todos.findIndex((t) => t.id === stepId)
    if (direct >= 0) return direct
    const stepKey = normalizePlanMatchKey(stepId)
    if (stepKey) {
      const byNormalizedId = todos.findIndex(
        (t) => normalizePlanMatchKey(t.id) === stepKey,
      )
      if (byNormalizedId >= 0) return byNormalizedId
      const byContentExact = todos.findIndex(
        (t) => normalizePlanMatchKey(t.content) === stepKey,
      )
      if (byContentExact >= 0) return byContentExact
      const byContentPrefix = todos.findIndex((t) => {
        const k = normalizePlanMatchKey(t.content)
        return k && (k.startsWith(stepKey) || stepKey.startsWith(k))
      })
      if (byContentPrefix >= 0) return byContentPrefix
    }
  }
  const contentKey = normalizePlanMatchKey(fallbackContent)
  if (!contentKey) return -1
  const byContentExact = todos.findIndex(
    (t) => normalizePlanMatchKey(t.content) === contentKey,
  )
  if (byContentExact >= 0) return byContentExact
  const byContentPrefix = todos.findIndex((t) => {
    const k = normalizePlanMatchKey(t.content)
    return k && (k.startsWith(contentKey) || contentKey.startsWith(k))
  })
  return byContentPrefix
}

function applyUpdatePlanUpdateToCard(
  card: Extract<UIMessage, { type: 'plan_card' }>,
  input: Record<string, unknown>,
): Extract<UIMessage, { type: 'plan_card' }> | null {
  const stepId = typeof input.step_id === 'string' ? (input.step_id as string).trim() : ''
  const titleHint =
    (typeof input.title === 'string' && (input.title as string).trim()) ||
    (typeof input.content === 'string' && (input.content as string).trim()) ||
    ''
  if (!stepId && !titleHint) return null
  const idx = findPlanTodoIdx(card.todos, stepId, titleHint)
  if (idx < 0) return null
  const statusRaw = typeof input.status === 'string' ? (input.status as string) : ''
  const newStatus = statusRaw ? normalizeUpdatePlanStatus(statusRaw) : null
  const notesRaw =
    typeof input.notes === 'string'
      ? (input.notes as string)
      : typeof input.note === 'string'
        ? (input.note as string)
        : null
  const current = card.todos[idx]!
  const statusChanged = !!newStatus && current.status !== newStatus
  const trimmedNotes = notesRaw?.trim() ?? null
  const currentNotes = (current as { notes?: string | null }).notes ?? null
  const notesChanged = trimmedNotes !== null && trimmedNotes !== currentNotes
  if (!statusChanged && !notesChanged) {
    return { ...card, wasExecuted: true }
  }
  const next = [...card.todos]
  const updatedStatus = statusChanged ? newStatus! : current.status
  const updatedNotes = notesChanged ? trimmedNotes : currentNotes
  next[idx] = {
    ...current,
    status: updatedStatus,
    ...(updatedNotes !== null ? { notes: updatedNotes } : {}),
  }
  const markdown = card.markdown
    ? rewritePlanMarkdownTodos(card.markdown, next)
    : card.markdown
  return {
    ...card,
    todos: next,
    markdown,
    pendingHydration: false,
    wasExecuted: true,
  }
}

function findLatestCuratorCardIdx(messages: UIMessage[]): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]!
    if (m.type === 'curator_card') return i
    if (m.type === 'user_text' || m.type === 'plan_question_answers') return -1
  }
  return -1
}

function applyUpdatePlanSetToCuratorCard(
  card: Extract<UIMessage, { type: 'curator_card' }>,
  steps: unknown,
): Extract<UIMessage, { type: 'curator_card' }> {
  if (!Array.isArray(steps)) return card
  const todos: PlanTodo[] = []
  for (const s of steps) {
    if (!s || typeof s !== 'object') continue
    const o = s as Record<string, unknown>
    const id = typeof o.id === 'string' ? (o.id as string).trim() : ''
    const content =
      (typeof o.title === 'string' && (o.title as string)) ||
      (typeof o.content === 'string' && (o.content as string)) ||
      ''
    if (!id || !content) continue
    const statusRaw = typeof o.status === 'string' ? (o.status as string) : ''
    const status = statusRaw ? normalizeUpdatePlanStatus(statusRaw) : 'pending'
    todos.push({ id, content, status })
  }
  if (todos.length === 0) return card
  return {
    ...card,
    todos,
    pendingHydration: false,
    wasExecuted: true,
  }
}

function applyUpdatePlanUpdateToCuratorCard(
  card: Extract<UIMessage, { type: 'curator_card' }>,
  input: Record<string, unknown>,
): Extract<UIMessage, { type: 'curator_card' }> | null {
  const existing = card.todos ?? []
  const stepId = typeof input.step_id === 'string' ? (input.step_id as string).trim() : ''
  const titleHint =
    (typeof input.title === 'string' && (input.title as string).trim()) ||
    (typeof input.content === 'string' && (input.content as string).trim()) ||
    ''
  if (!stepId && !titleHint) return null
  const idx = findPlanTodoIdx(existing, stepId, titleHint)
  if (idx < 0) return null
  const statusRaw = typeof input.status === 'string' ? (input.status as string) : ''
  const newStatus = statusRaw ? normalizeUpdatePlanStatus(statusRaw) : null
  const notesRaw =
    typeof input.notes === 'string'
      ? (input.notes as string)
      : typeof input.note === 'string'
        ? (input.note as string)
        : null
  const current = existing[idx]!
  const statusChanged = !!newStatus && current.status !== newStatus
  const trimmedNotes = notesRaw?.trim() ?? null
  const currentNotes = (current as { notes?: string | null }).notes ?? null
  const notesChanged = trimmedNotes !== null && trimmedNotes !== currentNotes
  if (!statusChanged && !notesChanged) {
    return { ...card, wasExecuted: true }
  }
  const next = [...existing]
  next[idx] = {
    ...current,
    status: statusChanged ? newStatus! : current.status,
    ...(notesChanged ? { notes: trimmedNotes } : {}),
  }
  return {
    ...card,
    todos: next,
    pendingHydration: false,
    wasExecuted: true,
  }
}

function makePendingPlanCardFromUpdatePlan(
  input: unknown,
  sourceToolUseId: string,
): Extract<UIMessage, { type: 'plan_card' }> {
  const obj = input && typeof input === 'object' ? (input as Record<string, unknown>) : {}
  const planName =
    (typeof obj.plan_name === 'string' && (obj.plan_name as string)) ||
    (typeof obj.title === 'string' && (obj.title as string)) ||
    'plan'
  const fileName = planName.endsWith('.plan.md') ? planName : `${planName}.plan.md`
  const title =
    typeof obj.title === 'string' && (obj.title as string).trim()
      ? (obj.title as string).trim()
      : planName
  const overview =
    typeof obj.description === 'string' ? (obj.description as string) : ''
  return {
    id: nextId(),
    type: 'plan_card',
    timestamp: Date.now(),
    planPath: '',
    fileName,
    title,
    overview,
    todos: [],
    status: 'writing',
    sourceToolUseId,
    source: 'update_plan_save',
  }
}

function makePendingPlanCardFromExitPlanMode(
  input: unknown,
  sourceToolUseId: string,
): Extract<UIMessage, { type: 'plan_card' }> {
  const obj = input && typeof input === 'object' ? (input as Record<string, unknown>) : {}
  const planContent = typeof obj.plan_content === 'string' ? (obj.plan_content as string) : ''
  const parsed = parsePlanMarkdown(planContent)
  const todos = parsed.todos.map((t) => ({
    id: t.id,
    content: t.content,
    status: t.status as PlanTodoStatus,
  }))
  const fileName = parsed.name ? `${parsed.name}.plan.md` : 'plan.plan.md'
  return {
    id: nextId(),
    type: 'plan_card',
    timestamp: Date.now(),
    planPath: '',
    fileName,
    title: parsed.title || parsed.name || 'Plan',
    overview: parsed.overview,
    todos,
    markdown: planContent,
    status: 'writing',
    sourceToolUseId,
    source: 'exit_plan_mode',
  }
}

function upgradePlanCardFromResult(
  card: Extract<UIMessage, { type: 'plan_card' }>,
  rawContent: unknown,
  isError: boolean,
): Extract<UIMessage, { type: 'plan_card' }> {
  if (isError) {
    return {
      ...card,
      status: 'failed',
      error:
        (typeof rawContent === 'string'
          ? rawContent
          : extractTextFromRawContent(rawContent)) ||
        (t('plan.failedHint') || 'Plan generation did not finish.'),
    }
  }
  const text = typeof rawContent === 'string' ? rawContent : extractTextFromRawContent(rawContent)

  if (card.source === 'exit_plan_mode') {
    const exited = parseExitPlanModeResult(text)
    if (!exited) {
      return { ...card, status: 'completed' }
    }
    const wrappedMd = exited.markdown
    if (wrappedMd) {
      const parsed = parsePlanMarkdown(wrappedMd)
      const todos = parsed.todos.map((t) => ({
        id: t.id,
        content: t.content,
        status: t.status as PlanTodoStatus,
      }))
      return {
        ...card,
        status: 'completed',
        planPath: exited.planPath || card.planPath,
        fileName:
          planFileNameFromPath(exited.planPath) || card.fileName,
        title: parsed.title || parsed.name || card.title,
        overview: parsed.overview || card.overview,
        todos: todos.length > 0 ? todos : card.todos,
        markdown: wrappedMd,
      }
    }
    return {
      ...card,
      status: 'completed',
      planPath: exited.planPath,
      fileName: planFileNameFromPath(exited.planPath) || card.fileName,
    }
  }

  const saved = parseSavedPlanResult(text)
  if (!saved) return { ...card, status: 'completed', markdown: text }
  const parsed = parsePlanMarkdown(saved.markdown)
  const todos = parsed.todos.map((t) => ({
    id: t.id,
    content: t.content,
    status: t.status as PlanTodoStatus,
  }))
  return {
    ...card,
    status: 'completed',
    planPath: saved.planPath,
    fileName: planFileNameFromPath(saved.planPath) || card.fileName,
    title: parsed.title || card.title,
    overview: parsed.overview || card.overview,
    todos: todos.length > 0 ? todos : card.todos,
    markdown: saved.markdown,
  }
}

function findReplaceablePlanCardIdx(messages: UIMessage[]): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]!
    if (m.type === 'plan_card') {
      if (m.source === 'update_plan_save') return i
      if (m.source === 'exit_plan_mode' && m.status === 'writing') return i
      return -1
    }
    if (
      m.type === 'user_text' ||
      m.type === 'mode_switch_card' ||
      m.type === 'curator_card' ||
      m.type === 'plan_question_answers'
    ) {
      return -1
    }
  }
  return -1
}

function isExitCuratorModeCall(toolName: string | undefined | null): boolean {
  return toolName === 'exit_curator_mode'
}

function deriveTitleFromCuratorBody(body: string, fallback: string): string {
  for (const line of body.split('\n')) {
    const m = line.match(/^#\s+(.+?)\s*$/)
    if (m && m[1]) return m[1].trim()
  }
  return fallback
}

function makePendingCuratorCardFromExitCuratorMode(
  input: unknown,
  sourceToolUseId: string,
): Extract<UIMessage, { type: 'curator_card' }> {
  const obj = input && typeof input === 'object' ? (input as Record<string, unknown>) : {}
  const finalContent =
    typeof obj.final_content === 'string' ? (obj.final_content as string) : ''
  const slug =
    (typeof obj.slug === 'string' && (obj.slug as string).trim()) ||
    'curator'
  const template =
    (typeof obj.template === 'string' && (obj.template as string).trim()) ||
    'document'
  const title = deriveTitleFromCuratorBody(finalContent, slug)
  return {
    id: nextId(),
    type: 'curator_card',
    timestamp: Date.now(),
    slug,
    template,
    finalMdPath: '',
    implBlueprintPath: '',
    docxPath: undefined,
    title,
    body: finalContent,
    status: 'writing',
    sourceToolUseId,
  }
}

function upgradeCuratorCardFromResult(
  card: Extract<UIMessage, { type: 'curator_card' }>,
  rawContent: unknown,
  isError: boolean,
): Extract<UIMessage, { type: 'curator_card' }> {
  const text =
    typeof rawContent === 'string' ? rawContent : extractTextFromRawContent(rawContent)
  if (isError) {
    return { ...card, status: 'failed', error: text.trim() || undefined }
  }
  const parsed = parseCuratorEnvelopeForCard(text)
  if (!parsed) {
    return { ...card, status: 'completed', error: undefined }
  }
  return {
    ...card,
    status: 'completed',
    error: undefined,
    slug: parsed.slug || card.slug,
    template: parsed.template || card.template,
    finalMdPath: parsed.finalMdPath || card.finalMdPath,
    implBlueprintPath: parsed.implBlueprintPath || card.implBlueprintPath,
    docxPath: parsed.docxPath ?? card.docxPath,
    title: parsed.title || card.title,
    body: parsed.body || card.body,
  }
}

function resolveDanglingCuratorCards(messages: UIMessage[]): UIMessage[] {
  let changed = false
  const next = messages.map((m) => {
    if (m.type === 'curator_card' && m.status === 'writing') {
      changed = true
      return {
        ...m,
        status: 'failed' as const,
        error:
          m.error ||
          (t('curator.interrupted') ||
            'The turn ended before the document was finalized. Ask the assistant to continue.'),
      }
    }
    if (m.type === 'plan_card' && m.status === 'writing') {
      changed = true
      return {
        ...m,
        status: 'failed' as const,
        error:
          m.error ||
          (t('plan.failedHint') ||
            'Plan generation did not finish. Ask the assistant to try again.'),
      }
    }
    return m
  })
  return changed ? next : messages
}

function findReplaceableCuratorCardIdx(messages: UIMessage[]): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]!
    if (m.type === 'curator_card') {
      if (m.status === 'writing' || m.status === 'failed') return i
      return -1
    }
    if (
      m.type === 'user_text' ||
      m.type === 'mode_switch_card' ||
      m.type === 'plan_question_answers'
    ) {
      return -1
    }
  }
  return -1
}

function extractTextFromRawContent(content: unknown): string {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    return content
      .map((c) =>
        c && typeof c === 'object' && typeof (c as { text?: unknown }).text === 'string'
          ? ((c as { text: string }).text)
          : '',
      )
      .filter((s) => s.length > 0)
      .join('\n')
  }
  if (content && typeof content === 'object') {
    const t = (content as { text?: unknown }).text
    if (typeof t === 'string') return t
  }
  return ''
}

function normalizeAnswerValue(value: unknown): string | string[] {
  if (value == null) return ''
  if (typeof value === 'string') return value
  if (Array.isArray(value)) {
    const labels = value
      .map((entry) => {
        if (typeof entry === 'string') return entry.trim()
        if (entry && typeof entry === 'object') {
          const o = entry as Record<string, unknown>
          const candidate =
            (typeof o.label === 'string' && o.label) ||
            (typeof o.text === 'string' && o.text) ||
            (typeof o.id === 'string' && o.id) ||
            ''
          return typeof candidate === 'string' ? candidate.trim() : ''
        }
        return ''
      })
      .filter((s) => s.length > 0)
    if (labels.length === 0) return ''
    if (labels.length === 1) return labels[0] ?? ''
    return labels
  }
  return ''
}

function collectPlanAnswerItems(
  rawInput: unknown,
  updatedInput: Record<string, unknown>,
): Array<{ question: string; answer: string | string[] }> {
  const skipped = updatedInput.skipped === true
  const answers =
    updatedInput.answers && typeof updatedInput.answers === 'object'
      ? (updatedInput.answers as Record<string, unknown>)
      : {}
  const out: Array<{ question: string; answer: string | string[] }> = []
  const obj =
    rawInput && typeof rawInput === 'object' ? (rawInput as Record<string, unknown>) : {}
  const rawQs = Array.isArray(obj.questions)
    ? (obj.questions as unknown[])
    : typeof obj.question === 'string' || typeof obj.prompt === 'string'
      ? [obj]
      : []
  rawQs.forEach((q, idx) => {
    if (!q || typeof q !== 'object') return
    const o = q as Record<string, unknown>
    const prompt =
      typeof o.prompt === 'string'
        ? (o.prompt as string)
        : typeof o.question === 'string'
          ? (o.question as string)
          : ''
    if (!prompt.trim()) return
    const qid = typeof o.id === 'string' && o.id ? (o.id as string) : `q-${idx}`
    const answer = (() => {
      if (skipped) return ''
      const direct = answers[qid]
      if (direct !== undefined) {
        const normalized = normalizeAnswerValue(direct)
        if ((typeof normalized === 'string' && normalized) || Array.isArray(normalized)) {
          return normalized
        }
      }
      const byPrompt = answers[prompt]
      if (byPrompt !== undefined) {
        const normalized = normalizeAnswerValue(byPrompt)
        if ((typeof normalized === 'string' && normalized) || Array.isArray(normalized)) {
          return normalized
        }
      }
      return ''
    })()
    out.push({ question: prompt, answer })
  })
  return out
}

function syncTasksAfterTurnEnd(sessionId: string, stopped: boolean) {
  void useCLITaskStore
    .getState()
    .refreshTasks(sessionId)
    .finally(() => {
      useCLITaskStore.getState().finalizeTasksOnTurnEnd(sessionId, stopped)
    })
}

const TODO_TOOL_NAMES = new Set(['TodoWrite', 'todo_write'])
const TASK_TOOL_NAMES = new Set([
  'TaskCreate',
  'TaskUpdate',
  'TaskGet',
  'TaskList',
  'TodoWrite',
  'task_create',
  'task_update',
  'task_get',
  'task_list',
  'todo_write',
])
const pendingTaskToolUseIdsBySession = new Map<string, Set<string>>()

function getOrCreateSessionSet(
  bucket: Map<string, Set<string>>,
  sessionId: string,
): Set<string> {
  let set = bucket.get(sessionId)
  if (!set) {
    set = new Set<string>()
    bucket.set(sessionId, set)
  }
  return set
}

function deleteSessionToolUseId(
  bucket: Map<string, Set<string>>,
  sessionId: string,
  toolUseId: string,
): boolean {
  const set = bucket.get(sessionId)
  if (!set) return false
  const had = set.delete(toolUseId)
  if (set.size === 0) bucket.delete(sessionId)
  return had
}

const SUBAGENT_PARENT_TOOL_NAMES = new Set([
  'delegate',
  'delegate_parallel',
  'swarm',
  'llm_task',
  'task',
  'Task',
  'Agent',
  'spawn_workers',
])

function isSubagentParentTool(name: string): boolean {
  return SUBAGENT_PARENT_TOOL_NAMES.has(name)
}

const BROWSER_FAMILY_TOOLS = new Set([
  'browser',
  'browser_open',
  'browser_delegate',
  'text_browser',
])

function isBrowserFamilyTool(name: string): boolean {
  return BROWSER_FAMILY_TOOLS.has(name)
}

function extractBrowserToolUrl(input: unknown): string | null {
  if (!input || typeof input !== 'object') return null
  const obj = input as Record<string, unknown>
  for (const key of ['url', 'href', 'target_url', 'targetUrl']) {
    const v = obj[key]
    if (typeof v === 'string' && v.trim()) return v.trim()
  }
  return null
}

function isExternalWebUrl(url: string | null): boolean {
  if (!url) return false
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return false
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') return false
  const host = parsed.hostname.toLowerCase()
  if (
    host === 'localhost' ||
    host === '127.0.0.1' ||
    host === '0.0.0.0' ||
    host === '::1' ||
    host.endsWith('.localhost')
  ) {
    return false
  }
  return true
}

function isDesignerSession(sessionId: string): boolean {
  const mode =
    useChatStore.getState().sessionCodingMode[sessionId] ??
    useSettingsStore.getState().codingMode
  return mode === 'designer'
}

function subagentChunkToEntry(
  kind: string,
  delta: string,
): AgentTimelineEntry {
  const text = delta ?? ''
  switch (kind) {
    case 'Thinking':
      return { kind: 'thinking', text }
    case 'ToolCall': {
      const parts = text.split(/\s+/).filter(Boolean)
      const name = parts[0] ?? text
      const summary = parts.slice(1).join(' ')
      return { kind: 'tool_call', name, summary }
    }
    case 'ToolResult': {
      const isError = /^error:/i.test(text) || /^failed:/i.test(text)
      const parts = text.split(/\s+/).filter(Boolean)
      const name = parts[0] ?? ''
      const preview = parts.slice(1).join(' ') || text
      return { kind: 'tool_result', name, preview, isError }
    }
    case 'Status':
      return { kind: 'status', text }
    case 'Chunk':
    default:
      return { kind: 'text', text }
  }
}

type PendingWorkerTimelineEvent = {
  parentToolUseId: string
  workerId: string
  action: string
  detail: string
  at: number
}

const pendingWorkerEventsBySession = new Map<string, PendingWorkerTimelineEvent[]>()
const workerEventFlushTimerBySession = new Map<string, ReturnType<typeof setTimeout>>()

function flushWorkerTimelineEvents(sessionId: string): void {
  workerEventFlushTimerBySession.delete(sessionId)
  const events = pendingWorkerEventsBySession.get(sessionId)
  pendingWorkerEventsBySession.delete(sessionId)
  if (!events || events.length === 0) return
  useChatStore.setState((state) => ({
    sessions: updateSessionIn(state.sessions, sessionId, (s) => {
      let timelines = s.subagentTimelines
      for (const ev of events) {
        const bucket = timelines[ev.parentToolUseId] ?? {
          parentToolUseId: ev.parentToolUseId,
          parentToolName: 'spawn_workers',
          agents: {},
        }
        const prevTimeline: AgentTimeline = bucket.agents[ev.workerId] ?? {
          agentId: ev.workerId,
          status: 'running',
          entries: [],
          startedAt: ev.at,
          updatedAt: ev.at,
        }
        const delta = ev.detail.trim() ? `${ev.action}: ${ev.detail}` : ev.action
        const entry = subagentChunkToEntry('Status', delta)
        const nextTimeline = appendTimelineEntry(prevTimeline, entry, ev.at)
        timelines = {
          ...timelines,
          [ev.parentToolUseId]: {
            ...bucket,
            agents: {
              ...bucket.agents,
              [ev.workerId]: nextTimeline,
            },
          },
        }
      }
      return { subagentTimelines: timelines }
    }),
  }))
}

function updateWorkerSubagentTimeline(
  sessionId: string,
  parentToolUseId: string,
  workerId: string,
  action: string,
  detail: string,
) {
  const ev: PendingWorkerTimelineEvent = {
    parentToolUseId,
    workerId,
    action,
    detail,
    at: Date.now(),
  }
  const list = pendingWorkerEventsBySession.get(sessionId)
  if (list) list.push(ev)
  else pendingWorkerEventsBySession.set(sessionId, [ev])
  if (workerEventFlushTimerBySession.has(sessionId)) return
  workerEventFlushTimerBySession.set(
    sessionId,
    setTimeout(() => flushWorkerTimelineEvents(sessionId), 80),
  )
}

const MAX_TIMELINE_ENTRY_CHARS = 100_000
const MAX_TIMELINE_ENTRIES = 800

function capTimelineText(text: string): string {
  if (text.length <= MAX_TIMELINE_ENTRY_CHARS) return text
  return `… [truncated]\n${text.slice(text.length - MAX_TIMELINE_ENTRY_CHARS)}`
}

function appendTimelineEntry(
  timeline: AgentTimeline,
  entry: AgentTimelineEntry,
  now: number,
): AgentTimeline {
  const entries = timeline.entries
  const last = entries[entries.length - 1]
  if (
    last &&
    (entry.kind === 'text' || entry.kind === 'thinking') &&
    last.kind === entry.kind
  ) {
    const merged: AgentTimelineEntry = {
      kind: entry.kind,
      text: capTimelineText(`${last.text}${entry.text}`),
    }
    return {
      ...timeline,
      entries: [...entries.slice(0, -1), merged],
      updatedAt: now,
    }
  }
  const appended = [...entries, entry]
  return {
    ...timeline,
    entries:
      appended.length > MAX_TIMELINE_ENTRIES
        ? appended.slice(appended.length - MAX_TIMELINE_ENTRIES)
        : appended,
    updatedAt: now,
  }
}

const MAX_TIMELINE_FINAL_OUTPUT_CHARS = 20_000

function capFinalOutputText(text: string | undefined): string | undefined {
  if (text === undefined) return undefined
  if (text.length <= MAX_TIMELINE_FINAL_OUTPUT_CHARS) return text
  return `${text.slice(0, MAX_TIMELINE_FINAL_OUTPUT_CHARS)}\n… [truncated ${text.length - MAX_TIMELINE_FINAL_OUTPUT_CHARS} chars]`
}

function extractToolResultText(content: unknown): string | undefined {
  if (typeof content === 'string') return capFinalOutputText(content)
  if (Array.isArray(content)) {
    return capFinalOutputText(
      content
        .map((chunk) => {
          if (typeof chunk === 'string') return chunk
          if (chunk && typeof chunk === 'object' && 'text' in chunk) {
            const t = (chunk as { text?: unknown }).text
            return typeof t === 'string' ? t : ''
          }
          return ''
        })
        .filter(Boolean)
        .join('\n') || undefined,
    )
  }
  if (content && typeof content === 'object') {
    try {
      return capFinalOutputText(JSON.stringify(content))
    } catch {
      return undefined
    }
  }
  return undefined
}

function markSubagentBucketStatus(
  timelines: Record<string, SubagentTimelineBucket>,
  parentId: string,
  status: AgentTimeline['status'],
  finalOutput?: string,
): Record<string, SubagentTimelineBucket> {
  const bucket = timelines[parentId]
  if (!bucket) return timelines
  const nextAgents: Record<string, AgentTimeline> = {}
  for (const [agentId, tl] of Object.entries(bucket.agents)) {
    nextAgents[agentId] = {
      ...tl,
      status: tl.status === 'error' ? tl.status : status,
      finalOutput: finalOutput ?? tl.finalOutput,
      updatedAt: Date.now(),
    }
  }
  return { ...timelines, [parentId]: { ...bucket, agents: nextAgents } }
}

const planModeBlockedToolUseIdsBySession = new Map<string, Set<string>>()

const updatePlanInlineToolUseIdsBySession = new Map<string, Set<string>>()

let msgCounter = 0
const nextId = () => `msg-${++msgCounter}-${Date.now()}`

const KNOWN_SYSTEM_NOTIFICATION_SUBTYPES = new Set<string>([
  'ws_reconnecting',
  'ws_unreachable',
  'ws_handler_error',
  'ws_frame_gap',
  'runtime_config_updated',
  'runtime_config_persist_failed',
  'runtime_config_validation_failed',
  'runtime_config_apply_failed',
  'coding_mode_confirm_required',
  'coding_mode_updated',
  'coding_mode_auto_resolved',
  'permission_mode_updated',
  'pii_config_updated',
  'slash_commands',
  'slash_command_result',
  'resource_wait_started',
  'resource_wait_resolved',
  'mcp_servers_updated',
  'debug_pii_stats',
  'task_notification',
  'file_edit',
  'status_detail',
  'command_preview',
  'cancelling',
  'subagent_chunk',
  'debug_tab_bound',
  'debug_tab_unbound',
  'prototype_ref_bound',
  'prototype_ref_unbound',
])

function stripNoModelErrorMessages(messages: UIMessage[]): UIMessage[] {
  let mutated = false
  const next: UIMessage[] = []
  for (const m of messages) {
    if (m.type === 'error' && isNoModelConfiguredError(m.message, m.code)) {
      mutated = true
      continue
    }
    next.push(m)
  }
  return mutated ? next : messages
}

let lastNoModelToastAt = 0
function emitNoModelWarning(sessionId?: string): void {
  const now = Date.now()
  if (now - lastNoModelToastAt < 3000) return
  lastNoModelToastAt = now
  useUIStore.getState().addToast({
    type: 'warning',
    message: t('chat.noModel.warning'),
    duration: 8000,
    sessionId,
    action: {
      label: t('chat.noModel.openSettings'),
      onClick: () => useUIStore.getState().openSettingsOverlay('providers'),
    },
  })
}

const pendingDeltaBySession = new Map<string, string>()
const flushTimerBySession = new Map<string, ScheduledFlushHandle>()
const pendingThinkingBySession = new Map<string, string>()
const thinkingFlushTimerBySession = new Map<string, ScheduledFlushHandle>()
const pendingDeltaFirstAt = new Map<string, number>()
const pendingThinkingFirstAt = new Map<string, number>()
const lastStreamActivityAtBySession = new Map<string, number>()
const continuationPrefixBySession = new Map<string, string>()
const FLUSH_HIGH_WATER_CHARS = 96
const FLUSH_HIGH_WATER_MS = 80
const BUSY_FLUSH_MAX_DEFER_MS = 250
const lastBusyFlushAtBySession = new Map<string, number>()

const deferredDeltaFlush = new Map<string, () => void>()
const deferredThinkingFlush = new Map<string, () => void>()
let busyIdleUnsub: (() => void) | null = null

type QueuedServerFrame = { sessionId: string; msg: ServerMessage }
const serverFrameQueue: QueuedServerFrame[] = []
let serverFrameDrainRaf: number | null = null
let serverFrameDrainTimer: ReturnType<typeof setTimeout> | null = null
const SERVER_FRAME_DRAIN_FALLBACK_MS = 32

function drainServerFrameQueue(): void {
  if (serverFrameDrainRaf !== null) {
    if (typeof window !== 'undefined' && typeof window.cancelAnimationFrame === 'function') {
      window.cancelAnimationFrame(serverFrameDrainRaf)
    }
    serverFrameDrainRaf = null
  }
  if (serverFrameDrainTimer !== null) {
    clearTimeout(serverFrameDrainTimer)
    serverFrameDrainTimer = null
  }
  if (serverFrameQueue.length === 0) return
  const drained = serverFrameQueue.splice(0, serverFrameQueue.length)
  const store = useChatStore.getState()
  for (const frame of drained) {
    try {
      store.handleServerMessage(frame.sessionId, frame.msg)
    } catch (err) {
      console.warn('[chatStore] server frame handling failed', err)
      const session = useChatStore.getState().sessions[frame.sessionId]
      if (session && isSessionUiBusy(session.chatState)) {
        dirtyMidTurnSessions.add(frame.sessionId)
      } else {
        void useChatStore.getState().reloadHistory(frame.sessionId)
      }
    }
  }
}

function enqueueServerFrame(sessionId: string, msg: ServerMessage): void {
  serverFrameQueue.push({ sessionId, msg })
  if (serverFrameDrainRaf !== null || serverFrameDrainTimer !== null) return
  if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
    serverFrameDrainRaf = window.requestAnimationFrame(() => {
      serverFrameDrainRaf = null
      drainServerFrameQueue()
    })
  }
  serverFrameDrainTimer = setTimeout(() => {
    serverFrameDrainTimer = null
    drainServerFrameQueue()
  }, SERVER_FRAME_DRAIN_FALLBACK_MS)
}

function ensureBusyIdleFlush(): void {
  if (busyIdleUnsub) return
  busyIdleUnsub = onWindowIdle(() => {
    busyIdleUnsub?.()
    busyIdleUnsub = null
    const deltaFlushes = Array.from(deferredDeltaFlush.values())
    deferredDeltaFlush.clear()
    for (const fn of deltaFlushes) fn()
    const thinkingFlushes = Array.from(deferredThinkingFlush.values())
    deferredThinkingFlush.clear()
    for (const fn of thinkingFlushes) fn()
  })
}

const runtimeSyncFailureToastSessions = new Set<string>()
const runtimeSyncRetrySessions = new Set<string>()

const STUCK_RECONCILE_QUIET_MS = 2000
const stuckReconcileDeferTimers = new Map<string, ReturnType<typeof setTimeout>>()

const activeTurnSeqBySession = new Map<string, number>()

const resyncingSessions = new Set<string>()

const historyChangedReloadAt = new Map<string, number>()

const dirtyMidTurnSessions = new Set<string>()

const historyGenerationBySession = new Map<string, number>()
const bumpHistoryGeneration = (sessionId: string) => {
  historyGenerationBySession.set(
    sessionId,
    (historyGenerationBySession.get(sessionId) ?? 0) + 1,
  )
}

const capReloadInFlight = new Set<string>()

function drainQueuedForSession(sessionId: string): void {
  const session =
    useSessionStore.getState().sessions.find((s) => s.id === sessionId) ?? null
  const wsKey = workspaceKeyFor(session, sessionId)
  const queueLen = useWorkspaceQueueStore.getState().queues[wsKey]?.length ?? 0
  if (queueLen === 0) return
  void tryDrainWorkspace(wsKey)
}

function handleRuntimeSyncFailure(sessionId: string, err: unknown): void {
  console.error(`[chatStore] runtime sync failed for session ${sessionId}`, err)
  runtimeSyncRetrySessions.add(sessionId)
  if (!runtimeSyncFailureToastSessions.has(sessionId)) {
    runtimeSyncFailureToastSessions.add(sessionId)
    useUIStore.getState().addToast({
      type: 'warning',
      message: t('runtime.syncFailed'),
      duration: 6000,
      sessionId,
    })
  }
}

const HISTORY_RETRY_DELAYS_MS = [2000, 5000]
const historyLoadRetryAttempts = new Map<string, number>()
const historyLoadRetryTimers = new Map<string, ReturnType<typeof setTimeout>>()

function scheduleHistoryLoadRetry(
  sessionId: string,
  retry: () => void | Promise<void>,
): void {
  const attempt = historyLoadRetryAttempts.get(sessionId) ?? 0
  if (attempt >= HISTORY_RETRY_DELAYS_MS.length) {
    historyLoadRetryAttempts.delete(sessionId)
    useUIStore.getState().addToast({
      type: 'error',
      message: t('chat.loadHistoryFailed'),
      duration: 5000,
    })
    return
  }
  historyLoadRetryAttempts.set(sessionId, attempt + 1)
  const existing = historyLoadRetryTimers.get(sessionId)
  if (existing) clearTimeout(existing)
  historyLoadRetryTimers.set(
    sessionId,
    setTimeout(() => {
      historyLoadRetryTimers.delete(sessionId)
      void retry()
    }, HISTORY_RETRY_DELAYS_MS[attempt]),
  )
}

function clearHistoryLoadRetry(sessionId: string): void {
  historyLoadRetryAttempts.delete(sessionId)
  const timer = historyLoadRetryTimers.get(sessionId)
  if (timer) {
    clearTimeout(timer)
    historyLoadRetryTimers.delete(sessionId)
  }
}

let elapsedTickerId: ReturnType<typeof setInterval> | null = null
const BACKGROUND_ELAPSED_TICK_SECONDS = 5
const elapsedLastTickAtBySession = new Map<string, number>()

function ensureElapsedTicker() {
  if (elapsedTickerId !== null) return
  elapsedTickerId = setInterval(() => {
    const activeTabId = useTabStore.getState().activeTabId
    const now = Date.now()
    useChatStore.setState((s) => {
      let changed = false
      let anyRunning = false
      const next: typeof s.sessions = { ...s.sessions }
      for (const [id, sess] of Object.entries(s.sessions)) {
        if (sess.chatState === 'idle') {
          elapsedLastTickAtBySession.delete(id)
          continue
        }
        anyRunning = true
        const lastAt = elapsedLastTickAtBySession.get(id)
        if (lastAt === undefined) {
          elapsedLastTickAtBySession.set(id, now)
          continue
        }
        const elapsedMs = now - lastAt
        if (id !== activeTabId && elapsedMs < BACKGROUND_ELAPSED_TICK_SECONDS * 1000) {
          continue
        }
        const deltaSeconds = Math.max(1, Math.round(elapsedMs / 1000))
        elapsedLastTickAtBySession.set(id, lastAt + deltaSeconds * 1000)
        next[id] = { ...sess, elapsedSeconds: sess.elapsedSeconds + deltaSeconds }
        changed = true
      }
      if (!anyRunning && elapsedTickerId !== null) {
        clearInterval(elapsedTickerId)
        elapsedTickerId = null
        elapsedLastTickAtBySession.clear()
      }
      return changed ? { sessions: next } : s
    })
  }, 1000)
}

function nowMs(): number {
  if (typeof performance !== 'undefined' && performance && typeof performance.now === 'function') {
    return performance.now()
  }
  return Date.now()
}

type ScheduledFlushHandle = { kind: 'raf' | 'timeout'; id: number }

function scheduleRafCallback(cb: () => void): ScheduledFlushHandle {
  if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
    return { kind: 'raf', id: window.requestAnimationFrame(cb) }
  }
  return { kind: 'timeout', id: setTimeout(cb, 16) as unknown as number }
}

function cancelScheduledFlush(handle: ScheduledFlushHandle): void {
  if (
    handle.kind === 'raf' &&
    typeof window !== 'undefined' &&
    typeof window.cancelAnimationFrame === 'function'
  ) {
    window.cancelAnimationFrame(handle.id)
  } else {
    clearTimeout(handle.id as unknown as ReturnType<typeof setTimeout>)
  }
}

const pendingDesignCanvasReveal = new Set<string>()

function revealDesignCanvasIfPending(sessionId: string) {
  if (!pendingDesignCanvasReveal.delete(sessionId)) return
  useDesignerCanvasStore.getState().setVisible(sessionId, true)
}

const lastDirectSendBySession = new Map<
  string,
  {
    content: string
    attachments?: AttachmentRef[]
    displayContent?: string
    designGeneration?: DesignGenerationOptions
    at: number
  }
>()
const DIRECT_SEND_REQUEUE_WINDOW_MS = 30_000

function takeLastDirectSend(sessionId: string) {
  const entry = lastDirectSendBySession.get(sessionId)
  if (!entry) return null
  lastDirectSendBySession.delete(sessionId)
  if (Date.now() - entry.at > DIRECT_SEND_REQUEUE_WINDOW_MS) return null
  return entry
}

function designRefFieldsFrom(
  designGeneration?: DesignGenerationOptions,
): Partial<Extract<UIMessage, { type: 'user_text' }>> {
  if (!designGeneration?.refArtifact) return {}
  return {
    designRef: designGeneration.refArtifact,
    ...(designGeneration.refArtifactName
      ? { designRefName: designGeneration.refArtifactName }
      : {}),
    ...(designGeneration.refElement
      ? {
          designRefElement: designGeneration.refElement,
          designRefElementLabel:
            designGeneration.refElementLabel ?? designGeneration.refElement,
        }
      : {}),
  }
}

function consumePendingDelta(sessionId: string): string {
  const timer = flushTimerBySession.get(sessionId)
  if (timer !== undefined) {
    cancelScheduledFlush(timer)
    flushTimerBySession.delete(sessionId)
  }
  deferredDeltaFlush.delete(sessionId)
  const text = pendingDeltaBySession.get(sessionId) ?? ''
  pendingDeltaBySession.delete(sessionId)
  pendingDeltaFirstAt.delete(sessionId)
  return text
}

function consumePendingThinking(sessionId: string): string {
  const timer = thinkingFlushTimerBySession.get(sessionId)
  if (timer !== undefined) {
    cancelScheduledFlush(timer)
    thinkingFlushTimerBySession.delete(sessionId)
  }
  deferredThinkingFlush.delete(sessionId)
  const text = pendingThinkingBySession.get(sessionId) ?? ''
  pendingThinkingBySession.delete(sessionId)
  pendingThinkingFirstAt.delete(sessionId)
  return text
}

type ThinkingActivePatch = {
  activeThinkingId: string | null
  activeThinkingContent: string
  activeThinkingStartedAt: number | null
  activeThinkingLastChunkAt: number | null
}

function mergePendingThinkingIntoActive(
  state: {
    activeThinkingId: string | null
    activeThinkingContent: string
    activeThinkingStartedAt: number | null
    activeThinkingLastChunkAt: number | null
  },
  sessionId: string,
): ThinkingActivePatch {
  const buffered = consumePendingThinking(sessionId)
  if (!buffered.trim()) {
    if (!state.activeThinkingContent.trim()) {
      return {
        activeThinkingId: null,
        activeThinkingContent: '',
        activeThinkingStartedAt: null,
        activeThinkingLastChunkAt: null,
      }
    }
    return {
      activeThinkingId: state.activeThinkingId,
      activeThinkingContent: state.activeThinkingContent,
      activeThinkingStartedAt: state.activeThinkingStartedAt,
      activeThinkingLastChunkAt: state.activeThinkingLastChunkAt,
    }
  }
  const hasActive = Boolean(state.activeThinkingId)
  const now = Date.now()
  const id = hasActive ? (state.activeThinkingId as string) : nextId()
  const startedAt = hasActive
    ? (state.activeThinkingStartedAt ?? now)
    : now
  const prevContent = hasActive ? state.activeThinkingContent : ''
  return {
    activeThinkingId: id,
    activeThinkingContent: prevContent + buffered,
    activeThinkingStartedAt: startedAt,
    activeThinkingLastChunkAt: now,
  }
}

function sealThinkingForSession(
  sessionId: string,
  state: {
    messages: UIMessage[]
    activeThinkingId: string | null
    activeThinkingContent: string
    activeThinkingStartedAt: number | null
    activeThinkingLastChunkAt: number | null
  },
): UIMessage[] {
  const patch = mergePendingThinkingIntoActive(state, sessionId)
  return sealThinkingFromState({
    messages: state.messages,
    activeThinkingId: patch.activeThinkingId,
    activeThinkingContent: patch.activeThinkingContent,
    activeThinkingStartedAt: patch.activeThinkingStartedAt,
  })
}

function flushPendingDeltaIntoStreaming(
  sessionId: string,
  prevStreamingText: string,
): string {
  const buffered = consumePendingDelta(sessionId)
  if (!buffered) return prevStreamingText
  return prevStreamingText + buffered
}

function clearSessionStreamBuffers(sessionId: string): void {
  const deltaTimer = flushTimerBySession.get(sessionId)
  if (deltaTimer !== undefined) {
    cancelScheduledFlush(deltaTimer)
    flushTimerBySession.delete(sessionId)
  }
  deferredDeltaFlush.delete(sessionId)
  pendingDeltaBySession.delete(sessionId)
  pendingDeltaFirstAt.delete(sessionId)
  const thinkingTimer = thinkingFlushTimerBySession.get(sessionId)
  if (thinkingTimer !== undefined) {
    cancelScheduledFlush(thinkingTimer)
    thinkingFlushTimerBySession.delete(sessionId)
  }
  deferredThinkingFlush.delete(sessionId)
  pendingThinkingBySession.delete(sessionId)
  pendingThinkingFirstAt.delete(sessionId)
  lastStreamActivityAtBySession.delete(sessionId)
  continuationPrefixBySession.delete(sessionId)
  clearPendingToolArgs(sessionId)
}

const pendingToolArgsBySession = new Map<
  string,
  { toolName: string; callIndex: number; argsSnapshot: string }
>()
const toolArgsFlushTimerBySession = new Map<string, ScheduledFlushHandle>()

type PendingSubagentChunk = {
  agentId: string
  chunkKind: string
  taskId?: string
  parentFromFrame?: string
  delta: string
}
const pendingSubagentChunksBySession = new Map<string, PendingSubagentChunk[]>()
const subagentChunkFlushTimerBySession = new Map<string, ReturnType<typeof setTimeout>>()
const SUBAGENT_CHUNK_FLUSH_MS = 80

function applySubagentChunkNow(sessionId: string, chunk: PendingSubagentChunk): void {
  const now = Date.now()
  useChatStore.setState((s) => {
    const session = s.sessions[sessionId]
    if (!session) return s
    const parentId = chunk.parentFromFrame ?? session.activeTaskToolUseId
    const bucketExists = parentId
      ? Boolean(session.subagentTimelines[parentId])
      : false

    let nextTimelines = session.subagentTimelines
    if (parentId && bucketExists) {
      const bucket = session.subagentTimelines[parentId]!
      const prevTimeline: AgentTimeline = bucket.agents[chunk.agentId] ?? {
        agentId: chunk.agentId,
        taskId: chunk.taskId,
        status: 'running',
        entries: [],
        startedAt: now,
        updatedAt: now,
      }
      const entry = subagentChunkToEntry(chunk.chunkKind, chunk.delta)
      const nextTimeline = appendTimelineEntry(prevTimeline, entry, now)
      nextTimelines = {
        ...session.subagentTimelines,
        [parentId]: {
          ...bucket,
          agents: {
            ...bucket.agents,
            [chunk.agentId]: {
              ...nextTimeline,
              taskId: chunk.taskId ?? prevTimeline.taskId,
            },
          },
        },
      }
    }
    const sealed = sealThinkingForSession(sessionId, session)
    let nextMessages: UIMessage[]
    if (parentId && bucketExists) {
      nextMessages = sealed
    } else {
      const last = sealed[sealed.length - 1]
      if (
        last &&
        last.type === 'subagent_chunk' &&
        last.agentId === chunk.agentId &&
        last.parentToolUseId === (parentId ?? undefined) &&
        last.chunkKind === chunk.chunkKind
      ) {
        const merged: UIMessage = {
          ...last,
          delta: `${last.delta}${chunk.delta}`,
          timestamp: now,
        }
        nextMessages = [...sealed.slice(0, -1), merged]
      } else {
        nextMessages = [
          ...sealed,
          {
            id: nextId(),
            type: 'subagent_chunk' as const,
            agentId: chunk.agentId,
            delta: chunk.delta,
            chunkKind: chunk.chunkKind,
            taskId: chunk.taskId,
            parentToolUseId: parentId ?? undefined,
            timestamp: now,
          },
        ]
      }
    }
    return {
      sessions: updateSessionIn(s.sessions, sessionId, () => ({
        messages: nextMessages,
        subagentTimelines: nextTimelines,
      })),
    }
  })
}

function flushPendingSubagentChunks(sessionId: string): void {
  const timer = subagentChunkFlushTimerBySession.get(sessionId)
  if (timer !== undefined) {
    clearTimeout(timer)
    subagentChunkFlushTimerBySession.delete(sessionId)
  }
  const chunks = pendingSubagentChunksBySession.get(sessionId)
  pendingSubagentChunksBySession.delete(sessionId)
  if (!chunks || chunks.length === 0) return
  for (const chunk of chunks) {
    applySubagentChunkNow(sessionId, chunk)
  }
}

function enqueueSubagentChunk(sessionId: string, chunk: PendingSubagentChunk): void {
  const list = pendingSubagentChunksBySession.get(sessionId) ?? []
  const last = list[list.length - 1]
  if (
    last &&
    last.agentId === chunk.agentId &&
    last.chunkKind === chunk.chunkKind &&
    last.parentFromFrame === chunk.parentFromFrame &&
    last.taskId === chunk.taskId
  ) {
    last.delta += chunk.delta
  } else {
    list.push(chunk)
  }
  pendingSubagentChunksBySession.set(sessionId, list)
  if (!subagentChunkFlushTimerBySession.has(sessionId)) {
    subagentChunkFlushTimerBySession.set(
      sessionId,
      setTimeout(() => {
        subagentChunkFlushTimerBySession.delete(sessionId)
        flushPendingSubagentChunks(sessionId)
      }, SUBAGENT_CHUNK_FLUSH_MS),
    )
  }
}

function clearPendingToolArgs(sessionId: string): void {
  const timer = toolArgsFlushTimerBySession.get(sessionId)
  if (timer !== undefined) {
    cancelScheduledFlush(timer)
    toolArgsFlushTimerBySession.delete(sessionId)
  }
  pendingToolArgsBySession.delete(sessionId)
  deferredDeltaFlush.delete(`${sessionId}::toolArgs`)
}

function purgeSessionEphemera(sessionId: string): void {
  pendingTaskToolUseIdsBySession.delete(sessionId)
  planModeBlockedToolUseIdsBySession.delete(sessionId)
  updatePlanInlineToolUseIdsBySession.delete(sessionId)
  lastDirectSendBySession.delete(sessionId)
  runtimeSyncFailureToastSessions.delete(sessionId)
  runtimeSyncRetrySessions.delete(sessionId)
  elapsedLastTickAtBySession.delete(sessionId)
  pendingDesignCanvasReveal.delete(sessionId)
  historyChangedReloadAt.delete(sessionId)
  historyGenerationBySession.delete(sessionId)
  capReloadInFlight.delete(sessionId)
  dirtyMidTurnSessions.delete(sessionId)
  activeTurnSeqBySession.delete(sessionId)
  lastBusyFlushAtBySession.delete(sessionId)
  clearHistoryLoadRetry(sessionId)
  pendingSubagentChunksBySession.delete(sessionId)
  const subagentTimer = subagentChunkFlushTimerBySession.get(sessionId)
  if (subagentTimer !== undefined) {
    clearTimeout(subagentTimer)
    subagentChunkFlushTimerBySession.delete(sessionId)
  }
  pendingWorkerEventsBySession.delete(sessionId)
  const workerEventTimer = workerEventFlushTimerBySession.get(sessionId)
  if (workerEventTimer !== undefined) {
    clearTimeout(workerEventTimer)
    workerEventFlushTimerBySession.delete(sessionId)
  }
  void import('./reviewPanelStore').then((m) => {
    m.useReviewPanelStore.getState().purgeSession(sessionId)
  })
}

function hasPendingThinking(sessionId: string): boolean {
  const buf = pendingThinkingBySession.get(sessionId)
  return buf !== undefined && buf.length > 0
}

function hasPendingDelta(sessionId: string): boolean {
  const buf = pendingDeltaBySession.get(sessionId)
  return buf !== undefined && buf.length > 0
}

function sealThinking(
  messages: UIMessage[],
  activeThinkingId: string | null,
  options?: { content?: string; startedAt?: number | null },
): UIMessage[] {
  const content = options?.content ?? ''
  const startedAt = options?.startedAt ?? null
  if (content.trim()) {
    if (activeThinkingId) {
      return commitActiveThinking(messages, activeThinkingId, content, startedAt, true)
    }
    for (let i = messages.length - 1; i >= 0; i--) {
      const m = messages[i]
      if (!m || m.type !== 'thinking') continue
      if (m.completedAt) break
      const next = [...messages]
      next[i] = {
        ...m,
        content: m.content === content ? m.content : content,
        completedAt: Date.now(),
      }
      return next
    }
    return commitActiveThinking(messages, nextId(), content, startedAt, true)
  }
  if (!activeThinkingId) return messages
  const idx = messages.findIndex(
    (m) => m.id === activeThinkingId && m.type === 'thinking',
  )
  if (idx < 0) return messages
  const target = messages[idx] as Extract<UIMessage, { type: 'thinking' }>
  if (target.completedAt) return messages
  const sealed: UIMessage = { ...target, completedAt: Date.now() }
  const next = [...messages]
  next[idx] = sealed
  return next
}

function sealThinkingFromState(state: {
  messages: UIMessage[]
  activeThinkingId: string | null
  activeThinkingContent: string
  activeThinkingStartedAt: number | null
}): UIMessage[] {
  return sealThinking(state.messages, state.activeThinkingId, {
    content: state.activeThinkingContent,
    startedAt: state.activeThinkingStartedAt,
  })
}

function commitActiveThinking(
  messages: UIMessage[],
  activeThinkingId: string | null,
  content: string,
  startedAt: number | null,
  seal: boolean,
): UIMessage[] {
  if (!activeThinkingId || !content.trim()) {
    return seal ? sealThinking(messages, activeThinkingId) : messages
  }
  const idx = messages.findIndex(
    (m) => m.id === activeThinkingId && m.type === 'thinking',
  )
  const now = Date.now()
  if (idx < 0) {
    const next = [...messages]
    next.push({
      id: activeThinkingId,
      type: 'thinking',
      content,
      timestamp: startedAt ?? now,
      startedAt: startedAt ?? now,
      ...(seal ? { completedAt: now } : {}),
    })
    return next
  }
  const target = messages[idx] as Extract<UIMessage, { type: 'thinking' }>
  if (target.completedAt && !seal) return messages
  const merged: UIMessage = {
    ...target,
    content: target.content === content ? target.content : content,
    ...(seal ? { completedAt: target.completedAt ?? now } : {}),
  }
  const next = [...messages]
  next[idx] = merged
  return next
}

function collapseRepeatedAssistantTextOnce(body: string): string {
  if (body.length < 16) return body
  const maxN = Math.min(40, Math.floor(body.length / 8))
  for (let n = maxN; n >= 2; n--) {
    if (body.length % n !== 0) continue
    const unitLen = body.length / n
    if (unitLen < 8) continue
    let tiled = true
    for (let i = 1; i < n && tiled; i++) {
      const base = i * unitLen
      for (let j = 0; j < unitLen; j++) {
        if (body.charCodeAt(j) !== body.charCodeAt(base + j)) {
          tiled = false
          break
        }
      }
    }
    if (tiled) return body.slice(0, unitLen)
  }
  const firstNl = body.indexOf('\n')
  if (firstNl >= 8) {
    const unit = body.slice(0, firstNl)
    const parts = body.split('\n')
    if (parts.length >= 2 && parts.every((p) => p === unit)) return unit
  }
  return body
}

function collapseRepeatedAssistantText(text: string): string {
  if (text.length < 16) return text
  const trailing = text.match(/\n+$/)?.[0] ?? ''
  let body = trailing ? text.slice(0, -trailing.length) : text
  for (;;) {
    const next = collapseRepeatedAssistantTextOnce(body)
    if (next === body) break
    body = next
  }
  return body + trailing
}

function assistantTextAlreadyIncludes(prev: string, next: string): boolean {
  if (!next) return true
  if (prev === next) return true
  if (prev.endsWith(next)) return true
  const a = prev.trimEnd()
  const b = next.trimEnd()
  if (!b) return true
  if (a === b) return true
  if (a.endsWith(b)) return true
  return false
}

function mergeAssistantTextContent(
  prev: string,
  next: string,
  dedupEcho: boolean,
): string | null {
  if (!dedupEcho) {
    return next ? prev + next : null
  }
  const collapsedPrev = collapseRepeatedAssistantText(prev)
  const collapsedNext = collapseRepeatedAssistantText(next)
  if (assistantTextAlreadyIncludes(collapsedPrev, collapsedNext)) {
    return collapsedPrev === prev ? null : collapsedPrev
  }
  if (collapsedPrev.length > 0 && collapsedNext.startsWith(collapsedPrev)) {
    return collapsedNext
  }
  const a = collapsedPrev.trimEnd()
  const b = collapsedNext.trimEnd()
  if (a.length > 0 && b.startsWith(a)) return collapsedNext
  return collapseRepeatedAssistantText(collapsedPrev + collapsedNext)
}

const MARKDOWN_SYNC_PARSE_MAX_CHARS = 3072

function warmAssistantMarkdownCache(content: string): void {
  if (content.length <= MARKDOWN_SYNC_PARSE_MAX_CHARS) return
  void Promise.all([
    import('../utils/sanitizeNarration'),
    import('../lib/markdownWorkerClient'),
  ])
    .then(([{ sanitizeNarration }, { parseMarkdownAsync }]) =>
      parseMarkdownAsync(sanitizeNarration(content), { cacheWrite: true }),
    )
    .catch(() => {})
}

function appendAssistantTextMessage(
  messages: UIMessage[],
  content: string,
  timestamp: number,
  model?: string,
  options?: { dedupEcho?: boolean },
): UIMessage[] {
  if (!content.trim()) return messages
  const dedupEcho = options?.dedupEcho === true

  const last = messages[messages.length - 1]
  if (last?.type === 'assistant_text') {
    const mergedContent = mergeAssistantTextContent(last.content, content, dedupEcho)
    if (mergedContent === null) {
      if (!(model ?? last.model)) return messages
      const touched: UIMessage = {
        ...last,
        ...(model ?? last.model ? { model: model ?? last.model } : {}),
      }
      return [...messages.slice(0, -1), touched]
    }
    warmAssistantMarkdownCache(mergedContent)
    const merged: UIMessage = {
      ...last,
      content: mergedContent,
      ...(model ?? last.model ? { model: model ?? last.model } : {}),
    }
    return [...messages.slice(0, -1), merged]
  }

  const storedContent = dedupEcho ? collapseRepeatedAssistantText(content) : content
  warmAssistantMarkdownCache(storedContent)
  return [
    ...messages,
    {
      id: nextId(),
      type: 'assistant_text',
      content: storedContent,
      timestamp,
      ...(model ? { model } : {}),
    },
  ]
}

function echoDedupOptions(sessionId: string): { dedupEcho: boolean } {
  return { dedupEcho: dirtyMidTurnSessions.has(sessionId) }
}

function isSessionUiBusy(chatState: ChatState): boolean {
  return (
    chatState === 'streaming' ||
    chatState === 'thinking' ||
    chatState === 'tool_executing' ||
    chatState === 'permission_pending' ||
    chatState === 'awaiting_workers'
  )
}

function resolvePlanningPhaseAction(verb: string | undefined, previous: string): string {
  const raw = (verb ?? '').trim()
  if (!raw || raw.toLowerCase().startsWith('iter ')) {
    return previous.trim() || 'waiting_model'
  }
  return raw
}

function resumeParentAfterWorkers(session: PerSessionState): Partial<PerSessionState> {
  if (session.chatState !== 'awaiting_workers') return {}
  return { chatState: 'thinking', planningPhaseStartedAt: Date.now() }
}

function absorbPendingStreamIntoSession(
  sessionId: string,
  session: {
    messages: UIMessage[]
    streamingText: string
    activeThinkingId: string | null
    activeThinkingContent: string
    activeThinkingStartedAt: number | null
    activeThinkingLastChunkAt: number | null
  },
): {
  messages: UIMessage[]
  streamingText: string
  activeThinkingId: string | null
  activeThinkingContent: string
  activeThinkingStartedAt: number | null
  activeThinkingLastChunkAt: number | null
} {
  const streamingText = flushPendingDeltaIntoStreaming(sessionId, session.streamingText)
  const patch = mergePendingThinkingIntoActive(session, sessionId)
  return {
    messages: commitActiveThinking(
      session.messages,
      patch.activeThinkingId,
      patch.activeThinkingContent,
      patch.activeThinkingStartedAt,
      false,
    ),
    streamingText,
    activeThinkingId: patch.activeThinkingId,
    activeThinkingContent: patch.activeThinkingContent,
    activeThinkingStartedAt: patch.activeThinkingStartedAt,
    activeThinkingLastChunkAt: patch.activeThinkingLastChunkAt,
  }
}

function commitIdleEphemeralTranscript(
  sessionId: string,
  session: {
    messages: UIMessage[]
    streamingText: string
    activeThinkingId: string | null
    activeThinkingContent: string
    activeThinkingStartedAt: number | null
    activeThinkingLastChunkAt: number | null
  },
): UIMessage[] {
  const pendingText = `${session.streamingText}${consumePendingDelta(sessionId)}`
  let messages = sealThinkingForSession(sessionId, session)
  if (pendingText.trim()) {
    messages = appendAssistantTextMessage(
      messages,
      pendingText,
      Date.now(),
      undefined,
      echoDedupOptions(sessionId),
    )
  }
  return messages
}

function stableJsonFingerprint(value: unknown): string {
  try {
    return JSON.stringify(value ?? null) ?? ''
  } catch {
    return ''
  }
}

function transcriptFingerprint(m: UIMessage): string | null {
  switch (m.type) {
    case 'thinking':
      return `thinking:${m.content.trim()}`
    case 'assistant_text':
      return `assistant:${m.content.trim()}`
    case 'user_text':
      return `user:${typeof m.userMessageIndex === 'number' ? m.userMessageIndex : ''}:${m.content.trim()}`
    case 'tool_use':
      return m.toolUseId
        ? `tool_use:${m.toolUseId}`
        : `tool_use:${m.toolName}:${stableJsonFingerprint(m.input)}`
    case 'tool_result':
      return m.toolUseId ? `tool_result:${m.toolUseId}` : null
    case 'file_edit':
      return m.editBatchId
        ? `file_edit:${m.editBatchId}`
        : `file_edit:${m.path}:${m.additions}:${m.deletions}`
    case 'command_preview':
      return `command_preview:${m.toolName}:${stableJsonFingerprint(m.input)}`
    case 'error':
      return `error:${m.code}:${m.message}`
    case 'plan_card':
      return `plan_card:${m.sourceToolUseId || m.planPath || m.title || m.id}`
    case 'curator_card':
      return `curator_card:${m.sourceToolUseId || m.implBlueprintPath || m.title || m.id}`
    case 'mode_switch_card':
      return `mode_switch:${m.planPath}:${m.status}:${m.handoffKind ?? ''}`
    case 'plan_progress':
      return `plan_progress:${m.planPath}`
    case 'permission_request':
      return `permission:${m.requestId}`
    case 'plan_mode_blocked':
      return `plan_mode_blocked:${m.reason ?? ''}:${m.mode ?? ''}`
    case 'plan_question_answers':
      return `plan_qa:${m.items.map((item) => item.question).join('|')}`
    case 'task_summary':
      return `task_summary:${m.tasks.map((task) => task.id).join('|')}`
    case 'system':
      return `system:${m.content}`
    case 'subagent_chunk':
      return `subagent:${m.parentToolUseId ?? ''}:${m.agentId}:${m.chunkKind}:${m.delta}`
  }
}

function transcriptTextsOverlap(a: string, b: string): boolean {
  const x = a.trim()
  const y = b.trim()
  if (!x || !y) return false
  if (x === y) return true
  if (x.startsWith(y) || y.startsWith(x)) return true
  return assistantTextAlreadyIncludes(x, y) || assistantTextAlreadyIncludes(y, x)
}

function pickThinkingTimes(
  hydrated: Extract<UIMessage, { type: 'thinking' }>,
  live: Extract<UIMessage, { type: 'thinking' }>,
): { startedAt?: number; completedAt?: number } {
  const liveDur = (live.completedAt ?? 0) - (live.startedAt ?? 0)
  const hydDur = (hydrated.completedAt ?? 0) - (hydrated.startedAt ?? 0)
  if (liveDur > hydDur) {
    return {
      startedAt: live.startedAt ?? hydrated.startedAt,
      completedAt: live.completedAt ?? hydrated.completedAt,
    }
  }
  return {
    startedAt: hydrated.startedAt ?? live.startedAt,
    completedAt: hydrated.completedAt ?? live.completedAt,
  }
}

function mergeMatchedLiveMessage(hydrated: UIMessage, live: UIMessage): UIMessage {
  const stableId = live.id
  const rawId = hydrated.rawId ?? hydrated.id
  if (hydrated.type === 'thinking' && live.type === 'thinking') {
    const content =
      live.content.trim().length > hydrated.content.trim().length ? live.content : hydrated.content
    return {
      ...hydrated,
      id: stableId,
      rawId,
      content,
      ...pickThinkingTimes(hydrated, live),
    }
  }
  if (hydrated.type === 'assistant_text' && live.type === 'assistant_text') {
    const model = hydrated.model ?? live.model
    if (live.content.trim().length > hydrated.content.trim().length) {
      return {
        ...hydrated,
        id: stableId,
        rawId,
        content: live.content,
        ...(model ? { model } : {}),
      }
    }
    return { ...hydrated, id: stableId, rawId, ...(model ? { model } : {}) }
  }
  if (hydrated.type === 'file_edit' && live.type === 'file_edit') {
    return {
      ...hydrated,
      id: stableId,
      rawId,
      additions: Math.max(hydrated.additions, live.additions),
      deletions: Math.max(hydrated.deletions, live.deletions),
      diff: live.diff || hydrated.diff,
      ...(live.reverted === true ? { reverted: true } : {}),
    }
  }
  if (hydrated.type === 'plan_card' && live.type === 'plan_card') {
    return live.status === 'completed' || live.todos.length >= hydrated.todos.length
      ? { ...live, rawId }
      : { ...hydrated, id: stableId, rawId }
  }
  if (hydrated.type === 'curator_card' && live.type === 'curator_card') {
    return live.status === 'completed' || live.body.length >= hydrated.body.length
      ? { ...live, rawId }
      : { ...hydrated, id: stableId, rawId }
  }
  if (hydrated.type === 'user_text' && live.type === 'user_text') {
    return {
      ...hydrated,
      id: stableId,
      rawId,
      ...(live.attachments && live.attachments.length > 0
        ? { attachments: live.attachments }
        : {}),
      ...(live.clientMsgId && !hydrated.clientMsgId
        ? { clientMsgId: live.clientMsgId }
        : {}),
    }
  }
  if (hydrated.type === 'tool_result') {
    return {
      ...hydrated,
      id: stableId,
      rawId,
      content: capToolResultContent(hydrated.content),
    }
  }
  return { ...hydrated, id: stableId, rawId }
}

function isPendingUserText(m: UIMessage): boolean {
  return m.type === 'user_text' && m.pending === true
}

function mergeHydratedHistoryWithLiveUi(hydrated: UIMessage[], live: UIMessage[]): UIMessage[] {
  if (live.length === 0) return hydrated
  if (hydrated.length === 0) {
    return live.filter((m) => !isPendingUserText(m) || m.type === 'user_text')
  }
  const result = hydrated.slice()
  const used = new Set<UIMessage>()
  const findHydrated = (pred: (m: UIMessage) => boolean): UIMessage | undefined =>
    result.find((m) => !used.has(m) && pred(m))
  const fpIndex = new Map<string, UIMessage[]>()
  for (const h of hydrated) {
    const hfp = transcriptFingerprint(h)
    if (!hfp) continue
    const bucket = fpIndex.get(hfp)
    if (bucket) bucket.push(h)
    else fpIndex.set(hfp, [h])
  }
  const takeByFingerprint = (fp: string): UIMessage | undefined => {
    const bucket = fpIndex.get(fp)
    if (!bucket) return undefined
    while (bucket.length > 0) {
      const candidate = bucket[0]!
      if (used.has(candidate)) {
        bucket.shift()
        continue
      }
      return candidate
    }
    return undefined
  }
  let lastPlaceIdx = -1

  for (const liveMsg of live) {
    const fp = transcriptFingerprint(liveMsg)
    let match: UIMessage | undefined
    if (fp) {
      match = takeByFingerprint(fp)
    }
    if (!match && liveMsg.type === 'assistant_text') {
      match = findHydrated(
        (h) => h.type === 'assistant_text' && transcriptTextsOverlap(h.content, liveMsg.content),
      )
    }
    if (!match && liveMsg.type === 'thinking') {
      match = findHydrated(
        (h) => h.type === 'thinking' && transcriptTextsOverlap(h.content, liveMsg.content),
      )
    }
    if (!match && liveMsg.type === 'user_text') {
      if (liveMsg.clientMsgId) {
        match = findHydrated(
          (h) => h.type === 'user_text' && h.clientMsgId === liveMsg.clientMsgId,
        )
      }
      if (!match && typeof liveMsg.userMessageIndex === 'number') {
        match = findHydrated(
          (h) =>
            h.type === 'user_text' &&
            h.userMessageIndex === liveMsg.userMessageIndex,
        )
      }
      if (!match) {
        const fromIdx = lastPlaceIdx + 1
        match = result.find(
          (m, idx) =>
            idx >= fromIdx &&
            !used.has(m) &&
            m.type === 'user_text' &&
            m.content.trim() === liveMsg.content.trim(),
        )
      }
    }
    if (match) {
      used.add(match)
      const idx = result.indexOf(match)
      if (idx >= 0) {
        result[idx] = mergeMatchedLiveMessage(match, liveMsg)
        lastPlaceIdx = idx
      }
      continue
    }
    let insertAt: number
    if (lastPlaceIdx >= 0) {
      insertAt = Math.min(lastPlaceIdx + 1, result.length)
    } else if (liveMsg.type === 'user_text') {
      insertAt = result.length
    } else {
      let hydUserIdx = -1
      for (let i = result.length - 1; i >= 0; i--) {
        if (result[i]?.type === 'user_text') {
          hydUserIdx = i
          break
        }
      }
      insertAt = hydUserIdx >= 0 ? hydUserIdx + 1 : result.length
    }
    result.splice(insertAt, 0, liveMsg)
    used.add(liveMsg)
    lastPlaceIdx = insertAt
  }
  return result
}

function dropHydratedBlockedToolUses(sessionId: string, messages: UIMessage[]): UIMessage[] {
  const blocked = planModeBlockedToolUseIdsBySession.get(sessionId)
  const inlined = updatePlanInlineToolUseIdsBySession.get(sessionId)
  if ((!blocked || blocked.size === 0) && (!inlined || inlined.size === 0)) return messages
  return messages.filter((m) => {
    if (m.type !== 'tool_use' && m.type !== 'tool_result') return true
    if (blocked && blocked.has(m.toolUseId)) return false
    if (inlined && inlined.has(m.toolUseId)) return false
    return true
  })
}

function mergeSubagentTimelineRecords(
  live: Record<string, SubagentTimelineBucket>,
  restored: Record<string, SubagentTimelineBucket>,
): Record<string, SubagentTimelineBucket> {
  const out: Record<string, SubagentTimelineBucket> = { ...restored }
  for (const [parentId, liveBucket] of Object.entries(live)) {
    const rest = out[parentId]
    if (!rest) {
      out[parentId] = liveBucket
      continue
    }
    const agents = { ...rest.agents }
    for (const [agentId, liveTl] of Object.entries(liveBucket.agents)) {
      const r = agents[agentId]
      if (!r) {
        agents[agentId] = liveTl
        continue
      }
      const liveRicher =
        (liveTl.entries?.length ?? 0) > (r.entries?.length ?? 0) || liveTl.updatedAt > r.updatedAt
      agents[agentId] = liveRicher
        ? {
            ...r,
            ...liveTl,
            finalOutput: liveTl.finalOutput || r.finalOutput,
          }
        : {
            ...liveTl,
            ...r,
            finalOutput: r.finalOutput || liveTl.finalOutput,
          }
    }
    out[parentId] = {
      ...rest,
      parentToolName: rest.parentToolName || liveBucket.parentToolName,
      agents,
    }
  }
  return out
}

function mergeAgentNotifications(
  live: Record<string, AgentTaskNotification>,
  restored: Record<string, AgentTaskNotification>,
): Record<string, AgentTaskNotification> {
  const out: Record<string, AgentTaskNotification> = { ...restored }
  for (const [id, n] of Object.entries(live)) {
    const r = out[id]
    if (!r) {
      out[id] = n
      continue
    }
    const summary =
      (r.summary?.length ?? 0) >= (n.summary?.length ?? 0) ? r.summary : n.summary
    out[id] = {
      ...r,
      ...n,
      summary: summary || r.summary || n.summary,
      status: r.status === 'completed' || n.status === 'completed' ? 'completed' : n.status,
    }
  }
  return out
}

function remapActiveThinkingId(
  messages: UIMessage[],
  prevId: string | null,
  prevContent: string,
): string | null {
  if (!prevId) return null
  if (messages.some((m) => m.id === prevId && m.type === 'thinking')) return prevId
  const needle = prevContent.trim()
  if (!needle) return null
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]
    if (m && m.type === 'thinking' && transcriptTextsOverlap(m.content, needle)) {
      return m.id
    }
  }
  return prevId
}

function updateSessionIn(
  sessions: Record<string, PerSessionState>,
  sessionId: string,
  updater: (s: PerSessionState) => Partial<PerSessionState>,
): Record<string, PerSessionState> {
  const session = sessions[sessionId]
  if (!session) return sessions
  return { ...sessions, [sessionId]: { ...session, ...updater(session) } }
}

const PII_KIND_ALIAS_FRONTEND: Record<string, string> = {
  authorization_header: 'auth_header',
  mac_address: 'mac',
}

function applyDebugPiiStatsDelta(
  update: (updater: (s: PerSessionState) => Partial<PerSessionState>) => void,
  payload: Record<string, unknown>,
) {
  const totalDelta =
    typeof payload.total === 'number' && Number.isFinite(payload.total)
      ? Math.max(0, Math.floor(payload.total as number))
      : 0
  const rawCounts =
    payload.counts && typeof payload.counts === 'object'
      ? (payload.counts as Record<string, unknown>)
      : {}
  const countsDelta: Record<string, number> = {}
  for (const [rawKey, value] of Object.entries(rawCounts)) {
    if (typeof rawKey !== 'string' || rawKey.length === 0) continue
    const num = typeof value === 'number' ? value : Number(value)
    if (!Number.isFinite(num) || num <= 0) continue
    const key = PII_KIND_ALIAS_FRONTEND[rawKey] ?? rawKey
    countsDelta[key] = (countsDelta[key] ?? 0) + Math.floor(num)
  }
  if (totalDelta <= 0 && Object.keys(countsDelta).length === 0) return
  update((session) => {
    const prev = session.debugPiiStats ?? {
      total: 0,
      counts: {},
      lastEventAt: null,
    }
    const mergedCounts: Record<string, number> = { ...prev.counts }
    for (const [key, value] of Object.entries(countsDelta)) {
      mergedCounts[key] = (mergedCounts[key] ?? 0) + value
    }
    return {
      debugPiiStats: {
        total: prev.total + totalDelta,
        counts: mergedCounts,
        lastEventAt: Date.now(),
      },
    }
  })
}

async function hydrateCumulativeTokensFromUsage(sessionId: string): Promise<void> {
  try {
    await useUsageStore.getState().fetch()
    const persisted = useUsageStore.getState().summary?.bySession?.[sessionId]?.totalTokens ?? 0
    if (!Number.isFinite(persisted) || persisted <= 0) return
    useChatStore.setState((state) => {
      const session = state.sessions[sessionId]
      if (!session) return state
      const nextValue = Math.max(session.cumulativeTokens ?? 0, Math.floor(persisted))
      if (nextValue === session.cumulativeTokens) return state
      return {
        sessions: {
          ...state.sessions,
          [sessionId]: { ...session, cumulativeTokens: nextValue },
        },
      }
    })
  } catch {

  }
}

const HISTORY_PAGE_SIZE = 200

export const MAX_IN_MEMORY_MESSAGES = 2500
const MESSAGE_TRIM_CHUNK = 500

const MAX_TOOL_RESULT_CHARS = 200_000

function capToolResultContent(content: unknown): unknown {
  if (typeof content === 'string' && content.length > MAX_TOOL_RESULT_CHARS) {
    const dropped = content.length - MAX_TOOL_RESULT_CHARS
    return `${content.slice(0, MAX_TOOL_RESULT_CHARS)}\n… [truncated ${dropped} chars]`
  }
  if (Array.isArray(content)) {
    let mutated = false
    const capped = content.map((item) => {
      if (
        item &&
        typeof item === 'object' &&
        typeof (item as { text?: unknown }).text === 'string' &&
        ((item as { text: string }).text.length > MAX_TOOL_RESULT_CHARS)
      ) {
        mutated = true
        const text = (item as { text: string }).text
        const dropped = text.length - MAX_TOOL_RESULT_CHARS
        return {
          ...(item as Record<string, unknown>),
          text: `${text.slice(0, MAX_TOOL_RESULT_CHARS)}\n… [truncated ${dropped} chars]`,
        }
      }
      return item
    })
    return mutated ? capped : content
  }
  if (
    content &&
    typeof content === 'object' &&
    typeof (content as { text?: unknown }).text === 'string' &&
    ((content as { text: string }).text.length > MAX_TOOL_RESULT_CHARS)
  ) {
    const text = (content as { text: string }).text
    const dropped = text.length - MAX_TOOL_RESULT_CHARS
    return {
      ...(content as Record<string, unknown>),
      text: `${text.slice(0, MAX_TOOL_RESULT_CHARS)}\n… [truncated ${dropped} chars]`,
    }
  }
  return content
}

function stripAttachmentMarkersForDisplay(content: string, allowEmpty = false): string {
  if (!content.includes('[IMAGE:') && !content.includes('[Attached file:')) return content
  const kept = content.split('\n').filter((line) => {
    const t = line.trim()
    if (t.startsWith('[IMAGE:') && t.endsWith(']')) return false
    if (t.startsWith('[Attached file:') && t.endsWith(']')) return false
    return true
  })
  const result = kept.join('\n').replace(/\n{3,}/g, '\n\n').trim()
  if (result.length > 0) return result
  return allowEmpty ? '' : content
}

function mapPersistedAttachments(entry: MessageEntry): UIAttachment[] | undefined {
  if (!Array.isArray(entry.attachments) || entry.attachments.length === 0) return undefined
  const mapped: UIAttachment[] = []
  for (const att of entry.attachments) {
    if (!att) continue
    const type = att.type === 'image' ? 'image' : 'file'
    const rawName = att.name || att.path || type
    mapped.push({
      type,
      name: rawName.replace(/^[0-9a-f]{8}-(?=.)/i, ''),
      ...(att.path ? { path: att.path } : {}),
      data: att.data,
      mimeType: att.mimeType,
    })
  }
  return mapped.length > 0 ? mapped : undefined
}

function extractDisplayBriefFromTaskEnvelope(content: string): string | null {
  const trimmed = content.trimStart()
  if (!trimmed.startsWith('[Design task')) return null
  if (!trimmed.includes('EXCLUSIVE TASK FOR THIS TURN]')) return null
  const briefIdx = trimmed.indexOf('\nBrief:')
  if (briefIdx < 0) return null
  const after = trimmed.slice(briefIdx + '\nBrief:'.length)
  const terminators = [
    '\n\nSelected parameters',
    '\n\nSubject fidelity',
    '\n\nWrite every produced asset',
    '\n\nOutput',
  ]
  let end = after.length
  for (const term of terminators) {
    const idx = after.indexOf(term)
    if (idx >= 0 && idx < end) end = idx
  }
  const brief = after.slice(0, end).trim()
  return brief.length > 0 ? brief : null
}

const yieldToMainThread = () => new Promise<void>((resolve) => setTimeout(resolve, 0))

async function fetchAndMapSessionHistory(sessionId: string) {
  const { messages, pendingRewind, firstIndex, hasMore } = await sessionsApi.getMessages(
    sessionId,
    { limit: HISTORY_PAGE_SIZE },
  )
  const large = messages.length > 60
  const uiMessages = await mapHistoryMessagesToUiMessages(messages)
  for (const m of uiMessages) {
    if (m.type === 'user_text' && m.clientMsgId) {
      wsManager.confirmUserMessage(sessionId, m.clientMsgId)
    }
  }
  if (large) await yieldToMainThread()
  const restoredNotifications = reconstructAgentNotifications(messages)
  const lastTodos = extractLastTodoWriteFromHistory(messages)
  if (large) await yieldToMainThread()
  const hasMessagesAfterTaskCompletion = hasUserMessagesAfterTaskCompletion(messages)
  const restoredSubagentTimelines = reconstructSubagentTimelines(messages)
  return {
    rawMessages: messages,
    uiMessages,
    restoredNotifications,
    lastTodos,
    hasMessagesAfterTaskCompletion,
    restoredSubagentTimelines,
    pendingRewind: pendingRewind ?? null,
    historyFirstIndex: firstIndex ?? 0,
    historyHasMore: hasMore === true,
  }
}

const ASK_ANSWER_TEXT_MARKER = 'Here are my answers to your clarifying questions:'

function tryParseAskResponseUserText(
  text: string,
): { items: Array<{ question: string; answer: string | string[] }>; details?: string } | null {
  const idx = text.indexOf(ASK_ANSWER_TEXT_MARKER)
  if (idx < 0) return null
  const body = text.slice(idx + ASK_ANSWER_TEXT_MARKER.length)
  const items: Array<{ question: string; answer: string | string[] }> = []
  const lines = body.split('\n')
  let i = 0
  let detailsBuf: string[] | null = null
  while (i < lines.length) {
    const line = lines[i] ?? ''
    if (line.startsWith('Additional details from the user:') || line.startsWith('Additional details:')) {
      detailsBuf = []
      i++
      continue
    }
    if (detailsBuf) {
      detailsBuf.push(line)
      i++
      continue
    }
    const m = /^\s*(\d+)\.\s+(.+?)\s*$/.exec(line)
    if (m && m[2]) {
      const question = m[2]
      const answers: string[] = []
      i++
      while (i < lines.length) {
        const next = lines[i] ?? ''
        const am = /^\s+->\s+(.+?)\s*$/.exec(next)
        if (!am) break
        if (am[1] && am[1] !== '(no answer)' && am[1] !== '(skipped)') answers.push(am[1])
        i++
      }
      items.push({
        question,
        answer: answers.length === 0 ? '' : answers.length === 1 ? answers[0]! : answers,
      })
      continue
    }
    i++
  }
  const details = detailsBuf ? detailsBuf.join('\n').trim() : undefined
  if (items.length === 0 && !details) return null
  return details ? { items, details } : { items }
}

function reconstructSubagentTimelines(
  messages: MessageEntry[],
): Record<string, SubagentTimelineBucket> {
  const buckets: Record<string, SubagentTimelineBucket> = {}
  const ensureAgentTimeline = (
    bucket: SubagentTimelineBucket,
    agentId: string,
    when: number,
    taskId?: string,
  ): AgentTimeline => {
    const prev = bucket.agents[agentId]
    if (prev) {
      if (taskId && !prev.taskId) prev.taskId = taskId
      return prev
    }
    const fresh: AgentTimeline = {
      agentId,
      taskId,
      status: 'running',
      entries: [],
      startedAt: when,
      updatedAt: when,
    }
    bucket.agents[agentId] = fresh
    return fresh
  }

  for (const msg of messages) {
    const baseTs = new Date(msg.timestamp).getTime()
    const historyBlocks = assistantBlocksFromMessage(msg)
    if (historyBlocks.length > 0) {
      for (const rawBlock of historyBlocks) {
        if (rawBlock.type === 'tool_use' && typeof rawBlock.name === 'string' && rawBlock.id) {
          if (isSubagentParentTool(rawBlock.name)) {
            if (!buckets[rawBlock.id]) {
              buckets[rawBlock.id] = {
                parentToolUseId: rawBlock.id,
                parentToolName: rawBlock.name,
                agents: {},
              }
            }
          }
        }
        if (rawBlock.type === 'subagent_chunk' && typeof rawBlock.agent_id === 'string') {
          const parentId = typeof rawBlock.parent_tool_use_id === 'string' ? rawBlock.parent_tool_use_id : ''
          if (!parentId) continue
          const bucket =
            buckets[parentId] ?? {
              parentToolUseId: parentId,
              parentToolName: 'Task',
              agents: {},
            }
          buckets[parentId] = bucket
          const when = typeof rawBlock.timestamp_ms === 'number' && Number.isFinite(rawBlock.timestamp_ms)
            ? rawBlock.timestamp_ms
            : baseTs
          const timeline = ensureAgentTimeline(
            bucket,
            rawBlock.agent_id,
            when,
            typeof rawBlock.task_id === 'string' ? rawBlock.task_id : undefined,
          )
          const entry = subagentChunkToEntry(
            typeof rawBlock.kind === 'string' ? rawBlock.kind : 'Chunk',
            typeof rawBlock.delta === 'string' ? rawBlock.delta : '',
          )
          const merged = appendTimelineEntry(timeline, entry, when)
          bucket.agents[rawBlock.agent_id] = merged
        }
        if (rawBlock.type === 'worker_event' && typeof rawBlock.worker_id === 'string') {
          const parentId = typeof rawBlock.parent_tool_use_id === 'string' ? rawBlock.parent_tool_use_id : ''
          if (!parentId) continue
          const bucket =
            buckets[parentId] ?? {
              parentToolUseId: parentId,
              parentToolName: 'spawn_workers',
              agents: {},
            }
          buckets[parentId] = bucket
          const when = typeof rawBlock.timestamp_ms === 'number' && Number.isFinite(rawBlock.timestamp_ms)
            ? rawBlock.timestamp_ms
            : baseTs
          const timeline = ensureAgentTimeline(bucket, rawBlock.worker_id, when)
          const payload = rawBlock.payload as Record<string, unknown> | undefined
          const kindStr = typeof rawBlock.kind === 'string' ? rawBlock.kind : 'status'
          const detail = typeof payload?.detail === 'string' ? (payload.detail as string) : ''
          const action = typeof payload?.action === 'string' ? (payload.action as string) : kindStr
          const text = detail ? `${action}: ${detail}` : action
          const entry = subagentChunkToEntry('Status', text)
          const merged = appendTimelineEntry(timeline, entry, when)
          if (kindStr === 'completed') {
            const success = payload?.success !== false
            merged.status = success ? 'completed' : 'error'
            const summary = typeof payload?.summary === 'string' ? (payload.summary as string) : undefined
            if (summary) merged.finalOutput = summary
          } else if (kindStr === 'stopped') {
            merged.status = 'error'
          }
          bucket.agents[rawBlock.worker_id] = merged
        }
      }
    }
    const userBlocks = userHistoryBlocksFromContent(msg.content)
    if ((msg.type === 'user' || msg.type === 'tool_result') && userBlocks) {
      for (const rawBlock of userBlocks) {
        if (rawBlock.type === 'tool_result' && typeof rawBlock.tool_use_id === 'string') {
          const bucket = buckets[rawBlock.tool_use_id]
          if (!bucket) continue
          const finalText =
            typeof rawBlock.content === 'string'
              ? rawBlock.content
              : extractTextFromRawContent(rawBlock.content)
          const isError = !!rawBlock.is_error
          for (const agentId of Object.keys(bucket.agents)) {
            const t = bucket.agents[agentId]!
            t.status = isError ? 'error' : 'completed'
            if (finalText && !t.finalOutput) t.finalOutput = finalText
          }
        }
      }
    }
  }
  return buckets
}

export const useChatStore = create<ChatStore>((set, get) => ({
  sessions: {},
  sessionCodingMode: {},
  sessionAutoResolvedMode: {},
  pendingSessionCodingMode: null,

  getSession: (sessionId) => get().sessions[sessionId] ?? createDefaultSessionState(),

  connectToSession: (sessionId, options) => {
    void useCLITaskStore.getState().fetchSessionTasks(sessionId)

    const existing = get().sessions[sessionId]
    if (existing && existing.connectionState !== 'disconnected' && !options?.force) {
      void useWorkersStore.getState().fetchByParent(sessionId)
      return
    }
    if (options?.force) {
      wsManager.clearHandlers(sessionId)
      wsManager.disconnect(sessionId)
    }
    if (hasPendingDelta(sessionId) || hasPendingThinking(sessionId)) {
      set((s) => {
        const cur = s.sessions[sessionId]
        if (!cur) return s
        const absorbed = absorbPendingStreamIntoSession(sessionId, cur)
        return {
          sessions: updateSessionIn(s.sessions, sessionId, () => absorbed),
        }
      })
    }

    clearSessionStreamBuffers(sessionId)

    set((s) => {
      const prev = s.sessions[sessionId]
      const base = prev ?? createDefaultSessionState()
      return {
        sessions: {
          ...s.sessions,
          [sessionId]: {
            ...base,
            connectionState: 'connecting',
          },
        },
      }
    })

    wsManager.clearHandlers(sessionId)
    wsManager.connect(sessionId)
    useSessionStore.getState().recordBrowseSessionWorkDir(sessionId)
    wsManager.onMessage(sessionId, (msg) => {
      if (msg.type === 'connected') {
        const prevSession = get().sessions[sessionId]
        const isReconnect =
          prevSession?.connectionState === 'reconnecting' ||
          (prevSession?.connectionState === 'connecting' &&
            prevSession?.historyLoaded === true)
        set((s) => ({
          sessions: updateSessionIn(s.sessions, sessionId, () => ({
            connectionState: 'connected',
          })),
        }))
        if (isReconnect) {
          const cur = get().sessions[sessionId]
          const uiActive =
            cur?.chatState === 'thinking' ||
            cur?.chatState === 'tool_executing' ||
            cur?.chatState === 'streaming' ||
            cur?.chatState === 'permission_pending'
          if (!uiActive) {
            void get().reloadHistory(sessionId)
          } else {
            const stillRunning = useSessionRunStateStore
              .getState()
              .running.has(sessionId)
            if (stillRunning) {
              dirtyMidTurnSessions.add(sessionId)
              void get().reloadHistory(sessionId)
              get().reconcileStuckSession(sessionId)
            } else {
              resyncingSessions.add(sessionId)
              void get()
                .reloadHistory(sessionId)
                .finally(() => {
                  resyncingSessions.delete(sessionId)
                  drainQueuedForSession(sessionId)
                })
            }
          }
        }
      }
      enqueueServerFrame(sessionId, msg)
    })

    const runtimeSelection = useSessionRuntimeStore.getState().selections[sessionId]
    void ensureSessionRuntimeSynced(sessionId, { persist: false })
      .then(() => {
        runtimeSyncFailureToastSessions.delete(sessionId)
        runtimeSyncRetrySessions.delete(sessionId)
      })
      .catch((err) => {
        handleRuntimeSyncFailure(sessionId, err)
        if (runtimeSelection) {
          wsManager.send(sessionId, {
            type: 'set_runtime_config',
            persist: false,
            ...runtimeSelection,
          })
        }
      })
    if (!sessionId.startsWith('__') && !useTeamStore.getState().getMemberBySessionId(sessionId)) {
      wsManager.send(sessionId, { type: 'prewarm_session' })
    }

    wsManager.send(sessionId, {
      type: 'set_permission_mode',
      mode: useSettingsStore.getState().permissionMode,
    })

    {
      const pinnedMode = get().sessionCodingMode[sessionId]
      if (pinnedMode) {
        wsManager.send(sessionId, {
          type: 'set_coding_mode',
          mode: pinnedMode,
          scope: 'session',
          confirmed: true,
        })
      }
    }

    get().loadHistory(sessionId)
    sessionsApi.getSlashCommands(sessionId)
      .then(({ commands }) => {
        if (get().sessions[sessionId]) {
          set((s) => ({ sessions: updateSessionIn(s.sessions, sessionId, () => ({ slashCommands: commands })) }))
        }
      })
      .catch(() => {
        if (get().sessions[sessionId]) {
          set((s) => ({ sessions: updateSessionIn(s.sessions, sessionId, () => ({ slashCommands: [] })) }))
        }
      })
  },

  connectToWorker: (workerId) => {
    const existing = get().sessions[workerId]
    if (existing && existing.connectionState !== 'disconnected') return

    if (hasPendingDelta(workerId) || hasPendingThinking(workerId)) {
      set((s) => {
        const cur = s.sessions[workerId]
        if (!cur) return s
        const absorbed = absorbPendingStreamIntoSession(workerId, cur)
        return {
          sessions: updateSessionIn(s.sessions, workerId, () => absorbed),
        }
      })
    }

    clearSessionStreamBuffers(workerId)

    set((s) => ({
      sessions: {
        ...s.sessions,
        [workerId]: {
          ...createDefaultSessionState(),
          connectionState: 'connecting',
          messages: existing?.messages ?? [],
        },
      },
    }))

    wsManager.clearHandlers(workerId)
    wsManager.connect(workerId, { pathPrefix: '/ws/worker', force: true })
    wsManager.onMessage(workerId, (msg) => {
      if (msg.type === 'connected') {
        set((s) => ({
          sessions: updateSessionIn(s.sessions, workerId, () => ({
            connectionState: 'connected',
          })),
        }))
      }
      enqueueServerFrame(workerId, msg)
    })

    get().loadHistory(workerId)
  },

  disconnectSession: (sessionId) => {
    const session = get().sessions[sessionId]
    if (hasPendingDelta(sessionId)) {
      const text = consumePendingDelta(sessionId)
      set((s) => ({ sessions: updateSessionIn(s.sessions, sessionId, (sess) => ({ streamingText: sess.streamingText + text })) }))
    } else {
      consumePendingDelta(sessionId)
    }
    if (session) {
      const sealedMessages = sealThinkingForSession(sessionId, session)
      set((s) => ({
        sessions: updateSessionIn(s.sessions, sessionId, () => ({
          messages: sealedMessages,
          activeThinkingId: null,
          activeThinkingContent: '',
          activeThinkingStartedAt: null,
          activeThinkingLastChunkAt: null,
        })),
      }))
    } else {
      consumePendingThinking(sessionId)
    }
    clearSessionStreamBuffers(sessionId)
    purgeSessionEphemera(sessionId)
    void import('./workspaceQueueStore').then((m) =>
      m.purgeQueueEphemeraForSession(sessionId),
    )
    wsManager.disconnect(sessionId)
    set((s) => {
      const { [sessionId]: _, ...rest } = s.sessions
      return { sessions: rest }
    })
  },

  suspendSession: (sessionId) => {
    const session = get().sessions[sessionId]
    if (!session) return
    if (hasPendingDelta(sessionId)) {
      const text = consumePendingDelta(sessionId)
      set((s) => ({ sessions: updateSessionIn(s.sessions, sessionId, (sess) => ({ streamingText: sess.streamingText + text })) }))
    } else {
      consumePendingDelta(sessionId)
    }
    const current = get().sessions[sessionId]
    if (!current) return
    const sealedMessages = sealThinkingForSession(sessionId, current)
    clearSessionStreamBuffers(sessionId)
    wsManager.disconnect(sessionId)
    const SUSPENDED_MESSAGE_WINDOW = 200
    let trimmed = sealedMessages
    if (sealedMessages.length > SUSPENDED_MESSAGE_WINDOW) {
      const cut = sealedMessages.length - SUSPENDED_MESSAGE_WINDOW
      const unackedHead = sealedMessages
        .slice(0, cut)
        .filter((m) => m.type === 'user_text' && m.pending === true && !!m.clientMsgId)
      trimmed = [...unackedHead, ...sealedMessages.slice(cut)]
    }
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, () => ({
        messages: trimmed,
        ...(trimmed !== sealedMessages ? { historyLoaded: false } : {}),
        connectionState: 'disconnected',
        chatState: 'idle',
        activeThinkingId: null,
        activeThinkingContent: '',
        activeThinkingStartedAt: null,
        activeThinkingLastChunkAt: null,
        statusVerb: '',
        planningPhaseAction: '',
        planningPhaseDetail: '',
      })),
    }))
  },

  sendMessage: (sessionId, content, attachments, options) => {

    {
      const session = get().sessions[sessionId]
      if (session?.pendingRewind) {
        set((s) => ({
          sessions: updateSessionIn(s.sessions, sessionId, () => ({
            pendingSendAfterRewind: { content, attachments, options },
          })),
        }))
        return
      }
    }
    const userFacingContent =
      options?.displayContent?.trim() || content.trim()
    const isMemberSession = !!useTeamStore.getState().getMemberBySessionId(sessionId)

    if (!isMemberSession && !options?.__internalDrain) {
      if (!hasUsableModelForSession(sessionId)) {
        emitNoModelWarning(sessionId)
        set((s) => {
          const session = s.sessions[sessionId]
          if (!session) return s
          const cleaned = stripNoModelErrorMessages(session.messages)
          if (cleaned === session.messages) return s
          return {
            sessions: {
              ...s.sessions,
              [sessionId]: { ...session, messages: cleaned },
            },
          }
        })
        return
      }
    }

    const uiAttachments: UIAttachment[] | undefined =
      attachments && attachments.length > 0
        ? attachments.map((a) => ({
            type: a.type,
            name: a.name || a.path || a.mimeType || a.type,
            ...(a.path ? { path: a.path } : {}),
            data: a.data,
            mimeType: a.mimeType,
          }))
        : undefined

    if (!isMemberSession && !options?.__internalDrain) {
      const queueState = useWorkspaceQueueStore.getState()
      const sameSessionRunning = useSessionRunStateStore
        .getState()
        .running.has(sessionId)
      const ownQueueLen = queueState.getQueueForSession(sessionId).length
      const approvalPending = !!get().sessions[sessionId]?.pendingPermission
      if (
        sameSessionRunning ||
        approvalPending ||
        ownQueueLen > 0 ||
        resyncingSessions.has(sessionId)
      ) {
        const passthroughOptions = options
          ? {
              displayContent: options.displayContent,
              designGeneration: options.designGeneration,
            }
          : undefined
        queueState.enqueue(sessionId, content, attachments, passthroughOptions)
        return
      }
    }
    if (!isMemberSession && !options?.__internalDrain && wsManager.isAbandoned(sessionId)) {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('chat.connectionLost'),
        duration: 10000,
        sessionId,
        action: {
          label: t('chat.reconnect'),
          onClick: () => {
            get().connectToSession(sessionId, { force: true })
          },
        },
      })
      return
    }

    consumePendingThinking(sessionId)
    clearPendingToolArgs(sessionId)
    const taskStore = useCLITaskStore.getState()
    const sessionTasks = taskStore.tasksBySessionId[sessionId] ?? []
    const allTasksDone = sessionTasks.length > 0 && sessionTasks.every((t) => t.status === 'completed')
    const completedTaskSummary = allTasksDone
      ? sessionTasks.map((t) => ({ id: t.id, subject: t.subject, status: t.status, activeForm: t.activeForm }))
      : []

    if (!isMemberSession && allTasksDone) {
      void taskStore.resetCompletedTasks(sessionId)
    }

    const clientMsgId =
      !isMemberSession && !options?.designGeneration ? crypto.randomUUID() : null

    continuationPrefixBySession.delete(sessionId)
    set((s) => {
      const session = s.sessions[sessionId] ?? createDefaultSessionState()
      const bufferedDelta = consumePendingDelta(sessionId)
      const pendingAssistantText = `${session.streamingText}${bufferedDelta}`

      const newMessages = pendingAssistantText.trim()
        ? appendAssistantTextMessage(session.messages, pendingAssistantText, Date.now(), undefined, echoDedupOptions(sessionId))
        : [...session.messages]
      if (!isMemberSession && allTasksDone) {
        newMessages.push({
          id: nextId(),
          type: 'task_summary',
          tasks: completedTaskSummary,
          timestamp: Date.now(),
        })
      }
      const userMessage: UIMessage = {
        id: nextId(),
        type: 'user_text',
        content: userFacingContent,
        attachments: isMemberSession ? undefined : uiAttachments,
        timestamp: Date.now(),
        ...designRefFieldsFrom(options?.designGeneration),
        ...(isMemberSession ? { pending: true } : {}),
        ...(clientMsgId ? { pending: true, clientMsgId } : {}),
      }
      newMessages.push(userMessage)

      return {
        sessions: {
          ...s.sessions,
          [sessionId]: {
            ...session,
            messages: newMessages,
            chatState: 'thinking',
            stopRequested: false,
            elapsedSeconds: 0,
            planningPhaseStartedAt: Date.now(),
            streamingText: '',
            statusVerb: isMemberSession ? '' : randomSpinnerVerb(),
            planningPhaseAction: 'waiting_model',
            planningPhaseDetail: '',
            activeThinkingId: null,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
            pendingPermission: null,
            streamingToolArgs: null,
            connectionState: isMemberSession ? 'connected' : session.connectionState,
          },
        },
      }
    })

    if (!isMemberSession) {
      elapsedLastTickAtBySession.set(sessionId, Date.now())
      ensureElapsedTicker()
    }

    if (isMemberSession) {
      void useTeamStore.getState().sendMessageToMember(sessionId, userFacingContent)
        .catch((err) => {
          set((s) => ({
            sessions: updateSessionIn(s.sessions, sessionId, (session) => ({
              chatState: 'idle',
              messages: [
                ...session.messages,
                {
                  id: nextId(),
                  type: 'error',
                  message: err instanceof Error ? err.message : String(err),
                  code: 'TEAM_MEMBER_MESSAGE_FAILED',
                  timestamp: Date.now(),
                },
              ],
            })),
          }))
        })
      return
    }

    if (runtimeSyncRetrySessions.has(sessionId)) {
      runtimeSyncRetrySessions.delete(sessionId)
      void ensureSessionRuntimeSynced(sessionId, { persist: false })
        .then(() => {
          runtimeSyncFailureToastSessions.delete(sessionId)
        })
        .catch((err) => {
          handleRuntimeSyncFailure(sessionId, err)
        })
    }
    queueSessionRuntimeSync(sessionId, { persist: false })
    if (options?.designGeneration) {
      pendingDesignCanvasReveal.add(sessionId)
      if (!options.__internalDrain) {
        lastDirectSendBySession.set(sessionId, {
          content,
          attachments,
          displayContent: options.displayContent,
          designGeneration: options.designGeneration,
          at: Date.now(),
        })
      }
      wsManager.send(sessionId, {
        type: 'start_design_generation',
        submode: options.designGeneration.submode,
        params: options.designGeneration.params,
        brief: content,
        attachments,
        refArtifact: options.designGeneration.refArtifact,
        refArtifactName: options.designGeneration.refArtifactName,
        refElement: options.designGeneration.refElement,
        refElementLabel: options.designGeneration.refElementLabel,
      })
    } else {
      if (!options?.__internalDrain) {
        lastDirectSendBySession.set(sessionId, {
          content,
          attachments,
          displayContent: options?.displayContent,
          at: Date.now(),
        })
      }
      wsManager.send(sessionId, {
        type: 'user_message',
        content,
        attachments,
        ...(clientMsgId ? { clientMsgId } : {}),
        ...(options?.displayContent?.trim()
          ? { displayContent: options.displayContent.trim() }
          : {}),
      })
    }
  },

  respondToPermission: (sessionId, requestId, allowed, options) => {
    if (wsManager.isAbandoned(sessionId)) {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('chat.connectionLost'),
        duration: 10000,
        sessionId,
        action: {
          label: t('chat.reconnect'),
          onClick: () => {
            get().connectToSession(sessionId, { force: true })
          },
        },
      })
      return false
    }
    wsManager.send(sessionId, {
      type: 'permission_response',
      requestId,
      allowed,
      ...(options?.rule ? { rule: options.rule } : {}),
      ...(options?.updatedInput ? { updatedInput: options.updatedInput } : {}),
    })
    set((s) => {
      const session = s.sessions[sessionId]
      const pending = session?.pendingPermission
      const isQuestion = isAskQuestionToolName(pending?.toolName)
      let messages = session?.messages ?? []
      if (
        allowed &&
        isQuestion &&
        pending &&
        options?.updatedInput &&
        typeof options.updatedInput === 'object'
      ) {
        const items = collectPlanAnswerItems(pending.input, options.updatedInput)
        const detailsRaw = (options.updatedInput as Record<string, unknown>).details
        const details =
          typeof detailsRaw === 'string' && detailsRaw.trim() ? detailsRaw.trim() : undefined
        if (items.length > 0 || details) {
          messages = [
            ...messages,
            {
              id: nextId(),
              type: 'plan_question_answers',
              timestamp: Date.now(),
              items,
              ...(details ? { details } : {}),
            },
          ]
        }
      }
      return {
        sessions: updateSessionIn(s.sessions, sessionId, () => ({
          pendingPermission: null,
          chatState: allowed ? 'tool_executing' : 'thinking',
          planningPhaseStartedAt: Date.now(),
          messages: allowed ? messages : resolveDanglingCuratorCards(messages),
        })),
      }
    })
    return true
  },

  setSessionRuntime: (sessionId, selection, options) => {
    wsManager.send(sessionId, {
      type: 'set_runtime_config',
      persist: options?.persist ?? true,
      ...selection,
    })
  },

  setSessionPermissionMode: (sessionId, mode) => {
    wsManager.send(sessionId, { type: 'set_permission_mode', mode })
  },

  setSessionCodingMode: (sessionId, mode, scope = 'session') => {
    if (!get().sessions[sessionId]) return
    wsManager.send(sessionId, { type: 'set_coding_mode', mode, scope, confirmed: false })
  },

  resolveSessionCodingMode: (confirmed) => {
    const pending = get().pendingSessionCodingMode
    if (!pending) return
    set({ pendingSessionCodingMode: null })
    if (!confirmed) return
    if (!get().sessions[pending.sessionId]) return
    wsManager.send(pending.sessionId, {
      type: 'set_coding_mode',
      mode: pending.mode,
      scope: pending.scope,
      confirmed: true,
    })
  },

  setSessionDebugSubmode: (sessionId, submode, params) => {
    if (!get().sessions[sessionId]) return
    wsManager.send(sessionId, { type: 'set_debug_submode', submode, params })
  },

  dismissAgentTaskNotification: (sessionId, toolUseId) => {
    if (!get().sessions[sessionId]?.agentTaskNotifications[toolUseId]) return
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, (session) => {
        const next = { ...session.agentTaskNotifications }
        delete next[toolUseId]
        return { agentTaskNotifications: next }
      }),
    }))
  },

  cancelTool: (sessionId, toolUseId) => {
    wsManager.send(sessionId, { type: 'cancel_tool', sessionId, toolUseId })
  },

  reconcileStuckSession: (sessionId) => {
    const session = get().sessions[sessionId]
    if (!session) return
    const isStuckActive =
      session.chatState === 'thinking' ||
      session.chatState === 'tool_executing' ||
      session.chatState === 'streaming' ||
      session.chatState === 'awaiting_workers'
    if (!isStuckActive) {
      const deferTimer = stuckReconcileDeferTimers.get(sessionId)
      if (deferTimer) {
        clearTimeout(deferTimer)
        stuckReconcileDeferTimers.delete(sessionId)
      }
      if (session.stopRequested) {
        set((s) => ({
          sessions: updateSessionIn(s.sessions, sessionId, () => ({
            stopRequested: false,
          })),
        }))
      }
      return
    }
    const lastActivityAt = lastStreamActivityAtBySession.get(sessionId) ?? 0
    const sinceLastActivity = Date.now() - lastActivityAt
    const hasBufferedStream =
      hasPendingDelta(sessionId) || hasPendingThinking(sessionId)
    if (hasBufferedStream || sinceLastActivity < STUCK_RECONCILE_QUIET_MS) {
      const existingTimer = stuckReconcileDeferTimers.get(sessionId)
      if (existingTimer) clearTimeout(existingTimer)
      stuckReconcileDeferTimers.set(
        sessionId,
        setTimeout(() => {
          stuckReconcileDeferTimers.delete(sessionId)
          if (useSessionRunStateStore.getState().running.has(sessionId)) return
          get().reconcileStuckSession(sessionId)
        }, STUCK_RECONCILE_QUIET_MS),
      )
      if (hasBufferedStream) {
        set((s) => {
          const cur = s.sessions[sessionId]
          if (!cur) return s
          const absorbed = absorbPendingStreamIntoSession(sessionId, cur)
          return {
            sessions: updateSessionIn(s.sessions, sessionId, () => absorbed),
          }
        })
      }
      return
    }
    const deferTimer = stuckReconcileDeferTimers.get(sessionId)
    if (deferTimer) {
      clearTimeout(deferTimer)
      stuckReconcileDeferTimers.delete(sessionId)
    }
    clearPendingToolArgs(sessionId)
    continuationPrefixBySession.delete(sessionId)
    set((s) => {
      const cur = s.sessions[sessionId]
      if (!cur) return s
      const merged = flushPendingDeltaIntoStreaming(sessionId, cur.streamingText)
      let baseMessages = sealThinkingForSession(sessionId, cur)
      if (merged.trim()) {
        baseMessages = appendAssistantTextMessage(baseMessages, merged, Date.now(), undefined, echoDedupOptions(sessionId))
      }
      return {
        sessions: updateSessionIn(s.sessions, sessionId, () => ({
          messages: baseMessages,
          streamingText: '',
          stopRequested: false,
          chatState: 'idle',
          statusVerb: '',
          activeToolUseId: null,
          activeToolName: null,
          activeTaskToolUseId: null,
          activeThinkingId: null,
          activeThinkingContent: '',
          activeThinkingStartedAt: null,
          activeThinkingLastChunkAt: null,
          streamingToolArgs: null,
        })),
      }
    })
    useTabStore.getState().updateTabStatus(sessionId, 'idle')
    void get().reloadHistory(sessionId)
  },

  stopGeneration: (sessionId) => {
    {
      const pending = get().sessions[sessionId]?.pendingPermission
      if (pending) {
        wsManager.send(sessionId, {
          type: 'permission_response',
          requestId: pending.requestId,
          allowed: false,
        })
      }
    }
    wsManager.send(sessionId, { type: 'stop_generation' })
    const pendingDelta = consumePendingDelta(sessionId)
    clearPendingToolArgs(sessionId)
    continuationPrefixBySession.delete(sessionId)
    set((s) => {
      const session = s.sessions[sessionId]
      if (!session) return s
      const sealedMessages = sealThinkingForSession(sessionId, session)
      const partialText = session.streamingText + pendingDelta
      const committedMessages = resolveDanglingCuratorCards(
        partialText.trim()
          ? appendAssistantTextMessage(sealedMessages, partialText, Date.now(), undefined, echoDedupOptions(sessionId))
          : sealedMessages,
      )
      return {
        sessions: {
          ...s.sessions,
          [sessionId]: {
            ...session,
            messages: committedMessages,
            streamingText: '',
            chatState: 'idle',
            stopRequested: true,
            planningPhaseStartedAt: null,
            pendingPermission: null,
            activeThinkingId: null,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
            activeToolUseId: null,
            activeToolName: null,
            activeTaskToolUseId: null,
            pendingResourceWaits: [],
            providerRetry: null,
            streamingToolArgs: null,
          },
        },
      }
    })
    useTabStore.getState().updateTabStatus(sessionId, 'idle')
  },

  loadHistory: async (sessionId) => {
    try {
      const {
        uiMessages,
        restoredNotifications,
        lastTodos,
        hasMessagesAfterTaskCompletion,
        restoredSubagentTimelines,
        pendingRewind,
        historyFirstIndex,
        historyHasMore,
      } = await fetchAndMapSessionHistory(sessionId)
      const taggedMessages = applySupersededFromPendingRewind(uiMessages, pendingRewind)
      set((state) => {
        const session = state.sessions[sessionId]
        if (!session) return state
        if (session.historyLoaded === true) return state
        const uiBusy = isSessionUiBusy(session.chatState)
        const absorbed = uiBusy
          ? absorbPendingStreamIntoSession(sessionId, session)
          : {
              messages: commitIdleEphemeralTranscript(sessionId, session),
              streamingText: '',
              activeThinkingId: null as string | null,
              activeThinkingContent: '',
              activeThinkingStartedAt: null as number | null,
              activeThinkingLastChunkAt: null as number | null,
            }
        const mergedRaw = dropHydratedBlockedToolUses(
          sessionId,
          mergeHydratedHistoryWithLiveUi(taggedMessages, absorbed.messages),
        )
        const merged = uiBusy ? mergedRaw : resolveDanglingCuratorCards(mergedRaw)
        bumpHistoryGeneration(sessionId)
        return {
          sessions: updateSessionIn(state.sessions, sessionId, (s) => ({
            messages: merged,
            streamingText: uiBusy ? absorbed.streamingText : '',
            activeThinkingId: uiBusy
              ? remapActiveThinkingId(
                  merged,
                  absorbed.activeThinkingId,
                  absorbed.activeThinkingContent,
                )
              : null,
            activeThinkingContent: uiBusy ? absorbed.activeThinkingContent : '',
            activeThinkingStartedAt: uiBusy ? absorbed.activeThinkingStartedAt : null,
            activeThinkingLastChunkAt: uiBusy ? absorbed.activeThinkingLastChunkAt : null,
            agentTaskNotifications: mergeAgentNotifications(
              s.agentTaskNotifications,
              restoredNotifications,
            ),
            subagentTimelines: mergeSubagentTimelineRecords(
              s.subagentTimelines,
              restoredSubagentTimelines,
            ),
            pendingRewind,
            historyLoaded: true,
            historyFirstIndex,
            historyHasMore,
            historyReloadNonce: (s.historyReloadNonce ?? 0) + 1,
          })),
        }
      })
      if (lastTodos && lastTodos.length > 0) {
        const taskStore = useCLITaskStore.getState()
        const existing = taskStore.tasksBySessionId[sessionId] ?? []
        if (existing.length === 0) taskStore.setTasksFromTodos(lastTodos, sessionId)
      } else {
        useCLITaskStore.getState().setTasksFromTodos([], sessionId)
      }
      if (hasMessagesAfterTaskCompletion) {
        useCLITaskStore.getState().markCompletedAndDismissed(sessionId)
      }
      void useWorkersStore.getState().fetchByParent(sessionId)
      clearHistoryLoadRetry(sessionId)
    } catch {
      scheduleHistoryLoadRetry(sessionId, () => get().loadHistory(sessionId))
    }
  },

  reloadHistory: async (sessionId) => {
    try {
      const {
        uiMessages,
        restoredNotifications,
        lastTodos,
        hasMessagesAfterTaskCompletion,
        restoredSubagentTimelines,
        pendingRewind,
        historyFirstIndex,
        historyHasMore,
      } = await fetchAndMapSessionHistory(sessionId)
      const taggedMessages = applySupersededFromPendingRewind(uiMessages, pendingRewind)

      set((state) => {
        const session = state.sessions[sessionId]
        if (!session) return state
        bumpHistoryGeneration(sessionId)
        return {
          sessions: updateSessionIn(state.sessions, sessionId, (s) => {
            const keepPermission =
              s.chatState === 'permission_pending' && s.pendingPermission != null
            const uiBusy = keepPermission || isSessionUiBusy(s.chatState)
            const absorbed = uiBusy
              ? absorbPendingStreamIntoSession(sessionId, s)
              : {
                  messages: commitIdleEphemeralTranscript(sessionId, s),
                  streamingText: '',
                  activeThinkingId: null as string | null,
                  activeThinkingContent: '',
                  activeThinkingStartedAt: null as number | null,
                  activeThinkingLastChunkAt: null as number | null,
                }
            const localMessages = absorbed.messages
            let overlapStart = -1
            if (!pendingRewind) {
              const hydratedKeys = new Set(
                taggedMessages.map((m) => m.rawId ?? m.id),
              )
              for (let i = 0; i < localMessages.length; i++) {
                const m = localMessages[i]!
                if (hydratedKeys.has(m.rawId ?? m.id)) {
                  overlapStart = i
                  break
                }
              }
            }
            let stitchCut = overlapStart
            while (stitchCut > 0) {
              const prevRow = localMessages[stitchCut - 1]!
              if (rawIndexFromMessageId(prevRow.rawId ?? prevRow.id) !== null) break
              stitchCut--
            }
            let prefix: UIMessage[] = []
            let liveWindow: UIMessage[] = localMessages
            if (stitchCut > 0) {
              prefix = localMessages.slice(0, stitchCut)
              liveWindow = localMessages.slice(stitchCut)
            } else if (overlapStart < 0) {
              let liveTailStart = localMessages.length
              while (liveTailStart > 0) {
                const row = localMessages[liveTailStart - 1]!
                if (rawIndexFromMessageId(row.rawId ?? row.id) !== null) break
                liveTailStart--
              }
              if (pendingRewind) {
                liveWindow = localMessages.slice(liveTailStart)
              } else if (liveTailStart > 0) {
                let maxLocalRaw = -1
                for (let i = 0; i < liveTailStart; i++) {
                  const row = localMessages[i]!
                  const idx = rawIndexFromMessageId(row.rawId ?? row.id)
                  if (idx !== null && idx > maxLocalRaw) maxLocalRaw = idx
                }
                if (
                  maxLocalRaw >= 0 &&
                  typeof historyFirstIndex === 'number' &&
                  maxLocalRaw < historyFirstIndex &&
                  historyFirstIndex - maxLocalRaw <= 1
                ) {
                  prefix = localMessages.slice(0, liveTailStart)
                }
                liveWindow = localMessages.slice(liveTailStart)
              }
            }
            const mergedRaw = dropHydratedBlockedToolUses(
              sessionId,
              mergeHydratedHistoryWithLiveUi(taggedMessages, liveWindow),
            )
            const mergedTail = keepPermission
              ? mergedRaw
              : resolveDanglingCuratorCards(mergedRaw)
            const rebuiltMessages =
              prefix.length > 0 ? [...prefix, ...mergedTail] : mergedTail
            let prefixStable = rebuiltMessages.length >= s.messages.length
            if (prefixStable) {
              for (let i = 0; i < s.messages.length; i++) {
                if (rebuiltMessages[i]!.id !== s.messages[i]!.id) {
                  prefixStable = false
                  break
                }
              }
            }
            return {
              messages: rebuiltMessages,
              agentTaskNotifications: mergeAgentNotifications(
                s.agentTaskNotifications,
                restoredNotifications,
              ),
              subagentTimelines: mergeSubagentTimelineRecords(
                s.subagentTimelines,
                restoredSubagentTimelines,
              ),
              chatState: keepPermission || uiBusy ? s.chatState : 'idle',
              activeThinkingId: uiBusy
                ? remapActiveThinkingId(
                    rebuiltMessages,
                    absorbed.activeThinkingId,
                    absorbed.activeThinkingContent,
                  )
                : null,
              activeThinkingContent: uiBusy ? absorbed.activeThinkingContent : '',
              activeThinkingStartedAt: uiBusy ? absorbed.activeThinkingStartedAt : null,
              activeThinkingLastChunkAt: uiBusy ? absorbed.activeThinkingLastChunkAt : null,
              activeToolUseId: uiBusy ? s.activeToolUseId : null,
              activeToolName: uiBusy ? s.activeToolName : null,
              streamingText: uiBusy ? absorbed.streamingText : '',
              pendingPermission: keepPermission ? s.pendingPermission : null,
              statusVerb: keepPermission || uiBusy ? s.statusVerb : '',
              planningPhaseAction: keepPermission || uiBusy ? s.planningPhaseAction : '',
              planningPhaseDetail: keepPermission || uiBusy ? s.planningPhaseDetail : '',
              pendingRewind,
              historyLoaded: true,
              historyFirstIndex:
                prefix.length > 0 ? s.historyFirstIndex : historyFirstIndex,
              historyHasMore:
                prefix.length > 0 ? s.historyHasMore : historyHasMore,
              historyReloadNonce: prefixStable
                ? s.historyReloadNonce ?? 0
                : (s.historyReloadNonce ?? 0) + 1,
            }
          }),
        }
      })

      if (lastTodos && lastTodos.length > 0) {
        useCLITaskStore.getState().setTasksFromTodos(lastTodos, sessionId)
      } else {
        useCLITaskStore.getState().setTasksFromTodos([], sessionId)
      }
      if (hasMessagesAfterTaskCompletion) {
        useCLITaskStore.getState().markCompletedAndDismissed(sessionId)
      }
      void useWorkersStore.getState().fetchByParent(sessionId)
      clearHistoryLoadRetry(sessionId)
    } catch {
      scheduleHistoryLoadRetry(sessionId, () => get().reloadHistory(sessionId))
    }
  },

  capMessageWindow: (sessionId) => {
    {
      const session = get().sessions[sessionId]
      if (!session || session.historyLoadingOlder === true) return
      if (session.messages.length > MAX_IN_MEMORY_MESSAGES) {
        const hasRawIndexEvidence = session.messages.some(
          (m) => rawIndexFromMessageId(m.rawId ?? m.id) !== null,
        )
        if (!hasRawIndexEvidence) {
          if (
            session.historyLoaded === true &&
            session.chatState === 'idle' &&
            !capReloadInFlight.has(sessionId)
          ) {
            capReloadInFlight.add(sessionId)
            void get()
              .reloadHistory(sessionId)
              .finally(() => capReloadInFlight.delete(sessionId))
          }
          return
        }
      }
    }
    set((state) => {
      const session = state.sessions[sessionId]
      if (!session) return state
      if (session.historyLoadingOlder === true) return state
      const len = session.messages.length
      if (len <= MAX_IN_MEMORY_MESSAGES) return state
      const dropCount = len - (MAX_IN_MEMORY_MESSAGES - MESSAGE_TRIM_CHUNK)
      if (dropCount <= 0) return state
      const dropped = session.messages.slice(0, dropCount)
      const unackedHead = dropped.filter(
        (m) => m.type === 'user_text' && m.pending === true && !!m.clientMsgId,
      )
      const droppedToolUseIds = new Set<string>()
      for (const m of dropped) {
        if (m.type === 'tool_use' && m.toolUseId) {
          droppedToolUseIds.add(m.toolUseId)
        }
      }
      const retained =
        droppedToolUseIds.size > 0
          ? session.messages
              .slice(dropCount)
              .filter(
                (m) => !(m.type === 'tool_result' && droppedToolUseIds.has(m.toolUseId)),
              )
          : session.messages.slice(dropCount)
      const trimmed = [...unackedHead, ...retained]
      const prevFirst = session.historyFirstIndex ?? 0
      const minRetainedRawIndex = trimmed.reduce<number | null>((acc, m) => {
        const idx = rawIndexFromMessageId(m.rawId ?? m.id)
        if (idx === null) return acc
        return acc === null ? idx : Math.min(acc, idx)
      }, null)
      const maxDroppedRawIndex = dropped.reduce<number | null>((acc, m) => {
        const idx = rawIndexFromMessageId(m.rawId ?? m.id)
        if (idx === null) return acc
        return acc === null ? idx : Math.max(acc, idx)
      }, null)
      const nextFirst =
        minRetainedRawIndex ??
        (maxDroppedRawIndex !== null ? maxDroppedRawIndex + 1 : prevFirst)
      return {
        sessions: updateSessionIn(state.sessions, sessionId, () => ({
          messages: trimmed,
          historyFirstIndex: Math.max(prevFirst, nextFirst),
          historyHasMore: true,
        })),
      }
    })
  },

  loadOlderHistory: async (sessionId) => {
    const session = get().sessions[sessionId]
    if (!session || session.historyLoadingOlder === true) return
    if (session.historyHasMore !== true) return
    const before = session.historyFirstIndex ?? 0
    if (before <= 0) return
    const generation = historyGenerationBySession.get(sessionId) ?? 0
    set((state) => ({
      sessions: updateSessionIn(state.sessions, sessionId, () => ({
        historyLoadingOlder: true,
      })),
    }))
    try {
      const { messages, firstIndex, hasMore } = await sessionsApi.getMessages(sessionId, {
        limit: HISTORY_PAGE_SIZE,
        before,
      })
      const olderUi = await mapHistoryMessagesToUiMessages(messages)
      for (const m of olderUi) {
        if (m.type === 'user_text' && m.clientMsgId) {
          wsManager.confirmUserMessage(sessionId, m.clientMsgId)
        }
      }
      await waitForScrollQuiet(1200)
      set((state) => {
        const current = state.sessions[sessionId]
        if (!current) return state
        if ((historyGenerationBySession.get(sessionId) ?? 0) !== generation) {
          return {
            sessions: updateSessionIn(state.sessions, sessionId, () => ({
              historyLoadingOlder: false,
            })),
          }
        }
        const knownIds = new Set(current.messages.map((m) => m.id))
        const prepend = olderUi.filter((m) => !knownIds.has(m.id))
        return {
          sessions: updateSessionIn(state.sessions, sessionId, (s) => {
            const combined =
              prepend.length === 0 ? s.messages : [...prepend, ...s.messages]
            const withSuperseded =
              prepend.length > 0 && s.pendingRewind
                ? applySupersededFromPendingRewind(combined, s.pendingRewind)
                : combined
            return {
              messages: withSuperseded,
              historyFirstIndex: firstIndex ?? 0,
              historyHasMore: hasMore === true,
              historyLoadingOlder: false,
            }
          }),
        }
      })
    } catch {
      set((state) => ({
        sessions: updateSessionIn(state.sessions, sessionId, () => ({
          historyLoadingOlder: false,
        })),
      }))
      useUIStore.getState().addToast({
        type: 'error',
        message: t('chat.loadHistoryFailed'),
        duration: 4000,
      })
    }
  },

  queueComposerPrefill: (sessionId, prefill) => {
    set((state) => ({
      sessions: updateSessionIn(state.sessions, sessionId, () => ({
        composerPrefill: {
          text: prefill.text,
          attachments: prefill.attachments,
          nonce: Date.now(),
        },
      })),
    }))
  },

  setComposerDraft: (sessionId, draft) => {
    set((state) => ({
      sessions: updateSessionIn(state.sessions, sessionId, () => ({
        composerDraft: {
          text: draft.text,
          attachments: draft.attachments,
          slashMenuOpen: draft.slashMenuOpen,
        },
      })),
    }))
  },

  clearComposerDraft: (sessionId) => {
    set((state) => ({
      sessions: updateSessionIn(state.sessions, sessionId, () => ({
        composerDraft: undefined,
      })),
    }))
  },

  clearMessages: (sessionId) => {
    set((s) => ({ sessions: updateSessionIn(s.sessions, sessionId, () => ({ messages: [], streamingText: '', chatState: 'idle' })) }))
  },

  restoreRewind: async (sessionId) => {
    const session = get().sessions[sessionId]
    const rewindId = session?.pendingRewind?.rewindId
    if (!rewindId) return
    try {
      await sessionsApi.restoreRewind(sessionId, rewindId)
    } catch (err) {

      set((s) => ({
        sessions: updateSessionIn(s.sessions, sessionId, (sess) => ({
          messages: [
            ...sess.messages,
            {
              id: nextId(),
              type: 'error',
              message: err instanceof Error ? err.message : String(err),
              code: 'REWIND_RESTORE_FAILED',
              timestamp: Date.now(),
            },
          ],
        })),
      }))
      return
    }
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, (sess) => ({
        pendingRewind: null,

        pendingEdits: [],
        keptEdits: [],

        subagentTimelines: {},
        activeTaskToolUseId: null,
        messages: sess.messages.map((m) =>
          m.superseded ? ({ ...(m as UIMessage), superseded: undefined } as UIMessage) : m,
        ),
      })),
    }))
  },

  confirmSendAfterRewind: async (sessionId) => {
    const session = get().sessions[sessionId]
    const pending = session?.pendingSendAfterRewind
    const rewindId = session?.pendingRewind?.rewindId
    if (!pending) return
    if (rewindId) {
      try {
        await sessionsApi.commitRewind(sessionId, rewindId)
      } catch (err) {
        set((s) => ({
          sessions: updateSessionIn(s.sessions, sessionId, (sess) => ({
            pendingSendAfterRewind: null,
            messages: [
              ...sess.messages,
              {
                id: nextId(),
                type: 'error',
                message: err instanceof Error ? err.message : String(err),
                code: 'REWIND_COMMIT_FAILED',
                timestamp: Date.now(),
              },
            ],
          })),
        }))
        return
      }
    }

    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, (sess) => ({
        pendingRewind: null,
        pendingSendAfterRewind: null,
        pendingEdits: [],
        keptEdits: [],
        subagentTimelines: {},
        activeTaskToolUseId: null,
        messages: sess.messages.filter((m) => !m.superseded),
      })),
    }))
    get().sendMessage(sessionId, pending.content, pending.attachments, pending.options)
  },

  cancelSendAfterRewind: (sessionId) => {
    const pending = get().sessions[sessionId]?.pendingSendAfterRewind
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, () => ({
        pendingSendAfterRewind: null,
      })),
    }))
    if (pending && (pending.content.trim() || (pending.attachments?.length ?? 0) > 0)) {
      const attachments = (pending.attachments ?? []).map((a) => ({
        type: a.type,
        name: a.name || a.path || a.mimeType || a.type,
        ...(a.path ? { path: a.path } : {}),
        data: a.data,
        mimeType: a.mimeType,
      }))
      get().queueComposerPrefill(sessionId, {
        text: pending.options?.displayContent?.trim() || pending.content,
        ...(attachments.length > 0 ? { attachments } : {}),
      })
    }
  },

  requestModeSwitch: (sessionId, planPath) => {
    set((s) => {
      const session = s.sessions[sessionId]
      if (!session) return s
      const messages: UIMessage[] = [
        ...session.messages,
        {
          id: nextId(),
          type: 'mode_switch_card',
          timestamp: Date.now(),
          planPath,
          targetMode: 'agent',
          status: 'pending',
          handoffKind: 'plan',
        },
      ]
      return {
        sessions: updateSessionIn(s.sessions, sessionId, () => ({ messages })),
      }
    })
  },

  requestCuratorModeSwitch: (sessionId, implBlueprintPath, _meta) => {
    set((s) => {
      const session = s.sessions[sessionId]
      if (!session) return s
      const messages: UIMessage[] = [
        ...session.messages,
        {
          id: nextId(),
          type: 'mode_switch_card',
          timestamp: Date.now(),
          planPath: implBlueprintPath,
          targetMode: 'agent',
          status: 'pending',
          handoffKind: 'curator',
        },
      ]
      return {
        sessions: updateSessionIn(s.sessions, sessionId, () => ({ messages })),
      }
    })
  },

  confirmModeSwitch: (sessionId, messageId) => {
    const state = get()
    const session = state.sessions[sessionId]
    if (!session) return
    const card = session.messages.find(
      (m) => m.id === messageId && m.type === 'mode_switch_card',
    ) as Extract<UIMessage, { type: 'mode_switch_card' }> | undefined
    if (!card || card.status !== 'pending') return

    const isCurator = card.handoffKind === 'curator'

    void useSettingsStore.getState().setCodingMode('agent')
    wsManager.send(sessionId, { type: 'set_coding_mode', mode: 'agent', scope: 'session' })

    wsManager.send(sessionId, {
      type: 'start_plan_execution',
      planPath: card.planPath,
      ...(isCurator ? { kind: 'curator' as const } : {}),
    })

    set((s) => {
      const sess = s.sessions[sessionId]
      if (!sess) return s
      const planPath = card.planPath
      const messages: UIMessage[] = sess.messages.map((m) => {
        if (m.id === messageId && m.type === 'mode_switch_card') {
          return { ...m, status: 'switched' }
        }
        if (!isCurator && m.type === 'plan_card' && m.planPath === planPath) {
          return { ...m, pendingHydration: true }
        }
        if (isCurator && m.type === 'curator_card' && m.implBlueprintPath === planPath) {
          return { ...m, pendingHydration: true }
        }
        return m
      })
      return {
        sessions: updateSessionIn(s.sessions, sessionId, () => ({
          messages,
          chatState: 'thinking',
          planningPhaseStartedAt: Date.now(),
        })),
      }
    })
  },

  dismissModeSwitch: (sessionId, messageId) => {
    set((s) => {
      const sess = s.sessions[sessionId]
      if (!sess) return s
      const messages: UIMessage[] = sess.messages.map((m) =>
        m.id === messageId && m.type === 'mode_switch_card'
          ? { ...m, status: 'dismissed' }
          : m,
      )
      return {
        sessions: updateSessionIn(s.sessions, sessionId, () => ({ messages })),
      }
    })
  },

  clearPendingEdits: (sessionId) => {
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, (sc) => ({
        pendingEdits: [],
        keptEdits: mergeKeptEdits(sc.keptEdits, sc.pendingEdits),
      })),
    }))
  },

  clearKeptEdits: (sessionId) => {
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, () => ({ keptEdits: [] })),
    }))
  },

  resumePlanExecution: (sessionId, planPath) => {
    if (!sessionId || !planPath) return

    void useSettingsStore.getState().setCodingMode('agent')
    wsManager.send(sessionId, { type: 'set_coding_mode', mode: 'agent', scope: 'session' })
    wsManager.send(sessionId, {
      type: 'start_plan_execution',
      planPath,
      resume: true,
    })
    set((s) => {
      const sess = s.sessions[sessionId]
      if (!sess) {
        return {
          sessions: updateSessionIn(s.sessions, sessionId, () => ({
            chatState: 'thinking',
            planningPhaseStartedAt: Date.now(),
          })),
        }
      }
      const messages: UIMessage[] = sess.messages.map((m) => {
        if (m.type === 'plan_card' && m.planPath === planPath) {
          return { ...m, pendingHydration: true }
        }
        return m
      })
      return {
        sessions: updateSessionIn(s.sessions, sessionId, () => ({
          messages,
          chatState: 'thinking',
          planningPhaseStartedAt: Date.now(),
        })),
      }
    })
  },

  applyPlanCardDocument: (sessionId, messageId, markdown) => {
    const parsed = parsePlanMarkdown(markdown)
    set((s) => {
      const session = s.sessions[sessionId]
      if (!session) return s
      let found = false
      const messages = session.messages.map((m) => {
        if (m.type !== 'plan_card' || m.id !== messageId) return m
        found = true
        const prevById = new Map(m.todos.map((todo) => [todo.id, todo]))
        return {
          ...m,
          markdown,
          title: parsed.title || parsed.name || m.title,
          overview: parsed.overview,
          todos: parsed.todos.map((todo) => {
            const prev = prevById.get(todo.id)
            const notes = prev?.notes
            return {
              id: todo.id,
              content: todo.content,
              status: todo.status,
              ...(notes !== undefined && notes !== null ? { notes } : {}),
            }
          }),
        }
      })
      if (!found) return s
      return {
        sessions: updateSessionIn(s.sessions, sessionId, () => ({ messages })),
      }
    })
  },

  resumeCuratorExecution: (sessionId, implBlueprintPath) => {
    if (!sessionId || !implBlueprintPath) return

    void useSettingsStore.getState().setCodingMode('agent')
    wsManager.send(sessionId, { type: 'set_coding_mode', mode: 'agent', scope: 'session' })
    wsManager.send(sessionId, {
      type: 'start_plan_execution',
      planPath: implBlueprintPath,
      resume: true,
      kind: 'curator',
    })
    set((s) => {
      const sess = s.sessions[sessionId]
      if (!sess) {
        return {
          sessions: updateSessionIn(s.sessions, sessionId, () => ({
            chatState: 'thinking',
            planningPhaseStartedAt: Date.now(),
          })),
        }
      }
      const messages: UIMessage[] = sess.messages.map((m) => {
        if (m.type === 'curator_card' && m.implBlueprintPath === implBlueprintPath) {
          return { ...m, pendingHydration: true }
        }
        return m
      })
      return {
        sessions: updateSessionIn(s.sessions, sessionId, () => ({
          messages,
          chatState: 'thinking',
          planningPhaseStartedAt: Date.now(),
        })),
      }
    })
  },

  continueCuratorWriting: (sessionId) => {
    if (!sessionId) return
    const display = t('curator.continueWritingPrompt') || 'Continue writing the document'
    const instruction =
      'Continue the interrupted Curator document. Reuse the existing research in `sources.md` and ' +
      '`research_notes.md` plus the current `draft.md`. Finish the full polished document in one pass ' +
      'and end this turn by calling `exit_curator_mode` with complete `final_content` and ' +
      '`impl_blueprint` arguments. Do not re-run research that is already sufficient.'
    get().sendMessage(sessionId, instruction, undefined, { displayContent: display })
  },

  undoAllPendingEdits: async (sessionId) => {
    const sess = get().sessions[sessionId]
    if (!sess) return
    const batchIds = Array.from(
      new Set(sess.pendingEdits.flatMap((e) => e.editBatchIds).filter(Boolean)),
    )
    if (batchIds.length === 0) {

      set((s) => ({
        sessions: updateSessionIn(s.sessions, sessionId, () => ({ pendingEdits: [] })),
      }))
      return
    }
    await sessionsApi.revertBatches(sessionId, batchIds)
    const revertedSet = new Set(batchIds)
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, (sc) => ({
        pendingEdits: [],
        keptEdits: sc.keptEdits.filter(
          (e) => !e.editBatchIds.some((id) => revertedSet.has(id)),
        ),
      })),
    }))
  },

  revertToTurnCheckpoint: async (sessionId, suffixBatchIds) => {
    const sess = get().sessions[sessionId]
    if (!sess) return
    const batchIds = Array.from(new Set(suffixBatchIds.filter(Boolean)))
    if (batchIds.length === 0) return
    await sessionsApi.revertBatches(sessionId, batchIds)
    const revertedSet = new Set(batchIds)
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, (sc) => ({
        messages: sc.messages.map((m) =>
          m.type === 'file_edit' && m.editBatchId && revertedSet.has(m.editBatchId)
            ? { ...m, reverted: true }
            : m,
        ),
        pendingEdits: sc.pendingEdits.filter(
          (e) => !e.editBatchIds.some((id) => revertedSet.has(id)),
        ),
        keptEdits: sc.keptEdits.filter(
          (e) => !e.editBatchIds.some((id) => revertedSet.has(id)),
        ),
      })),
    }))
  },

  undoPendingEditFile: async (sessionId, path) => {
    const sess = get().sessions[sessionId]
    if (!sess) return
    const target = sess.pendingEdits.find((e) => e.path === path)
    if (!target) return
    const revertedBatchIds = new Set(target.editBatchIds.filter(Boolean))
    if (revertedBatchIds.size > 0) {
      await sessionsApi.revertBatches(sessionId, Array.from(revertedBatchIds))
    }
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, (sc) => ({
        pendingEdits: sc.pendingEdits.filter((e) => {
          if (e.path === path) return false
          if (revertedBatchIds.size === 0) return true
          return !e.editBatchIds.some((id) => revertedBatchIds.has(id))
        }),
        keptEdits:
          revertedBatchIds.size === 0
            ? sc.keptEdits
            : sc.keptEdits.filter(
                (e) => !e.editBatchIds.some((id) => revertedBatchIds.has(id)),
              ),
      })),
    }))
  },

  keepPendingEditFile: (sessionId, path) => {
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, (sc) => {
        const target = sc.pendingEdits.find((e) => e.path === path)
        return {
          pendingEdits: sc.pendingEdits.filter((e) => e.path !== path),
          ...(target ? { keptEdits: mergeKeptEdits(sc.keptEdits, [target]) } : {}),
        }
      }),
    }))
  },

  resetDebugPiiStats: (sessionId) => {
    if (!sessionId) return
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, () => ({
        debugPiiStats: { total: 0, counts: {}, lastEventAt: null },
      })),
    }))
  },

  handleServerMessage: (sessionId, msg) => {
    const update = (updater: (session: PerSessionState) => Partial<PerSessionState>) => {
      set((s) => ({ sessions: updateSessionIn(s.sessions, sessionId, updater) }))
    }

    {
      const frameTurnSeq = (msg as { turnSeq?: unknown }).turnSeq
      if (typeof frameTurnSeq === 'number' && Number.isFinite(frameTurnSeq)) {
        const active = activeTurnSeqBySession.get(sessionId) ?? 0
        if (frameTurnSeq > active) {
          activeTurnSeqBySession.set(sessionId, frameTurnSeq)
        } else if (frameTurnSeq < active) {
          if (
            msg.type === 'tool_use_complete' ||
            msg.type === 'tool_result' ||
            msg.type === 'content_delta' ||
            msg.type === 'content_start' ||
            msg.type === 'message_complete'
          ) {
            dirtyMidTurnSessions.add(sessionId)
          }
          return
        }
      }
    }

    const turnWasStopped = get().sessions[sessionId]?.stopRequested === true

    {
      const guardSession = get().sessions[sessionId]
      if (guardSession?.stopRequested) {
        const isStatusIdle = msg.type === 'status' && msg.state === 'idle'
        const isTurnEnd =
          isStatusIdle || msg.type === 'message_complete' || msg.type === 'error'
        const passThrough =
          isTurnEnd ||
          msg.type === 'connected' ||
          msg.type === 'pong' ||
          msg.type === 'session_title_updated' ||
          msg.type === 'session_history_changed' ||
          msg.type === 'task_update' ||
          msg.type === 'lsp_diagnostics' ||
          msg.type === 'permission_request' ||
          msg.type === 'system_notification' ||
          msg.type === 'tool_use_complete' ||
          msg.type === 'tool_result' ||
          msg.type === 'worker_spawned' ||
          msg.type === 'worker_status' ||
          msg.type === 'worker_progress' ||
          msg.type === 'worker_completed' ||
          msg.type === 'worker_stopped' ||
          msg.type === 'parent_resumed'
        if (!passThrough) {
          return
        }
        if (isStatusIdle || msg.type === 'message_complete' || msg.type === 'error') {
          set((s) => ({
            sessions: updateSessionIn(s.sessions, sessionId, () => ({
              stopRequested: false,
            })),
          }))
          setTimeout(() => {
            const st = get().sessions[sessionId]
            if (!st) return
            const busy =
              st.chatState === 'streaming' ||
              st.chatState === 'thinking' ||
              st.chatState === 'tool_executing' ||
              st.chatState === 'permission_pending'
            if (busy) return
            void get().reloadHistory(sessionId)
          }, 150)
        }
      }
    }

    switch (msg.type) {
      case 'connected':

        activeTurnSeqBySession.delete(sessionId)
        void hydrateCumulativeTokensFromUsage(sessionId)
        set((s) => {
          const session = s.sessions[sessionId]
          if (!session) return s
          const cleaned = stripNoModelErrorMessages(session.messages)
          if (cleaned === session.messages && session.connectionState === 'connected') {
            return s
          }
          return {
            sessions: {
              ...s.sessions,
              [sessionId]: {
                ...session,
                messages: cleaned,
                connectionState: 'connected',
              },
            },
          }
        })
        break

      case 'status':
        if (msg.state === 'idle') {
          continuationPrefixBySession.delete(sessionId)
        }
        update((session) => {
          const pendingText = `${session.streamingText}${consumePendingDelta(sessionId)}`
          const hasPendingStreamText = pendingText.trim().length > 0

          const preserveStreamingTurn = hasPendingStreamText && msg.state !== 'idle'
          const shouldFlush = hasPendingStreamText && msg.state === 'idle'
          const keepPending = msg.state === 'idle' && !!session.pendingPermission
          const sealedMessages =
            msg.state === 'idle'
              ? sealThinkingForSession(sessionId, session)
              : session.messages
          const baseMessages =
            msg.state === 'idle' && !session.pendingPermission
              ? resolveDanglingCuratorCards(sealedMessages)
              : sealedMessages
          return {
            chatState: preserveStreamingTurn
              ? 'streaming'
              : keepPending
                ? 'permission_pending'
                : msg.state,
            ...(msg.verb && msg.verb !== 'Thinking' ? { statusVerb: msg.verb } : {}),
            ...(msg.tokens ? { tokenUsage: { ...session.tokenUsage, output_tokens: msg.tokens } } : {}),
            ...(session.chatState === 'idle' && msg.state !== 'idle'
              ? { planningPhaseStartedAt: Date.now() }
              : {}),
            ...(msg.state === 'idle' ? {
              activeThinkingId: null,
              activeThinkingContent: '',
              activeThinkingStartedAt: null,
              activeThinkingLastChunkAt: null,
              statusVerb: '',
              planningPhaseAction: '',
              planningPhaseDetail: '',
              planningPhaseStartedAt: null,
              streamingToolArgs: null,
            } : {
              planningPhaseAction: resolvePlanningPhaseAction(
                msg.verb,
                session.planningPhaseAction,
              ),
              planningPhaseDetail:
                typeof msg.detail === 'string' && msg.detail.length > 0
                  ? msg.detail
                  : session.planningPhaseDetail,
            }),
            ...(shouldFlush ? {
              messages: appendAssistantTextMessage(baseMessages, pendingText, Date.now(), undefined, echoDedupOptions(sessionId)),
              streamingText: '',
            } : msg.state === 'idle' && baseMessages !== session.messages ? {
              messages: baseMessages,
              ...(pendingText !== session.streamingText ? { streamingText: pendingText } : {}),
            } : pendingText !== session.streamingText ? { streamingText: pendingText } : {}),
          }
        })
        if (msg.state === 'idle') {
          clearPendingToolArgs(sessionId)
          syncTasksAfterTurnEnd(sessionId, turnWasStopped)
          revealDesignCanvasIfPending(sessionId)
          if (dirtyMidTurnSessions.delete(sessionId)) {
            void get().reloadHistory(sessionId)
          }
        }

        {
          const sAfterStatus = get().sessions[sessionId]
          const tabIsIdle = msg.state === 'idle' && !sAfterStatus?.pendingPermission
          useTabStore.getState().updateTabStatus(sessionId, tabIsIdle ? 'idle' : 'running')
        }
        break

      case 'content_start': {
        const session = get().sessions[sessionId]
        if (!session) break
        const pendingText = `${session.streamingText}${consumePendingDelta(sessionId)}`
        const flushText = msg.blockType !== 'text' && pendingText.trim().length > 0
        if (msg.blockType === 'text') {
          update((s) => ({
            messages: sealThinkingForSession(sessionId, s),
            ...(pendingText !== s.streamingText ? { streamingText: pendingText } : {}),
            chatState: 'streaming',
            activeThinkingId: null,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
            providerRetry: null,
            planningPhaseAction: '',
            planningPhaseDetail: '',
          }))
        } else if (msg.blockType === 'tool_use') {
          continuationPrefixBySession.delete(sessionId)
          update((s) => {
            const flushed = flushText
              ? appendAssistantTextMessage(s.messages, pendingText, Date.now(), undefined, echoDedupOptions(sessionId))
              : s.messages
            return {
              messages: sealThinkingForSession(sessionId, { ...s, messages: flushed }),
              ...(flushText ? { streamingText: '' } : {}),
              activeToolUseId: msg.toolUseId ?? null,
              activeToolName: msg.toolName ?? null,
              chatState: 'tool_executing',
              activeThinkingId: null,
              activeThinkingContent: '',
              activeThinkingStartedAt: null,
              activeThinkingLastChunkAt: null,
              providerRetry: null,
              planningPhaseAction: '',
              planningPhaseDetail: '',
            }
          })

          if (msg.toolName && isBrowserFamilyTool(msg.toolName) && !isDesignerSession(sessionId)) {
            void useBrowserPanelStore.getState().openForTool(sessionId, {
              source: 'tool',
              url: null,
              presentOnly: true,
            })
          }
        } else if (flushText) {
          continuationPrefixBySession.delete(sessionId)
          update((s) => ({
            messages: appendAssistantTextMessage(s.messages, pendingText, Date.now(), undefined, echoDedupOptions(sessionId)),
            streamingText: '',
          }))
        }
        break
      }

      case 'content_delta':
        if (msg.text !== undefined) {
          lastStreamActivityAtBySession.set(sessionId, Date.now())
          const prev = pendingDeltaBySession.get(sessionId) ?? ''
          const next = prev + msg.text
          pendingDeltaBySession.set(sessionId, next)
          if (!pendingDeltaFirstAt.has(sessionId)) {
            pendingDeltaFirstAt.set(sessionId, nowMs())
          }
          const firstAt = pendingDeltaFirstAt.get(sessionId) ?? nowMs()
          const elapsed = nowMs() - firstAt

          const flushDelta = () => {
            const pendingHandle = flushTimerBySession.get(sessionId)
            if (pendingHandle) {
              cancelScheduledFlush(pendingHandle)
              flushTimerBySession.delete(sessionId)
            }
            deferredDeltaFlush.delete(sessionId)
            if (isWindowBusy()) {
              const busyNow = Date.now()
              const lastAt = lastBusyFlushAtBySession.get(sessionId) ?? 0
              const sinceLast = busyNow - lastAt
              if (sinceLast < BUSY_FLUSH_MAX_DEFER_MS) {
                deferredDeltaFlush.set(sessionId, flushDelta)
                ensureBusyIdleFlush()
                if (!flushTimerBySession.has(sessionId)) {
                  const id = setTimeout(
                    () => {
                      flushTimerBySession.delete(sessionId)
                      deferredDeltaFlush.delete(sessionId)
                      flushDelta()
                    },
                    Math.max(16, BUSY_FLUSH_MAX_DEFER_MS - sinceLast),
                  ) as unknown as number
                  flushTimerBySession.set(sessionId, { kind: 'timeout', id })
                }
                return
              }
              lastBusyFlushAtBySession.set(sessionId, busyNow)
            }
            const text = pendingDeltaBySession.get(sessionId) ?? ''
            pendingDeltaBySession.delete(sessionId)
            pendingDeltaFirstAt.delete(sessionId)
            if (text) {
              update((s) => ({ streamingText: s.streamingText + text }))
            }
          }

          if (next.length >= FLUSH_HIGH_WATER_CHARS || elapsed >= FLUSH_HIGH_WATER_MS) {
            const existing = flushTimerBySession.get(sessionId)
            if (existing !== undefined) {
              cancelScheduledFlush(existing)
              flushTimerBySession.delete(sessionId)
            }
            flushDelta()
          } else if (!flushTimerBySession.has(sessionId)) {
            const id = scheduleRafCallback(flushDelta)
            flushTimerBySession.set(sessionId, id)
          }
        }
        break

      case 'content_checkpoint': {
        update((s) => {
          const pendingText = `${s.streamingText}${consumePendingDelta(sessionId)}`
          if (pendingText.trim()) {
            continuationPrefixBySession.set(sessionId, pendingText)
          } else {
            continuationPrefixBySession.delete(sessionId)
          }
          return pendingText !== s.streamingText ? { streamingText: pendingText } : {}
        })
        break
      }

      case 'content_reset': {
        consumePendingDelta(sessionId)
        clearPendingToolArgs(sessionId)
        const restoredText = continuationPrefixBySession.get(sessionId) ?? ''
        update((s) => {
          const patch = mergePendingThinkingIntoActive(s, sessionId)
          return {
            streamingText: restoredText,
            streamingToolArgs: null,
            planningPhaseStartedAt: Date.now(),
            activeThinkingId: patch.activeThinkingId,
            activeThinkingContent: patch.activeThinkingId
              ? patch.activeThinkingContent
              : '',
            activeThinkingStartedAt: patch.activeThinkingStartedAt,
            activeThinkingLastChunkAt: patch.activeThinkingLastChunkAt,
          }
        })
        break
      }

      case 'thinking':
        if (msg.text !== undefined) {
          lastStreamActivityAtBySession.set(sessionId, Date.now())
          const prev = pendingThinkingBySession.get(sessionId) ?? ''
          const next = prev + msg.text
          pendingThinkingBySession.set(sessionId, next)
          if (!pendingThinkingFirstAt.has(sessionId)) {
            pendingThinkingFirstAt.set(sessionId, nowMs())
          }
          const firstAt = pendingThinkingFirstAt.get(sessionId) ?? nowMs()
          const elapsed = nowMs() - firstAt

          const flushThinking = () => {
            const pendingHandle = thinkingFlushTimerBySession.get(sessionId)
            if (pendingHandle) {
              cancelScheduledFlush(pendingHandle)
              thinkingFlushTimerBySession.delete(sessionId)
            }
            deferredThinkingFlush.delete(sessionId)
            if (isWindowBusy()) {
              const busyNow = Date.now()
              const lastAt = lastBusyFlushAtBySession.get(sessionId) ?? 0
              const sinceLast = busyNow - lastAt
              if (sinceLast < BUSY_FLUSH_MAX_DEFER_MS) {
                deferredThinkingFlush.set(sessionId, flushThinking)
                ensureBusyIdleFlush()
                if (!thinkingFlushTimerBySession.has(sessionId)) {
                  const id = setTimeout(
                    () => {
                      thinkingFlushTimerBySession.delete(sessionId)
                      deferredThinkingFlush.delete(sessionId)
                      flushThinking()
                    },
                    Math.max(16, BUSY_FLUSH_MAX_DEFER_MS - sinceLast),
                  ) as unknown as number
                  thinkingFlushTimerBySession.set(sessionId, { kind: 'timeout', id })
                }
                return
              }
              lastBusyFlushAtBySession.set(sessionId, busyNow)
            }
            const buffered = pendingThinkingBySession.get(sessionId) ?? ''
            pendingThinkingBySession.delete(sessionId)
            pendingThinkingFirstAt.delete(sessionId)
            if (!buffered.trim()) return
            const THINKING_IDLE_THRESHOLD_MS = 5000
            const keepDraftOpen = continuationPrefixBySession.has(sessionId)
            update((s) => {
              const pendingText = `${s.streamingText}${consumePendingDelta(sessionId)}`
              const commitDraft = pendingText.trim().length > 0 && !keepDraftOpen
              const baseMessages = commitDraft
                ? appendAssistantTextMessage(s.messages, pendingText, Date.now(), undefined, echoDedupOptions(sessionId))
                : s.messages
              const hasActive = Boolean(s.activeThinkingId)
              const id = hasActive ? (s.activeThinkingId as string) : nextId()
              const now = Date.now()
              let startedAt: number
              if (!hasActive) {
                startedAt = now
              } else {
                const prevStart = s.activeThinkingStartedAt ?? now
                const lastChunkAt = s.activeThinkingLastChunkAt ?? prevStart
                const realGap = now - lastChunkAt
                if (realGap > THINKING_IDLE_THRESHOLD_MS) {
                  startedAt = prevStart + (realGap - THINKING_IDLE_THRESHOLD_MS)
                } else {
                  startedAt = prevStart
                }
              }
              const prevContent = hasActive ? s.activeThinkingContent : ''
              const nextContent = prevContent + buffered
              const hasThinkingRow =
                hasActive &&
                baseMessages.some((m) => m.id === id && m.type === 'thinking')
              return {
                messages: hasThinkingRow
                  ? baseMessages
                  : commitActiveThinking(baseMessages, id, nextContent, startedAt, false),
                chatState: 'thinking',
                activeThinkingId: id,
                activeThinkingContent: nextContent,
                activeThinkingStartedAt: startedAt,
                activeThinkingLastChunkAt: now,
                planningPhaseAction: '',
                planningPhaseDetail: '',
                ...(commitDraft
                  ? { streamingText: '' }
                  : pendingText !== s.streamingText
                    ? { streamingText: pendingText }
                    : {}),
              }
            })
          }

          if (next.length >= FLUSH_HIGH_WATER_CHARS || elapsed >= FLUSH_HIGH_WATER_MS) {
            const existing = thinkingFlushTimerBySession.get(sessionId)
            if (existing !== undefined) {
              cancelScheduledFlush(existing)
              thinkingFlushTimerBySession.delete(sessionId)
            }
            flushThinking()
          } else if (!thinkingFlushTimerBySession.has(sessionId)) {
            const id = scheduleRafCallback(flushThinking)
            thinkingFlushTimerBySession.set(sessionId, id)
          }
        }
        break

      case 'tool_use_args_delta': {
        const toolName = typeof msg.toolName === 'string' ? msg.toolName : ''
        const argsSnapshot = typeof msg.argsSnapshot === 'string' ? msg.argsSnapshot : ''
        const callIndex = typeof msg.callIndex === 'number' ? msg.callIndex : 0
        if (toolName && argsSnapshot) {
          pendingToolArgsBySession.set(sessionId, { toolName, callIndex, argsSnapshot })
          if (!toolArgsFlushTimerBySession.has(sessionId)) {
            const flushToolArgs = () => {
              toolArgsFlushTimerBySession.delete(sessionId)
              if (isWindowBusy()) {
                deferredDeltaFlush.set(`${sessionId}::toolArgs`, flushToolArgs)
                ensureBusyIdleFlush()
                return
              }
              const latest = pendingToolArgsBySession.get(sessionId)
              pendingToolArgsBySession.delete(sessionId)
              if (latest) {
                update(() => ({ streamingToolArgs: latest }))
              }
            }
            toolArgsFlushTimerBySession.set(sessionId, scheduleRafCallback(flushToolArgs))
          }
        }
        break
      }

      case 'tool_use_complete': {
        clearPendingToolArgs(sessionId)
        const session = get().sessions[sessionId]
        const toolName = msg.toolName || session?.activeToolName || 'unknown'
        const toolUseId = msg.toolUseId || session?.activeToolUseId || ''
        const input = msg.input
        const isPlanSave = isPlanSaveCall(toolName, input)
        const isExitPlanMode = isExitPlanModeCall(toolName)
        const isExitCuratorMode = isExitCuratorModeCall(toolName)
        const isUpdatePlanSet = isUpdatePlanSetCall(toolName, input)
        const isUpdatePlanUpdate = isUpdatePlanUpdateCall(toolName, input)

        const sessionCodingMode =
          get().sessionCodingMode[sessionId] ?? useSettingsStore.getState().codingMode
        const planModeBlocked =
          sessionCodingMode === 'plan' &&
          !isExitPlanMode &&
          !isPlanSave &&
          !isPlanModeAllowedTool(toolName)

        let modeBlockedReason: 'plan' | 'read_only' | 'tool_not_allowed' | null = null
        if (planModeBlocked) {
          modeBlockedReason = 'plan'
        } else if (
          sessionCodingMode !== 'plan' &&
          !isExitPlanMode &&
          !isExitCuratorMode &&
          !isPlanSave &&
          !isUpdatePlanSet &&
          !isUpdatePlanUpdate
        ) {
          const allModes = useSettingsStore.getState().codingModes
          const modeInfo = allModes.find((m) => m.id === sessionCodingMode)
          const allowedTools = modeInfo?.allowedTools
          if (allowedTools && allowedTools.length > 0 && !allowedTools.includes(toolName)) {
            modeBlockedReason =
              modeInfo?.permissionMode === 'plan' && sessionCodingMode === 'ask'
                ? 'read_only'
                : 'tool_not_allowed'
          }
        }

        if (modeBlockedReason) {
          if (toolUseId)
            getOrCreateSessionSet(planModeBlockedToolUseIdsBySession, sessionId).add(
              toolUseId,
            )
          update((s) => {
            const sealed = sealThinkingForSession(sessionId, s)
            const lastIdx = sealed.length - 1
            const last = lastIdx >= 0 ? sealed[lastIdx] : undefined
            if (
              last &&
              last.type === 'plan_mode_blocked' &&
              (last.reason ?? 'plan') === modeBlockedReason &&
              (last.mode ?? 'plan') === sessionCodingMode
            ) {
              const merged: UIMessage = {
                ...last,
                tools: [...last.tools, { name: toolName, input }],
              }
              const next = [...sealed]
              next[lastIdx] = merged
              return {
                messages: next,
                streamingToolArgs: null,
                activeToolUseId: null,
                activeToolName: null,
                activeThinkingId: null,
              }
            }
            return {
              messages: [
                ...sealed,
                {
                  id: nextId(),
                  type: 'plan_mode_blocked',
                  timestamp: Date.now(),
                  tools: [{ name: toolName, input }],
                  mode: sessionCodingMode,
                  reason: modeBlockedReason,
                },
              ],
              streamingToolArgs: null,
              activeToolUseId: null,
              activeToolName: null,
              activeThinkingId: null,
            }
          })
          break
        }

        let inlinedToCard = false
        if (isUpdatePlanSet || isUpdatePlanUpdate) {
          update((s) => {
            const sealed = sealThinkingForSession(sessionId, s)
            const planIdx = findLatestPlanCardIdx(sealed)
            const curatorIdx = planIdx < 0 ? findLatestCuratorCardIdx(sealed) : -1
            if (planIdx < 0 && curatorIdx < 0) {

              return { messages: sealed, streamingToolArgs: null, activeThinkingId: null }
            }
            if (planIdx >= 0) {
              const cur = sealed[planIdx] as Extract<UIMessage, { type: 'plan_card' }>
              const upgraded = isUpdatePlanSet
                ? applyUpdatePlanSetToCard(cur, (input as Record<string, unknown> | null)?.steps)
                : applyUpdatePlanUpdateToCard(cur, input as Record<string, unknown>)

              if (upgraded === null || upgraded === cur) {
                return { messages: sealed, streamingToolArgs: null, activeThinkingId: null }
              }
              inlinedToCard = true
              const next = [...sealed]
              next[planIdx] = upgraded
              return {
                messages: next,
                streamingToolArgs: null,
                activeToolUseId: null,
                activeToolName: null,
                activeThinkingId: null,
              }
            }
            const curCard = sealed[curatorIdx] as Extract<UIMessage, { type: 'curator_card' }>
            const upgraded = isUpdatePlanSet
              ? applyUpdatePlanSetToCuratorCard(
                  curCard,
                  (input as Record<string, unknown> | null)?.steps,
                )
              : applyUpdatePlanUpdateToCuratorCard(curCard, input as Record<string, unknown>)
            if (upgraded === null || upgraded === curCard) {
              return { messages: sealed, streamingToolArgs: null, activeThinkingId: null }
            }
            inlinedToCard = true
            const next = [...sealed]
            next[curatorIdx] = upgraded
            return {
              messages: next,
              streamingToolArgs: null,
              activeToolUseId: null,
              activeToolName: null,
              activeThinkingId: null,
            }
          })
          if (inlinedToCard) {
            if (toolUseId)
              getOrCreateSessionSet(
                updatePlanInlineToolUseIdsBySession,
                sessionId,
              ).add(toolUseId)
            break
          }
        }
        update((s) => {
          const sealed = sealThinkingForSession(sessionId, s)
          if (isExitPlanMode) {

            const card = makePendingPlanCardFromExitPlanMode(input, toolUseId)
            const draftIdx = findReplaceablePlanCardIdx(sealed)
            if (draftIdx >= 0) {
              const next = [...sealed]
              const previous = sealed[draftIdx] as Extract<UIMessage, { type: 'plan_card' }>
              next[draftIdx] = { ...card, id: previous.id, timestamp: previous.timestamp }
              return {
                messages: next,
                streamingToolArgs: null,
                activeToolUseId: null,
                activeToolName: null,
                activeThinkingId: null,
              }
            }
            return {
              messages: [...sealed, card],
              streamingToolArgs: null,
              activeToolUseId: null,
              activeToolName: null,
              activeThinkingId: null,
            }
          }
          if (isExitCuratorMode) {
            const card = makePendingCuratorCardFromExitCuratorMode(input, toolUseId)
            const draftIdx = findReplaceableCuratorCardIdx(sealed)
            if (draftIdx >= 0) {
              const next = [...sealed]
              const previous = sealed[draftIdx] as Extract<UIMessage, { type: 'curator_card' }>
              next[draftIdx] = { ...card, id: previous.id, timestamp: previous.timestamp }
              return {
                messages: next,
                streamingToolArgs: null,
                activeToolUseId: null,
                activeToolName: null,
                activeThinkingId: null,
              }
            }
            return {
              messages: [...sealed, card],
              streamingToolArgs: null,
              activeToolUseId: null,
              activeToolName: null,
              activeThinkingId: null,
            }
          }
          if (isPlanSave) {

            const card = makePendingPlanCardFromUpdatePlan(input, toolUseId)
            return {
              messages: [...sealed, card],
              streamingToolArgs: null,
              activeToolUseId: null,
              activeToolName: null,
              activeThinkingId: null,
            }
          }
          const spawnsSubagents = isSubagentParentTool(toolName) && !!toolUseId
          return {
            messages: [
              ...sealed,
              {
                id: nextId(),
                type: 'tool_use',
                toolName,
                toolUseId,
                input,
                timestamp: Date.now(),
                parentToolUseId: msg.parentToolUseId,
              },
            ],
            streamingToolArgs: null,
            activeToolUseId: null,
            activeToolName: null,
            activeThinkingId: null,
            ...(spawnsSubagents
              ? {
                  subagentTimelines: {
                    ...s.subagentTimelines,
                    [toolUseId]: {
                      parentToolUseId: toolUseId,
                      parentToolName: toolName,
                      agents: {},
                    },
                  },
                  activeTaskToolUseId: toolUseId,
                }
              : {}),
          }
        })
        if (TODO_TOOL_NAMES.has(toolName) && Array.isArray((input as any)?.todos)) {
          const incomingSessionId = (msg as { sessionId?: string }).sessionId
          if (!incomingSessionId || incomingSessionId === sessionId) {
            useCLITaskStore
              .getState()
              .setTasksFromTodos((input as any).todos, sessionId)
          }
        } else if (TASK_TOOL_NAMES.has(toolName)) {
          const useId = toolUseId
          if (useId)
            getOrCreateSessionSet(pendingTaskToolUseIdsBySession, sessionId).add(useId)
        }
        if (isBrowserFamilyTool(toolName)) {

          const targetUrl = extractBrowserToolUrl(input)
          if (!isDesignerSession(sessionId) || isExternalWebUrl(targetUrl)) {
            void useBrowserPanelStore.getState().openForTool(sessionId, {
              source: 'tool',
              url: targetUrl,
              presentOnly: true,
            })
          }
        }
        break
      }

      case 'plan_progress':
        update((s) => {
          const sealed = sealThinkingForSession(sessionId, s)
          return {
            messages: [
              ...sealed,
              makePlanProgressCard({
                id: nextId(),
                planPath: msg.planPath,
                title: msg.title,
                todos: msg.todos,
                timestamp:
                  typeof msg.timestampMs === 'number' ? msg.timestampMs : Date.now(),
                handoffKind: msg.handoffKind,
              }),
            ],
            chatState: 'thinking',
            planningPhaseStartedAt: Date.now(),
            activeThinkingId: null,
          }
        })
        break

      case 'tool_result':
        if (
          deleteSessionToolUseId(
            planModeBlockedToolUseIdsBySession,
            sessionId,
            msg.toolUseId,
          )
        ) {
          update((s) => ({
            messages: sealThinkingForSession(sessionId, s),
            chatState: s.chatState === 'idle' ? 'idle' : 'thinking',
            planningPhaseStartedAt: Date.now(),
            activeThinkingId: null,
          }))
          break
        }
        if (
          deleteSessionToolUseId(
            updatePlanInlineToolUseIdsBySession,
            sessionId,
            msg.toolUseId,
          )
        ) {
          update((s) => ({
            messages: sealThinkingForSession(sessionId, s),
            chatState: s.chatState === 'idle' ? 'idle' : 'thinking',
            planningPhaseStartedAt: Date.now(),
            activeThinkingId: null,
          }))
          break
        }
        update((s) => {
          const sealed = sealThinkingForSession(sessionId, s)
          const bucket = s.subagentTimelines[msg.toolUseId]
          const subagentPatch: Partial<PerSessionState> = bucket
            ? {
                subagentTimelines: markSubagentBucketStatus(
                  s.subagentTimelines,
                  msg.toolUseId,
                  msg.isError ? 'error' : 'completed',
                  extractToolResultText(msg.content),
                ),
                activeTaskToolUseId:
                  s.activeTaskToolUseId === msg.toolUseId
                    ? null
                    : s.activeTaskToolUseId,
              }
            : {}

          const cardIdx = sealed.findIndex(
            (m) => m.type === 'plan_card' && m.sourceToolUseId === msg.toolUseId,
          )
          if (cardIdx >= 0) {
            const upgraded = upgradePlanCardFromResult(
              sealed[cardIdx] as Extract<UIMessage, { type: 'plan_card' }>,
              msg.content,
              msg.isError,
            )
            const next = [...sealed]
            next[cardIdx] = upgraded
            return {
              messages: next,
              chatState: s.chatState === 'idle' ? 'idle' : 'thinking',
              planningPhaseStartedAt: Date.now(),
              activeThinkingId: null,
              ...subagentPatch,
            }
          }

          const curatorIdx = sealed.findIndex(
            (m) => m.type === 'curator_card' && m.sourceToolUseId === msg.toolUseId,
          )
          if (curatorIdx >= 0) {
            const upgraded = upgradeCuratorCardFromResult(
              sealed[curatorIdx] as Extract<UIMessage, { type: 'curator_card' }>,
              msg.content,
              msg.isError,
            )
            const next = [...sealed]
            next[curatorIdx] = upgraded
            return {
              messages: next,
              chatState: s.chatState === 'idle' ? 'idle' : 'thinking',
              planningPhaseStartedAt: Date.now(),
              activeThinkingId: null,
              ...subagentPatch,
            }
          }
          return {
            messages: [
              ...sealed,
              {
                id: nextId(),
                type: 'tool_result',
                toolUseId: msg.toolUseId,
                content: capToolResultContent(msg.content),
                isError: msg.isError,
                timestamp: Date.now(),
                parentToolUseId: msg.parentToolUseId,
              },
            ],
            chatState: s.chatState === 'idle' ? 'idle' : 'thinking',
            planningPhaseStartedAt: Date.now(),
            activeThinkingId: null,
            ...subagentPatch,
          }
        })
        if (
          deleteSessionToolUseId(
            pendingTaskToolUseIdsBySession,
            sessionId,
            msg.toolUseId,
          )
        ) {
          useCLITaskStore.getState().refreshTasks(sessionId)
        }
        break

      case 'permission_request':
        update((s) => {
          const sealed = sealThinkingForSession(sessionId, s)
          const isQuestionTool = isAskQuestionToolName(msg.toolName)
          return {
            pendingPermission: {
              requestId: msg.requestId,
              toolName: msg.toolName,
              toolUseId: msg.toolUseId,
              input: msg.input,
              description: msg.description,
            },
            chatState: 'permission_pending',
            activeThinkingId: null,
            messages: isQuestionTool
              ? sealed
              : [...sealed, {
                  id: nextId(),
                  type: 'permission_request',
                  requestId: msg.requestId,
                  toolName: msg.toolName,
                  toolUseId: msg.toolUseId,
                  input: msg.input,
                  description: msg.description,
                  timestamp: Date.now(),
                }],
          }
        })
        break

      case 'message_complete': {
        const session = get().sessions[sessionId]
        if (!session) break
        continuationPrefixBySession.delete(sessionId)
        const text = `${session.streamingText}${consumePendingDelta(sessionId)}`
        const turnDelta =
          (msg.usage?.input_tokens ?? 0) +
          (msg.usage?.output_tokens ?? 0) +
          (msg.usage?.cache_read_tokens ?? 0) +
          (msg.usage?.cache_creation_tokens ?? 0)
        update((s) => {
          const sealed = sealThinkingForSession(sessionId, s)
          const flushed = text.trim()
            ? appendAssistantTextMessage(
                sealed,
                text,
                Date.now(),
                undefined,
                echoDedupOptions(sessionId),
              )
            : sealed
          return {
            streamingToolArgs: null,
            messages: s.pendingPermission
              ? flushed
              : resolveDanglingCuratorCards(flushed),
            streamingText: '',
            activeThinkingId: null,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
            tokenUsage: msg.usage,
            cumulativeTokens: (s.cumulativeTokens ?? 0) + Math.max(0, turnDelta),
            chatState: s.pendingPermission ? s.chatState : 'idle',
            planningPhaseStartedAt: null,
            pendingResourceWaits: [],
            providerRetry: null,
          }
        })

        void useUsageStore.getState().fetch()
        syncTasksAfterTurnEnd(sessionId, turnWasStopped)
        revealDesignCanvasIfPending(sessionId)
        break
      }

      case 'provider_retry': {
        const now = Date.now()
        const continuationPrefix = continuationPrefixBySession.get(sessionId)
        update((s) => {
          const merged = flushPendingDeltaIntoStreaming(sessionId, s.streamingText)
          const sealed = sealThinkingForSession(sessionId, s)
          const commitPartial = merged.trim().length > 0 && continuationPrefix === undefined
          const baseMessages = commitPartial
            ? appendAssistantTextMessage(sealed, merged, Date.now(), undefined, echoDedupOptions(sessionId))
            : sealed
          if (commitPartial) {
            dirtyMidTurnSessions.add(sessionId)
          }
          return {
            messages: baseMessages,
            streamingText: continuationPrefix ?? '',
            activeThinkingId: null,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
            providerRetry: {
              attempt: msg.attempt,
              maxAttempts: msg.maxAttempts,
              waitMs: msg.waitMs,
              waitDeadlineAt: now + Math.max(0, msg.waitMs),
              class: msg.class,
              provider: msg.provider,
              model: msg.model,
              message: msg.message,
              receivedAt: now,
            },
          }
        })
        break
      }

      case 'worker_spawned': {
        useWorkersStore.getState().spawnWorker({
          parentSessionId: sessionId,
          parentToolUseId: msg.parentToolUseId,
          workerId: msg.workerId,
          title: msg.title,
          model: msg.model,
        })
        update((s) => {
          const parentId =
            msg.parentToolUseId?.trim() || s.activeTaskToolUseId || null
          if (!parentId) {
            return { chatState: 'awaiting_workers' as ChatState }
          }
          const existing = s.subagentTimelines[parentId]
          return {
            chatState: 'awaiting_workers' as ChatState,
            subagentTimelines: {
              ...s.subagentTimelines,
              [parentId]: existing ?? {
                parentToolUseId: parentId,
                parentToolName: 'spawn_workers',
                agents: {},
              },
            },
          }
        })
        break
      }

      case 'worker_status': {
        useWorkersStore
          .getState()
          .updateStatus(msg.workerId, msg.status, msg.detail ?? null)
        const workerAfterStatus = useWorkersStore.getState().getById(msg.workerId)
        const parentIdStatus = workerAfterStatus?.parentToolUseId?.trim()
        if (parentIdStatus && msg.detail) {
          updateWorkerSubagentTimeline(
            sessionId,
            parentIdStatus,
            msg.workerId,
            'status',
            msg.detail,
          )
        }
        break
      }

      case 'worker_progress': {
        useWorkersStore
          .getState()
          .updateProgress(msg.workerId, msg.action, msg.detail)
        const workerAfterProgress = useWorkersStore.getState().getById(msg.workerId)
        const parentIdProgress = workerAfterProgress?.parentToolUseId?.trim()
        if (parentIdProgress) {
          updateWorkerSubagentTimeline(
            sessionId,
            parentIdProgress,
            msg.workerId,
            msg.action,
            msg.detail,
          )
        }
        break
      }

      case 'worker_completed': {
        useWorkersStore
          .getState()
          .markCompleted(msg.workerId, msg.success, msg.summary)
        if (!useWorkersStore.getState().hasRunningWorkers(sessionId)) {
          update(resumeParentAfterWorkers)
        }
        break
      }

      case 'worker_stopped': {
        useWorkersStore.getState().markStopped(msg.workerId, msg.reason)
        if (!useWorkersStore.getState().hasRunningWorkers(sessionId)) {
          update(resumeParentAfterWorkers)
        }
        break
      }

      case 'parent_resumed': {
        update(resumeParentAfterWorkers)
        break
      }

      case 'error': {
        const isConfigError = isNoModelConfiguredError(msg.message, msg.code)
        const isCancelled = msg.code === 'CANCELLED'
        continuationPrefixBySession.delete(sessionId)
        update((s) => {
          const pendingText = `${s.streamingText}${consumePendingDelta(sessionId)}`
          let newMessages = sealThinkingForSession(sessionId, s)
          if (pendingText.trim()) {
            newMessages = appendAssistantTextMessage(newMessages, pendingText, Date.now(), undefined, echoDedupOptions(sessionId))
          }
          if (!isConfigError && !isCancelled) {
            newMessages = [...newMessages, {
              id: nextId(),
              type: 'error',
              message: msg.message,
              code: msg.code,
              detail: msg.detail,
              timestamp: Date.now(),
            }]
          }
          newMessages = resolveDanglingCuratorCards(newMessages)
          return {
            messages: newMessages,
            chatState: 'idle',
            planningPhaseStartedAt: null,
            activeThinkingId: null,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
            activeToolUseId: null,
            activeToolName: null,
            activeTaskToolUseId: null,
            streamingText: '',
            streamingToolArgs: null,
            pendingPermission: null,
            pendingResourceWaits: [],
            providerRetry: null,
          }
        })
        if (isConfigError || isCancelled) {
          if (isConfigError) emitNoModelWarning(sessionId)
          useTabStore.getState().updateTabStatus(sessionId, 'idle')
        } else {
          useTabStore.getState().updateTabStatus(sessionId, 'error')
        }
        revealDesignCanvasIfPending(sessionId)
        break
      }

      case 'context_compressed': {
        break
      }

      case 'task_update': {

        const status = msg.status
        const terminal =
          status === 'completed' || status === 'failed' || status === 'stopped'
        update((s) => {
          const patch: Partial<PerSessionState> = {}

          if (msg.taskId) {
            const nextTimelines = { ...s.subagentTimelines }
            let mutated = false
            for (const [parentId, bucket] of Object.entries(s.subagentTimelines)) {
              const nextAgents: Record<string, AgentTimeline> = {}
              let bucketMutated = false
              for (const [agentId, tl] of Object.entries(bucket.agents)) {
                if (tl.taskId === msg.taskId) {
                  const mappedStatus: AgentTimeline['status'] =
                    status === 'completed'
                      ? 'completed'
                      : status === 'failed'
                        ? 'error'
                        : status === 'stopped'
                          ? 'completed'
                          : tl.status
                  const progressEntry: AgentTimelineEntry | null = msg.progress
                    ? { kind: 'status', text: msg.progress }
                    : null
                  const nextEntries = progressEntry
                    ? [...tl.entries, progressEntry]
                    : tl.entries
                  nextAgents[agentId] = {
                    ...tl,
                    status: mappedStatus,
                    entries: nextEntries,
                    updatedAt: Date.now(),
                  }
                  bucketMutated = true
                } else {
                  nextAgents[agentId] = tl
                }
              }
              if (bucketMutated) {
                nextTimelines[parentId] = { ...bucket, agents: nextAgents }
                mutated = true
              }
            }
            if (mutated) patch.subagentTimelines = nextTimelines
          }

          if (terminal) {
            const key = msg.taskId
            if (key) {
              patch.agentTaskNotifications = {
                ...s.agentTaskNotifications,
                [key]: {
                  taskId: msg.taskId,
                  toolUseId: s.agentTaskNotifications[key]?.toolUseId ?? '',
                  status: status as 'completed' | 'failed' | 'stopped',
                  summary: msg.progress,
                },
              }
            }
          }
          return patch
        })
        break
      }
      case 'user_message_ack': {
        wsManager.confirmUserMessage(sessionId, msg.clientMsgId)
        update((s) => {
          let changed = false
          const messages = s.messages.map((m) => {
            if (
              m.type === 'user_text' &&
              m.clientMsgId === msg.clientMsgId &&
              m.pending === true
            ) {
              changed = true
              const { pending: _pending, ...rest } = m
              return rest as UIMessage
            }
            return m
          })
          return changed ? { messages } : {}
        })
        break
      }
      case 'session_title_updated':
        useSessionStore.getState().updateSessionTitle(msg.sessionId, msg.title)
        useTabStore.getState().updateTabTitle(msg.sessionId, msg.title)
        break
      case 'session_history_changed': {
        const now = Date.now()
        const last = historyChangedReloadAt.get(sessionId) ?? 0
        if (now - last < 1000) break
        historyChangedReloadAt.set(sessionId, now)
        const st = get().sessions[sessionId]
        if (!st || st.historyLoaded !== true) break
        if (isSessionUiBusy(st.chatState)) {
          dirtyMidTurnSessions.add(sessionId)
          break
        }
        void get().reloadHistory(sessionId)
        break
      }
      case 'todo_snapshot': {
        if (msg.sessionId && msg.sessionId !== sessionId) break
        const todos = Array.isArray(msg.todos) ? msg.todos : []
        const mapped = todos.map((t) => ({
          id: String((t as { id?: unknown }).id ?? ''),
          content: String((t as { content?: unknown }).content ?? ''),
          status: String((t as { status?: unknown }).status ?? 'pending'),
          activeForm: (t as { activeForm?: string }).activeForm,
        }))
        useCLITaskStore.getState().setTasksFromTodos(mapped, sessionId)
        break
      }
      case 'usage_updated': {
        const cost = typeof msg.costUsd === 'number' ? msg.costUsd : 0
        const belongsToThisSession = !!msg.sessionId && msg.sessionId === sessionId
        if (cost > 0 && belongsToThisSession) {
          update((s) => ({
            cumulativeCostUsd: (s.cumulativeCostUsd ?? 0) + cost,
          }))
        }
        import('../stores/usageStore').then(({ useUsageStore }) => {
          void useUsageStore.getState().fetch().catch(() => {})
        }).catch(() => {})
        break
      }
      case 'buddy_event': {
        const event = msg.event
        const greeting = msg.greeting
        const showNotifications = msg.showNotifications
        void import('../stores/buddyStore')
          .then(({ useBuddyStore }) => {
            useBuddyStore.getState().applyEvent(event, greeting, showNotifications)
          })
          .catch(() => {})
        break
      }
      case 'system_notification': {
        if (msg.subtype === 'ws_reconnecting') {
          set((s) => ({
            sessions: updateSessionIn(s.sessions, sessionId, (sess) =>
              sess.connectionState === 'connected' ||
              sess.connectionState === 'connecting'
                ? { connectionState: 'reconnecting' }
                : {},
            ),
          }))
          break
        }
        if (
          msg.subtype === 'ws_handler_error' ||
          msg.subtype === 'ws_frame_gap' ||
          msg.subtype === 'stream_lagged'
        ) {
          const cur = get().sessions[sessionId]
          if (cur && isSessionUiBusy(cur.chatState)) {
            dirtyMidTurnSessions.add(sessionId)
          } else {
            void get().reloadHistory(sessionId)
          }
          break
        }
        if (msg.subtype === 'ws_unreachable') {
          dirtyMidTurnSessions.delete(sessionId)
          continuationPrefixBySession.delete(sessionId)
          update((s) => {
            const merged = flushPendingDeltaIntoStreaming(sessionId, s.streamingText)
            const baseMessages = resolveDanglingCuratorCards(
              merged.trim()
                ? appendAssistantTextMessage(sealThinkingForSession(sessionId, s), merged, Date.now(), undefined, echoDedupOptions(sessionId))
                : sealThinkingForSession(sessionId, s),
            )
            return {
              connectionState: 'disconnected',
              messages: baseMessages,
              streamingText: '',
              chatState: 'idle',
              activeThinkingId: null,
              activeThinkingContent: '',
              activeThinkingStartedAt: null,
              activeThinkingLastChunkAt: null,
              providerRetry: null,
            }
          })
          useTabStore.getState().updateTabStatus(sessionId, 'idle')
          useUIStore.getState().addToast({
            type: 'error',
            message: t('chat.connectionLost'),
            duration: 10000,
            sessionId,
            action: {
              label: t('chat.reconnect'),
              onClick: () => {
                get().connectToSession(sessionId, { force: true })
              },
            },
          })
          break
        }
        if (msg.subtype === 'slash_command_result') {
          const data =
            msg.data && typeof msg.data === 'object'
              ? (msg.data as Record<string, unknown>)
              : null
          const success = data?.success !== false
          const text =
            typeof msg.message === 'string' && msg.message.length > 0
              ? msg.message
              : success
                ? 'Command executed.'
                : 'Command failed.'
          update((s) => ({
            messages: success
              ? appendAssistantTextMessage(s.messages, text, Date.now(), undefined, echoDedupOptions(sessionId))
              : [
                  ...s.messages,
                  {
                    id: nextId(),
                    type: 'error',
                    message: text,
                    code: 'SLASH_COMMAND_FAILED',
                    timestamp: Date.now(),
                  },
                ],
            chatState: 'idle',
            streamingText: '',
            statusVerb: '',
            planningPhaseAction: '',
            planningPhaseDetail: '',
          }))
          useTabStore.getState().updateTabStatus(sessionId, 'idle')
          break
        }
        if (msg.subtype === 'status_detail') {
          const text =
            typeof msg.message === 'string' && msg.message.length > 0 ? msg.message : ''
          if (text) {
            update((s) => ({
              planningPhaseDetail: text,
              planningPhaseAction: s.planningPhaseAction,
            }))
          }
          break
        }
        const level = (msg as { level?: 'info' | 'warning' | 'error' }).level
        if (level === 'warning' || level === 'error') {
          const text =
            typeof msg.message === 'string' && msg.message.length > 0
              ? msg.message
              : `[${msg.subtype}]`
          import('../stores/uiStore').then(({ useUIStore }) => {
            useUIStore.getState().addToast({
              type: level,
              message: text,
              duration: level === 'error' ? 8000 : 5000,
            })
          }).catch(() => {})
        }

        if (
          msg.subtype === 'runtime_config_updated' ||
          msg.subtype === 'runtime_config_apply_failed'
        ) {
          const requestId =
            typeof msg.data === 'object' &&
            msg.data !== null &&
            'requestId' in msg.data &&
            typeof msg.data.requestId === 'string'
              ? msg.data.requestId
              : null
          if (requestId) {
            import('../api/websocket').then(({ wsManager }) => {
              wsManager.notifyRuntimeConfigUpdated(
                sessionId,
                requestId,
                msg.subtype === 'runtime_config_updated',
              )
            }).catch(() => {})
          }
        }

        if (
          msg.subtype === 'coding_mode_confirm_required' &&
          msg.data &&
          typeof msg.data === 'object'
        ) {
          const data = msg.data as Record<string, unknown>
          const mode = typeof data.mode === 'string' ? (data.mode as CodingModeId) : undefined
          const scope = data.scope === 'global' ? 'global' : 'session'
          const from = typeof data.from === 'string' ? data.from : undefined
          const targetSessionId =
            typeof data.sessionId === 'string' && data.sessionId.length > 0
              ? data.sessionId
              : sessionId
          if (mode) {
            set({
              pendingSessionCodingMode: {
                sessionId: targetSessionId,
                mode,
                scope,
                from,
              },
            })
          }
          break
        }

        if (msg.subtype === 'coding_mode_updated' && msg.data && typeof msg.data === 'object') {
          const data = msg.data as Record<string, unknown>
          const mode = typeof data.mode === 'string' ? data.mode : undefined
          const perm = typeof data.permissionMode === 'string' ? data.permissionMode : undefined
          const explicitSessionId =
            typeof data.sessionId === 'string' && data.sessionId.length > 0
              ? data.sessionId
              : undefined
          const scope =
            typeof data.scope === 'string' ? (data.scope as string) : undefined
          const targetSessionId = explicitSessionId ?? sessionId
          if (mode && targetSessionId) {
            set((s) => {
              const nextResolved = { ...s.sessionAutoResolvedMode }
              if (mode !== 'auto') {
                delete nextResolved[targetSessionId]
              }
              return {
                sessionCodingMode: {
                  ...s.sessionCodingMode,
                  [targetSessionId]: mode as CodingModeId,
                },
                sessionAutoResolvedMode: nextResolved,
              }
            })
          }
          if (mode && perm && scope === 'global') {
            import('../stores/settingsStore').then(({ useSettingsStore }) => {
              useSettingsStore
                .getState()
                .applyCodingMode(mode as CodingModeId, perm as PermissionMode)
            }).catch(() => {})
          }
        }
        if (
          msg.subtype === 'coding_mode_auto_resolved' &&
          msg.data &&
          typeof msg.data === 'object'
        ) {
          const data = msg.data as Record<string, unknown>
          const resolved = typeof data.mode === 'string' ? (data.mode as CodingModeId) : undefined
          if (resolved) {
            set((s) => ({
              sessionAutoResolvedMode: {
                ...s.sessionAutoResolvedMode,
                [sessionId]: resolved,
              },
            }))
          }
        }
        if (msg.subtype === 'permission_mode_updated') {
          const data =
            msg.data && typeof msg.data === 'object'
              ? (msg.data as Record<string, unknown>)
              : null
          const explicitSessionId =
            data && typeof data.sessionId === 'string' && data.sessionId.length > 0
              ? (data.sessionId as string)
              : undefined
          const mode =
            data && typeof data.mode === 'string'
              ? (data.mode as string)
              : typeof msg.message === 'string'
                ? msg.message.replace(/^Permission mode: /, '')
                : undefined
          if (mode) {
            import('../stores/settingsStore').then(({ useSettingsStore }) => {
              useSettingsStore.getState().applyPermissionMode(mode as PermissionMode)
            }).catch(() => {})
          } else {
            void explicitSessionId
          }
        }

        if (msg.subtype === 'slash_commands' && Array.isArray(msg.data)) {
          update(() => ({ slashCommands: msg.data as Array<{ name: string; description: string }> }))
        }

        if (msg.subtype === 'resource_wait_started' && msg.data && typeof msg.data === 'object') {
          const data = msg.data as Record<string, unknown>
          const kindRaw = typeof data.kind === 'string' ? data.kind : ''
          const kind: ResourceWaitKind =
            kindRaw === 'file' || kindRaw === 'shell' || kindRaw === 'browser'
              ? (kindRaw as ResourceWaitKind)
              : 'file'
          const target = typeof data.target === 'string' ? data.target : ''
          const holderSessionId =
            typeof data.holderSessionId === 'string' ? data.holderSessionId : ''
          const holderTitle =
            typeof data.holderTitle === 'string' ? data.holderTitle : holderSessionId
          update((session) => {
            const existing = session.pendingResourceWaits ?? []
            const filtered = existing.filter(
              (w) => !(w.kind === kind && w.target === target),
            )
            const next: PendingResourceWait = {
              id: `${kind}:${target}:${Date.now().toString(36)}`,
              kind,
              target,
              holderSessionId,
              holderTitle,
              startedAt: Date.now(),
            }
            return { pendingResourceWaits: [...filtered, next] }
          })
        }

        if (msg.subtype === 'resource_wait_resolved' && msg.data && typeof msg.data === 'object') {
          const data = msg.data as Record<string, unknown>
          const kind = typeof data.kind === 'string' ? data.kind : ''
          const target = typeof data.target === 'string' ? data.target : ''
          update((session) => {
            const existing = session.pendingResourceWaits ?? []
            const filtered = existing.filter(
              (w) => !(w.kind === kind && w.target === target),
            )
            if (filtered.length === existing.length) return session
            return { pendingResourceWaits: filtered }
          })
        }

        if (msg.subtype === 'mcp_servers_updated') {
          import('../stores/mcpStore').then((mod) => {
            const store = mod.useMcpStore?.getState?.()
            if (store && typeof store.fetchServers === 'function') {
              void store.fetchServers()
            }
          }).catch(() => {})
        }

        if (msg.subtype === 'debug_pii_stats' && msg.data && typeof msg.data === 'object') {
          applyDebugPiiStatsDelta(update, msg.data as Record<string, unknown>)
        }

        if (msg.subtype === 'task_notification' && msg.data && typeof msg.data === 'object') {
          const data = msg.data as Record<string, unknown>
          const toolUseId =
            typeof data.tool_use_id === 'string' && data.tool_use_id.trim()
              ? data.tool_use_id
              : null
          const taskStatus = data.status
          if (
            toolUseId &&
            (taskStatus === 'completed' ||
              taskStatus === 'failed' ||
              taskStatus === 'stopped')
          ) {
            update((session) => ({
              agentTaskNotifications: {
                ...session.agentTaskNotifications,
                [toolUseId]: {
                  taskId:
                    typeof data.task_id === 'string' && data.task_id.trim()
                      ? data.task_id
                      : toolUseId,
                  toolUseId,
                  status: taskStatus,
                  summary:
                    typeof data.summary === 'string' && data.summary.trim()
                      ? data.summary
                      : undefined,
                  outputFile:
                    typeof data.output_file === 'string' && data.output_file.trim()
                      ? data.output_file
                      : undefined,
                },
              },
            }))
          }
        }

        if (msg.subtype === 'file_edit' && msg.data && typeof msg.data === 'object') {
          const data = msg.data as Record<string, unknown>
          const path = typeof data.path === 'string' ? data.path : ''
          const additions = typeof data.additions === 'number' ? data.additions : 0
          const deletions = typeof data.deletions === 'number' ? data.deletions : 0
          const editBatchId =
            typeof data.editBatchId === 'string' ? data.editBatchId : null
          const now = Date.now()
          const diffStr = typeof data.diff === 'string' ? data.diff : null
          update((s) => {
            const sealed = sealThinkingForSession(sessionId, s)
            const last = sealed[sealed.length - 1]
            const coalesceTarget =
              last &&
              last.type === 'file_edit' &&
              path !== '' &&
              last.path === path &&
              editBatchId !== null &&
              last.editBatchId === editBatchId
                ? last
                : null
            const messages = coalesceTarget
              ? [
                  ...sealed.slice(0, -1),
                  {
                    ...coalesceTarget,
                    additions: coalesceTarget.additions + additions,
                    deletions: coalesceTarget.deletions + deletions,
                    diff: diffStr ?? coalesceTarget.diff,
                    timestamp: now,
                  },
                ]
              : [
                  ...sealed,
                  {
                    id: nextId(),
                    type: 'file_edit' as const,
                    path,
                    additions,
                    deletions,
                    diff: diffStr,
                    editBatchId,
                    timestamp: now,
                  },
                ]
            return {
              messages,
              pendingEdits: path
                ? mergePendingEdit(s.pendingEdits, {
                    path,
                    additions,
                    deletions,
                    editBatchId,
                    timestamp: now,
                  })
                : s.pendingEdits,
            }
          })
          if (path) {
            void import('./reviewPanelStore').then((m) => {
              m.useReviewPanelStore.getState().notifyFileEdit(sessionId)
            })
          }
        }

        if (msg.subtype === 'command_preview' && msg.data && typeof msg.data === 'object') {
          const data = msg.data as Record<string, unknown>
          update((s) => ({
            messages: [
              ...sealThinkingForSession(sessionId, s),
              {
                id: nextId(),
                type: 'command_preview' as const,
                toolName: typeof data.toolName === 'string' ? data.toolName : 'unknown',
                input: data.input ?? null,
                timestamp: Date.now(),
              },
            ],
          }))
        }

        if (msg.subtype === 'subagent_chunk' && msg.data && typeof msg.data === 'object') {
          const data = msg.data as Record<string, unknown>
          const agentId = typeof data.agentId === 'string' ? data.agentId : 'sub'
          const delta = typeof data.delta === 'string' ? data.delta : ''
          const chunkKind = typeof data.kind === 'string' ? data.kind : 'Chunk'
          const taskId = typeof data.taskId === 'string' ? data.taskId : undefined
          const parentFromFrame =
            typeof data.parentToolUseId === 'string' && data.parentToolUseId.trim()
              ? data.parentToolUseId
              : undefined
          enqueueSubagentChunk(sessionId, {
            agentId,
            chunkKind,
            taskId,
            parentFromFrame,
            delta,
          })
        }

        if (!KNOWN_SYSTEM_NOTIFICATION_SUBTYPES.has(msg.subtype) && level !== 'warning' && level !== 'error') {
          if (import.meta.env?.DEV) {
            console.debug(
              '[chatStore] unhandled system_notification subtype',
              msg.subtype,
            )
          }
        }
        break
      }
      case 'pong':
        break
      case 'debug_pii_stats': {
        applyDebugPiiStatsDelta(update, msg as unknown as Record<string, unknown>)
        break
      }
      case 'debug_submode_set': {
        const m = msg as unknown as {
          submode?: string
          params?: Record<string, unknown>
        }
        if (m.submode) {
          useDebugStore
            .getState()
            .applyServerConfirmed(sessionId, m.submode, m.params ?? {})
        }
        break
      }
      case 'workspace_busy': {
        const requeuedContent = (() => {
          const drained = takeLastDrainedItem(sessionId)
          if (drained) {
            requeueRejectedItem(drained)
            return (
              drained.options?.displayContent?.trim() || drained.content.trim()
            )
          }
          const direct = takeLastDirectSend(sessionId)
          if (direct) {
            useWorkspaceQueueStore
              .getState()
              .enqueue(sessionId, direct.content, direct.attachments, {
                displayContent: direct.displayContent,
                designGeneration: direct.designGeneration,
              })
            return direct.displayContent?.trim() || direct.content.trim()
          }
          return null
        })()
        let fallbackClientMsgId: string | null = null
        if (!requeuedContent) {
          const currentMessages = get().sessions[sessionId]?.messages ?? []
          for (let i = currentMessages.length - 1; i >= 0; i--) {
            const m = currentMessages[i]
            if (m && m.type === 'user_text' && m.pending === true && m.clientMsgId) {
              fallbackClientMsgId = m.clientMsgId
              useWorkspaceQueueStore
                .getState()
                .enqueue(sessionId, m.content, m.attachments as AttachmentRef[] | undefined, {})
              break
            }
          }
        }
        useUIStore.getState().addToast({
          type: 'warning',
          message: t('wsManager.workspaceBusyToast'),
          duration: 4000,
        })
        update((session) => {
          let messages = sealThinkingForSession(sessionId, session)
          const pendingText = `${session.streamingText}${consumePendingDelta(sessionId)}`
          if (pendingText.trim()) {
            messages = appendAssistantTextMessage(
              messages,
              pendingText,
              Date.now(),
              undefined,
              echoDedupOptions(sessionId),
            )
          }
          if (requeuedContent || fallbackClientMsgId) {
            for (let i = messages.length - 1; i >= 0; i--) {
              const m = messages[i]
              if (
                m &&
                m.type === 'user_text' &&
                (fallbackClientMsgId
                  ? m.clientMsgId === fallbackClientMsgId
                  : m.content === requeuedContent)
              ) {
                if (m.clientMsgId) {
                  wsManager.confirmUserMessage(sessionId, m.clientMsgId)
                }
                const next = messages.slice()
                next.splice(i, 1)
                messages = next
                break
              }
            }
          }
          return {
            messages,
            chatState: 'idle',
            stopRequested: false,
            statusVerb: '',
            planningPhaseAction: '',
            planningPhaseDetail: '',
            streamingText: '',
            activeThinkingId: null,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
          }
        })
        break
      }
      case 'lsp_diagnostics':
      case 'lsp_install_progress':
      case 'lsp_server_status':
        useLspStore.getState().handleBroadcastEvent(msg as LspBroadcastEvent)
        break
      case 'persist_lag': {
        useUIStore.getState().addToast({
          type: 'error',
          message: t('chat.persistLag'),
          duration: 8000,
          sessionId,
        })
        break
      }
      default: {
        if (import.meta.env?.DEV) {
          console.debug(
            '[chatStore] unhandled ws message type',
            (msg as { type?: unknown }).type,
          )
        }
        break
      }
    }
  },
}))

type AssistantHistoryBlock = {
  type: string
  text?: string
  thinking?: string

  started_at_ms?: number
  completed_at_ms?: number
  name?: string
  id?: string
  input?: unknown

  path?: string
  additions?: number
  deletions?: number
  diff?: string | null
  edit_batch_id?: string | null

  tool_name?: string

  agent_id?: string
  kind?: string
  delta?: string
  task_id?: string
  parent_tool_use_id?: string

  worker_id?: string
  payload?: unknown

  plan_path?: string
  target_mode?: string
  handoff_kind?: string
  status?: string
  resume?: boolean

  title?: string
  todos?: unknown

  message?: string
  code?: string

  timestamp_ms?: number
}
type UserHistoryBlock = { type: string; text?: string; tool_use_id?: string; content?: unknown; is_error?: boolean; source?: { data?: string }; mimeType?: string; media_type?: string; name?: string }

function coerceHistoryJsonContent(content: unknown): unknown {
  if (typeof content !== 'string') return content
  const s = content.trim()
  if (s.length < 2) return content
  const c0 = s.charCodeAt(0)
  if (c0 !== 91 && c0 !== 123) return content
  try {
    return JSON.parse(s)
  } catch {
    return content
  }
}

function userHistoryBlocksFromContent(content: unknown): UserHistoryBlock[] | null {
  const value = coerceHistoryJsonContent(content)
  if (Array.isArray(value)) return value as UserHistoryBlock[]
  if (value && typeof value === 'object') {
    const obj = value as Record<string, unknown>
    if (typeof obj.type === 'string') return [obj as unknown as UserHistoryBlock]
  }
  return null
}

const assistantBlocksCache = new WeakMap<object, AssistantHistoryBlock[]>()

function assistantBlocksFromMessage(msg: MessageEntry): AssistantHistoryBlock[] {
  if (msg.type !== 'assistant' && msg.type !== 'tool_use') return []
  const cached = assistantBlocksCache.get(msg)
  if (cached) return cached
  const blocks = normalizeAssistantHistoryContent(msg.content) ?? []
  assistantBlocksCache.set(msg, blocks)
  return blocks
}

function thinkingTextFromBlock(block: AssistantHistoryBlock): string {
  if (typeof block.thinking === 'string' && block.thinking.trim()) return block.thinking
  if (block.type === 'thinking' && typeof block.text === 'string' && block.text.trim()) return block.text
  return ''
}

function parseToolCallArguments(raw: unknown): unknown {
  if (raw == null) return {}
  if (typeof raw === 'string') {
    try {
      return JSON.parse(raw)
    } catch {
      return {}
    }
  }
  return raw
}

function reasoningTextFromEnvelope(obj: Record<string, unknown>): string {
  const keys = ['reasoning_content', 'reasoning', 'thinking', 'thinking_content', 'chain_of_thought']
  for (const key of keys) {
    const text = extractTextFromRawContent(obj[key])
    if (text.trim()) return text
  }
  return ''
}

function normalizeAssistantHistoryContent(content: unknown): AssistantHistoryBlock[] | null {
  const value = coerceHistoryJsonContent(content)
  if (Array.isArray(value)) return value as AssistantHistoryBlock[]
  if (!value || typeof value !== 'object') return null
  const obj = value as Record<string, unknown>
  if (typeof obj.type === 'string') return [obj as unknown as AssistantHistoryBlock]
  const blocks: AssistantHistoryBlock[] = []
  const reasoning = reasoningTextFromEnvelope(obj)
  if (reasoning.trim()) {
    blocks.push({ type: 'thinking', thinking: reasoning })
  }
  if (typeof obj.content === 'string' && obj.content.trim()) {
    blocks.push({ type: 'text', text: obj.content })
  } else {
    const nestedText = extractTextFromRawContent(obj.content)
    if (nestedText.trim()) {
      blocks.push({ type: 'text', text: nestedText })
    }
  }
  const calls = Array.isArray(obj.tool_calls) ? obj.tool_calls : []
  for (const call of calls) {
    if (!call || typeof call !== 'object') continue
    const c = call as Record<string, unknown>
    const name = typeof c.name === 'string' ? c.name : ''
    const id = typeof c.id === 'string' ? c.id : ''
    if (!name) continue
    blocks.push({
      type: 'tool_use',
      name,
      id,
      input: parseToolCallArguments(c.input ?? c.arguments),
    })
  }
  return blocks.length > 0 ? blocks : null
}

function isTeammateMessage(text: string): boolean {
  return text.includes('<teammate-message') && text.includes('</teammate-message>')
}

const TEAMMATE_CONTENT_REGEX = /<teammate-message\s+teammate_id="([^"]+)"[^>]*>\n?([\s\S]*?)\n?<\/teammate-message>/g

function extractVisibleTeammateMessageContents(text: string): string[] {
  const contents: string[] = []

  for (const match of text.matchAll(TEAMMATE_CONTENT_REGEX)) {
    const content = match[2]?.trim()
    if (!content) continue

    if (content.startsWith('{') && content.endsWith('}')) {
      try {
        const parsed = JSON.parse(content) as Record<string, unknown>
        if (typeof parsed.type === 'string' && AGENT_LIFECYCLE_TYPES.has(parsed.type)) {
          continue
        }
      } catch {

      }
    }

    contents.push(content)
  }

  return contents
}

function pushAssistantHistoryText(
  messages: UIMessage[],
  content: string,
  timestamp: number,
  model?: string,
  superseded?: boolean,
): void {
  if (!content.trim()) return

  const last = messages[messages.length - 1]
  if (last?.type === 'assistant_text' && !!last.superseded === !!superseded) {
    const merged = mergeAssistantTextContent(last.content, content, true)
    if (merged === null) {
      if (model && !last.model) last.model = model
      return
    }
    last.content = merged
    if (model && !last.model) last.model = model
    return
  }

  messages.push({
    id: nextId(),
    type: 'assistant_text',
    content,
    timestamp,
    ...(model ? { model } : {}),
    ...(superseded ? { superseded: true } : {}),
  })
}

type HistoryMappingOptions = {
  includeTeammateMessages?: boolean
}

export function reconstructAgentNotifications(messages: MessageEntry[]): Record<string, AgentTaskNotification> {

  const agentNameToToolUseId = new Map<string, string>()

  for (const msg of messages) {
    const historyBlocks = assistantBlocksFromMessage(msg)
    if (historyBlocks.length > 0) {
      for (const block of historyBlocks) {
        if (block.type === 'tool_use' && block.name === 'Agent' && block.id) {
          const input = block.input as Record<string, unknown> | undefined
          const name = input?.name as string | undefined

          if (name && !agentNameToToolUseId.has(name)) agentNameToToolUseId.set(name, block.id)
        }
      }
    }
  }

  if (agentNameToToolUseId.size === 0) return {}

  const teammateContent = new Map<string, string>()
  for (const msg of messages) {
    if (msg.type !== 'user') continue
    const userBlocks = userHistoryBlocksFromContent(msg.content)
    const text = userBlocks
      ? userBlocks
          .filter((b) => b.type === 'text' && b.text)
          .map((b) => b.text ?? '')
          .filter((t) => t.length > 0)
          .join('\n')
      : typeof msg.content === 'string'
        ? msg.content
        : extractTextFromRawContent(msg.content)
    if (!text.includes('<teammate-message')) continue
    for (const match of text.matchAll(TEAMMATE_CONTENT_REGEX)) {
      if (match[1] && match[2]) {
        const content = match[2].trim()

        if (content.startsWith('{') && content.endsWith('}')) {
          try {
            const parsed = JSON.parse(content) as Record<string, unknown>
            if (typeof parsed.type === 'string' && AGENT_LIFECYCLE_TYPES.has(parsed.type)) continue
          } catch {  }
        }

        if (!teammateContent.has(match[1])) {
          teammateContent.set(match[1], content)
        }
      }
    }
  }

  const notifications: Record<string, AgentTaskNotification> = {}
  for (const [name, toolUseId] of agentNameToToolUseId) {
    const content = teammateContent.get(name)
    if (content) {
      notifications[toolUseId] = {
        taskId: toolUseId,
        toolUseId,
        status: 'completed',
        summary: content,
      }
    }
  }

  return notifications
}

type PlanProgressTodo = {
  id: string
  content: string
  status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
  notes?: string | null
}

export function normalizePlanProgressTodos(raw: unknown): PlanProgressTodo[] {
  if (!Array.isArray(raw)) return []
  const out: PlanProgressTodo[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const obj = item as Record<string, unknown>
    const content = typeof obj.content === 'string' ? obj.content : ''
    if (!content) continue
    const statusRaw = typeof obj.status === 'string' ? obj.status : 'completed'
    const status: PlanProgressTodo['status'] =
      statusRaw === 'pending' ||
      statusRaw === 'in_progress' ||
      statusRaw === 'completed' ||
      statusRaw === 'cancelled'
        ? statusRaw
        : 'completed'
    out.push({
      id: typeof obj.id === 'string' && obj.id ? obj.id : `s${out.length + 1}`,
      content,
      status,
      notes: typeof obj.notes === 'string' ? obj.notes : null,
    })
  }
  return out
}

function deriveHandoffKindFromPath(planPath: string): 'plan' | 'curator' {
  const norm = (planPath || '').replace(/\\/g, '/')
  return norm.includes('/curators/') || norm.endsWith('impl_blueprint.md')
    ? 'curator'
    : 'plan'
}

export function makePlanProgressCard(args: {
  id: string
  planPath: string
  title: string
  todos: unknown
  timestamp: number
  handoffKind?: 'plan' | 'curator'
}): Extract<UIMessage, { type: 'plan_progress' }> {
  const handoffKind = args.handoffKind ?? deriveHandoffKindFromPath(args.planPath)
  return {
    id: args.id,
    type: 'plan_progress',
    timestamp: args.timestamp,
    planPath: args.planPath,
    title: args.title || (handoffKind === 'curator' ? 'Curator' : 'Plan'),
    todos: normalizePlanProgressTodos(args.todos),
    handoffKind,
  }
}

export function rawIndexFromMessageId(id: string): number | null {
  if (id.startsWith('msg-')) return null
  const base = id.includes(':') ? id.slice(0, id.indexOf(':')) : id
  const dash = base.lastIndexOf('-')
  if (dash < 0 || dash === base.length - 1) return null
  const tail = base.slice(dash + 1)
  if (!/^\d{1,9}$/.test(tail)) return null
  const n = Number.parseInt(tail, 10)
  return Number.isFinite(n) ? n : null
}

export async function mapHistoryMessagesToUiMessages(
  messages: MessageEntry[],
  options?: HistoryMappingOptions,
): Promise<UIMessage[]> {
  const includeTeammateMessages = options?.includeTeammateMessages === true
  const uiMessages: UIMessage[] = []
  let liveUserCount = 0
  let processedEntries = 0

  for (const msg of messages) {
    if (++processedEntries % 60 === 0) await yieldToMainThread()
    const timestamp = new Date(msg.timestamp).getTime()
    const tombstoned = msg.tombstoned === true
    const sup = tombstoned ? { superseded: true } : {}
    if (msg.type === 'user') {
      const coercedUser = coerceHistoryJsonContent(msg.content)
      if (typeof coercedUser === 'string') {
      const userText = typeof msg.content === 'string' ? msg.content : coercedUser
      if (isTeammateMessage(userText) && userText.trimStart().startsWith('<teammate-message')) {
        if (!includeTeammateMessages) continue
        const teammateContents = extractVisibleTeammateMessageContents(userText)
        if (teammateContents.length === 0) continue
        uiMessages.push({
          id: msg.id || nextId(),
          type: 'user_text',
          content: teammateContents.join('\n\n'),
          timestamp,
          ...(tombstoned ? {} : { userMessageIndex: msg.userMessageIndex ?? liveUserCount }),
          ...sup,
        })
        if (!tombstoned) liveUserCount++
        continue
      }
      const parsedAsk = tryParseAskResponseUserText(userText)
      if (parsedAsk) {
        uiMessages.push({
          id: msg.id || nextId(),
          type: 'plan_question_answers',
          timestamp,
          items: parsedAsk.items,
          ...(parsedAsk.details ? { details: parsedAsk.details } : {}),
          ...sup,
        })
        if (!tombstoned) liveUserCount++
        continue
      }
      const displayOverride = msg.displayContent?.trim()
      const legacyBrief = displayOverride
        ? null
        : extractDisplayBriefFromTaskEnvelope(userText)
      const persistedAttachments = mapPersistedAttachments(msg)
      uiMessages.push({
        id: msg.id || nextId(),
        type: 'user_text',
        content:
          displayOverride ||
          (legacyBrief ??
            stripAttachmentMarkersForDisplay(
              userText,
              !!persistedAttachments && persistedAttachments.length > 0,
            )),
        timestamp,
        ...(persistedAttachments ? { attachments: persistedAttachments } : {}),
        ...(msg.clientMsgId ? { clientMsgId: msg.clientMsgId } : {}),
        ...(msg.designRef ? { designRef: msg.designRef } : {}),
        ...(msg.designRefName ? { designRefName: msg.designRefName } : {}),
        ...(msg.designRefElement ? { designRefElement: msg.designRefElement } : {}),
        ...(msg.designRefElementLabel
          ? { designRefElementLabel: msg.designRefElementLabel }
          : {}),
        ...(tombstoned ? {} : { userMessageIndex: msg.userMessageIndex ?? liveUserCount }),
        ...sup,
      })
      if (!tombstoned) liveUserCount++
      continue
      }
    }
    if (msg.type === 'assistant' || msg.type === 'tool_use') {
      const historyBlocks = normalizeAssistantHistoryContent(msg.content)
      if (historyBlocks) {
      let blockSeq = 0
      const blockId = (): string => (msg.id ? `${msg.id}:${blockSeq++}` : nextId())
      for (const block of historyBlocks) {
        const thinkingText = thinkingTextFromBlock(block)
        if (block.type === 'thinking' && thinkingText) {
          const startedAt =
            typeof block.started_at_ms === 'number' && Number.isFinite(block.started_at_ms)
              ? block.started_at_ms
              : timestamp
          const completedAt =
            typeof block.completed_at_ms === 'number' && Number.isFinite(block.completed_at_ms)
              ? block.completed_at_ms
              : startedAt
          uiMessages.push({
            id: blockId(),
            type: 'thinking',
            content: thinkingText,
            timestamp,
            startedAt,
            completedAt,
            ...sup,
          })
        }
        else if (block.type === 'text' && block.text) pushAssistantHistoryText(uiMessages, block.text, timestamp, msg.model, tombstoned)
        else if (block.type === 'tool_use') {
          const blockToolName = block.name ?? 'unknown'
          const blockToolUseId = block.id ?? ''
          const blockInput = block.input
          if (isPlanSaveCall(blockToolName, blockInput)) {
            uiMessages.push({
              ...makePendingPlanCardFromUpdatePlan(blockInput, blockToolUseId),
              id: blockId(),
              timestamp,
              ...sup,
            })
          } else if (isExitPlanModeCall(blockToolName)) {
            uiMessages.push({
              ...makePendingPlanCardFromExitPlanMode(blockInput, blockToolUseId),
              id: blockId(),
              timestamp,
              ...sup,
            })
          } else if (isExitCuratorModeCall(blockToolName)) {
            uiMessages.push({
              ...makePendingCuratorCardFromExitCuratorMode(blockInput, blockToolUseId),
              id: blockId(),
              timestamp,
              ...sup,
            })
          } else if (isUpdatePlanSetCall(blockToolName, blockInput) || isUpdatePlanUpdateCall(blockToolName, blockInput)) {
            const lastPlanIdx = (() => {
              for (let i = uiMessages.length - 1; i >= 0; i--) {
                if (uiMessages[i]!.type === 'plan_card') return i
                const t = uiMessages[i]!.type
                if (t === 'user_text' || t === 'curator_card' || t === 'plan_question_answers') return -1
              }
              return -1
            })()
            if (lastPlanIdx >= 0) {
              const cur = uiMessages[lastPlanIdx] as Extract<UIMessage, { type: 'plan_card' }>
              const upgraded = isUpdatePlanSetCall(blockToolName, blockInput)
                ? applyUpdatePlanSetToCard(cur, (blockInput as Record<string, unknown> | null)?.steps)
                : applyUpdatePlanUpdateToCard(cur, blockInput as Record<string, unknown>)
              if (upgraded && upgraded !== cur) {
                uiMessages[lastPlanIdx] = upgraded
              } else {
                uiMessages[lastPlanIdx] = { ...cur, wasExecuted: true }
              }
            } else {
              uiMessages.push({
                id: blockId(),
                type: 'tool_use',
                toolName: blockToolName,
                toolUseId: blockToolUseId,
                input: blockInput,
                timestamp,
                parentToolUseId: msg.parentToolUseId,
                ...sup,
              })
            }
          } else {
            uiMessages.push({
              id: blockId(),
              type: 'tool_use',
              toolName: blockToolName,
              toolUseId: blockToolUseId,
              input: blockInput,
              timestamp,
              parentToolUseId: msg.parentToolUseId,
              ...sup,
            })
          }
        }
        else if (block.type === 'file_edit' && typeof block.path === 'string') {
          uiMessages.push({
            id: blockId(),
            type: 'file_edit',
            path: block.path,
            additions: typeof block.additions === 'number' ? block.additions : 0,
            deletions: typeof block.deletions === 'number' ? block.deletions : 0,
            diff: typeof block.diff === 'string' ? block.diff : null,
            editBatchId: typeof block.edit_batch_id === 'string' ? block.edit_batch_id : null,
            timestamp,
            ...sup,
          })
        }
        else if (block.type === 'command_preview' && typeof block.tool_name === 'string') {
          uiMessages.push({
            id: blockId(),
            type: 'command_preview',
            toolName: block.tool_name,
            input: block.input ?? null,
            timestamp,
            ...sup,
          })
        }
        else if (block.type === 'subagent_chunk' && typeof block.agent_id === 'string') {
          uiMessages.push({
            id: blockId(),
            type: 'subagent_chunk',
            agentId: block.agent_id,
            delta: typeof block.delta === 'string' ? block.delta : '',
            chunkKind: typeof block.kind === 'string' ? block.kind : 'Chunk',
            taskId: typeof block.task_id === 'string' ? block.task_id : undefined,
            parentToolUseId: typeof block.parent_tool_use_id === 'string' ? block.parent_tool_use_id : undefined,
            timestamp,
            ...sup,
          })
        }
        else if (block.type === 'mode_switch') {
          const planPath = typeof block.plan_path === 'string' ? block.plan_path : ''
          const handoffKindRaw = typeof block.handoff_kind === 'string' ? block.handoff_kind : 'plan'
          const handoffKind: 'plan' | 'curator' =
            handoffKindRaw === 'curator' ? 'curator' : 'plan'
          const targetModeRaw = typeof block.target_mode === 'string' ? block.target_mode : 'agent'
          const statusRaw = typeof block.status === 'string' ? block.status : 'switched'
          const status: 'pending' | 'switched' | 'dismissed' =
            statusRaw === 'pending'
              ? 'pending'
              : statusRaw === 'dismissed'
                ? 'dismissed'
                : 'switched'
          uiMessages.push({
            id: blockId(),
            type: 'mode_switch_card',
            timestamp,
            planPath,
            targetMode: targetModeRaw as CodingModeId,
            status,
            handoffKind,
            ...sup,
          })
        }
        else if (block.type === 'plan_progress') {
          uiMessages.push({
            ...makePlanProgressCard({
              id: blockId(),
              planPath: typeof block.plan_path === 'string' ? block.plan_path : '',
              title: typeof block.title === 'string' ? block.title : 'Plan',
              todos: block.todos,
              timestamp,
            }),
            ...sup,
          })
        }
        else if (block.type === 'error' && typeof block.message === 'string') {
          uiMessages.push({
            id: blockId(),
            type: 'error',
            message: block.message,
            code: typeof block.code === 'string' ? block.code : 'TURN_FAILED',
            timestamp:
              typeof block.timestamp_ms === 'number' && Number.isFinite(block.timestamp_ms)
                ? block.timestamp_ms
                : timestamp,
            ...sup,
          })
        }
      }
      continue
      }
      if (msg.type === 'assistant') {
        const text =
          typeof msg.content === 'string'
            ? msg.content
            : extractTextFromRawContent(msg.content)
        if (text) pushAssistantHistoryText(uiMessages, text, timestamp, msg.model, tombstoned)
      }
      continue
    }
    if (msg.type === 'user' || msg.type === 'tool_result') {
      const mappedUserBlocks = userHistoryBlocksFromContent(msg.content)
      if (!mappedUserBlocks) {
        if (msg.type === 'user') {
          const fallbackText = (() => {
            if (typeof msg.content === 'string') return msg.content
            try {
              return JSON.stringify(msg.content, null, 2)
            } catch {
              return ''
            }
          })()
          if (fallbackText.trim()) {
            uiMessages.push({
              id: msg.id || nextId(),
              type: 'user_text',
              content: fallbackText,
              timestamp,
              ...(msg.clientMsgId ? { clientMsgId: msg.clientMsgId } : {}),
              ...(tombstoned ? {} : { userMessageIndex: msg.userMessageIndex ?? liveUserCount }),
              ...sup,
            })
            if (!tombstoned) liveUserCount++
          }
        }
        continue
      }
      let blockSeq = 0
      const blockId = (): string => (msg.id ? `${msg.id}:${blockSeq++}` : nextId())
      const textParts: string[] = []
      const attachments: UIAttachment[] = []
      for (const block of mappedUserBlocks) {
        if (block.type === 'text' && block.text && isTeammateMessage(block.text)) {
          if (!includeTeammateMessages) continue
          textParts.push(...extractVisibleTeammateMessageContents(block.text))
        } else if (block.type === 'text' && block.text) {
          textParts.push(block.text)
        }
        else if (block.type === 'image') attachments.push({ type: 'image', name: block.name || 'image', data: block.source?.data, mimeType: block.mimeType || block.media_type })
        else if (block.type === 'file') attachments.push({ type: 'file', name: block.name || 'file' })
        else if (block.type === 'tool_result') {
          const toolUseId = block.tool_use_id ?? ''
          let resolvedToolName: string | null = null
          let sourceUseIdx = -1
          for (let k = uiMessages.length - 1; k >= 0; k--) {
            const u = uiMessages[k]
            if (u && u.type === 'tool_use' && u.toolUseId === toolUseId) {
              resolvedToolName = u.toolName
              sourceUseIdx = k
              break
            }
          }
          const sourceToolUseId = sourceUseIdx >= 0 ? toolUseId : null
          const isErrorResult = !!block.is_error
          const resultText =
            typeof block.content === 'string'
              ? block.content
              : extractTextFromRawContent(block.content)

          const planCardIdx = uiMessages.findIndex(
            (m) => m.type === 'plan_card' && m.sourceToolUseId === toolUseId,
          )
          if (planCardIdx >= 0) {
            const cur = uiMessages[planCardIdx] as Extract<UIMessage, { type: 'plan_card' }>
            uiMessages[planCardIdx] = upgradePlanCardFromResult(cur, block.content, isErrorResult)
            continue
          }

          const curatorCardIdx = uiMessages.findIndex(
            (m) => m.type === 'curator_card' && m.sourceToolUseId === toolUseId,
          )
          if (curatorCardIdx >= 0) {
            const cur = uiMessages[curatorCardIdx] as Extract<UIMessage, { type: 'curator_card' }>
            uiMessages[curatorCardIdx] = upgradeCuratorCardFromResult(cur, block.content, isErrorResult)
            continue
          }

          if (
            !isErrorResult &&
            resultText &&
            resultText.includes('===CURATOR_MARKDOWN_BEGIN===') &&
            resolvedToolName === 'exit_curator_mode'
          ) {
            const parsed = parseCuratorEnvelopeForCard(resultText)
            if (parsed) {
              if (sourceUseIdx >= 0) uiMessages.splice(sourceUseIdx, 1)
              uiMessages.push({
                id: blockId(),
                type: 'curator_card',
                timestamp,
                slug: parsed.slug,
                template: parsed.template,
                finalMdPath: parsed.finalMdPath,
                implBlueprintPath: parsed.implBlueprintPath,
                docxPath: parsed.docxPath,
                title: parsed.title,
                body: parsed.body,
                status: 'completed',
                sourceToolUseId: sourceToolUseId ?? toolUseId,
                ...sup,
              })
              continue
            }
          }

          uiMessages.push({
            id: blockId(),
            type: 'tool_result',
            toolUseId,
            content: capToolResultContent(block.content),
            isError: isErrorResult,
            timestamp,
            parentToolUseId: msg.parentToolUseId,
            ...sup,
          })
        }
      }
      if (textParts.length > 0 || attachments.length > 0) {
        const joined = textParts.join('\n')
        const parsedAsk = attachments.length === 0 ? tryParseAskResponseUserText(joined) : null
        if (parsedAsk) {
          uiMessages.push({
            id: blockId(),
            type: 'plan_question_answers',
            timestamp,
            items: parsedAsk.items,
            ...(parsedAsk.details ? { details: parsedAsk.details } : {}),
            ...sup,
          })
          if (msg.type === 'user' && !tombstoned) liveUserCount++
        } else {
          uiMessages.push({
            id: blockId(),
            type: 'user_text',
            content: joined,
            attachments: attachments.length > 0 ? attachments : undefined,
            timestamp,
            ...(msg.clientMsgId ? { clientMsgId: msg.clientMsgId } : {}),
            ...(tombstoned ? {} : { userMessageIndex: msg.userMessageIndex ?? liveUserCount }),
            ...sup,
          })
          if (!tombstoned) liveUserCount++
        }
      } else if (
        msg.type === 'user' &&
        mappedUserBlocks.every(
          (b) =>
            b?.type !== 'tool_result' &&
            !(b?.type === 'text' && typeof b.text === 'string' && isTeammateMessage(b.text)),
        )
      ) {
        const fallbackText = (() => {
          if (typeof msg.content === 'string') return msg.content
          try {
            return JSON.stringify(msg.content, null, 2)
          } catch {
            return ''
          }
        })()
        if (fallbackText.trim()) {
          uiMessages.push({
            id: blockId(),
            type: 'user_text',
            content: fallbackText,
            timestamp,
            ...(msg.clientMsgId ? { clientMsgId: msg.clientMsgId } : {}),
            ...(tombstoned ? {} : { userMessageIndex: msg.userMessageIndex ?? liveUserCount }),
            ...sup,
          })
          if (!tombstoned) liveUserCount++
        }
      }
    }
  }
  return uiMessages
}

function applySupersededFromPendingRewind(
  messages: UIMessage[],
  pendingRewind: PendingRewindSummary | null | undefined,
): UIMessage[] {
  if (!pendingRewind) return messages
  let anchorIdx = -1
  for (let i = 0; i < messages.length; i++) {
    const m = messages[i]!
    if (
      m.type === 'user_text' &&
      typeof m.userMessageIndex === 'number' &&
      m.userMessageIndex === pendingRewind.userMessageIndex
    ) {
      anchorIdx = i
      break
    }
  }
  if (anchorIdx === -1) {
    let minUserIndex: number | null = null
    for (const m of messages) {
      if (m.type === 'user_text' && typeof m.userMessageIndex === 'number') {
        if (minUserIndex === null || m.userMessageIndex < minUserIndex) {
          minUserIndex = m.userMessageIndex
        }
      }
    }
    if (minUserIndex !== null && pendingRewind.userMessageIndex < minUserIndex) {
      anchorIdx = 0
    } else {
      return messages
    }
  }
  return messages.map((m, i) =>
    i >= anchorIdx ? ({ ...(m as UIMessage), superseded: true } as UIMessage) : m,
  )
}

function extractLastTodoWriteFromHistory(messages: MessageEntry[]): Array<{ content: string; status: string; activeForm?: string }> | null {
  let foundIndex = -1
  let todos: Array<{ content: string; status: string; activeForm?: string }> | null = null
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i]!
    const blocks = assistantBlocksFromMessage(msg)
    if (blocks.length > 0) {
      for (let j = blocks.length - 1; j >= 0; j--) {
        const block = blocks[j]!
        if (
          block.type === 'tool_use' &&
          typeof block.name === 'string' &&
          TODO_TOOL_NAMES.has(block.name)
        ) {
          const input = block.input as { todos?: unknown } | undefined
          if (input && Array.isArray(input.todos)) {
            todos = input.todos as Array<{ content: string; status: string; activeForm?: string }>
            foundIndex = i
            break
          }
        }
      }
      if (todos) break
    }
  }
  if (!todos) return null
  const allDone = todos.every((t) => t.status === 'completed')
  if (allDone) {
    for (let i = foundIndex + 1; i < messages.length; i++) {
      if (messages[i]!.type === 'user' && messages[i]!.content) return null
    }
  }
  return todos
}

const TASK_RELATED_TOOL_NAMES = new Set([
  'TodoWrite',
  'TaskCreate',
  'TaskUpdate',
  'TaskGet',
  'TaskList',
  'todo_write',
  'task_create',
  'task_update',
  'task_get',
  'task_list',
])

function hasUserMessagesAfterTaskCompletion(messages: MessageEntry[]): boolean {
  let lastTaskIndex = -1
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i]!
    const blocks = assistantBlocksFromMessage(msg)
    if (blocks.some((b) => b.type === 'tool_use' && TASK_RELATED_TOOL_NAMES.has(b.name ?? ''))) { lastTaskIndex = i; break }
  }
  if (lastTaskIndex < 0) return false
  for (let i = lastTaskIndex + 1; i < messages.length; i++) { if (messages[i]!.type === 'user') return true }
  return false
}
