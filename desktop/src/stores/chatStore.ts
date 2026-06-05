// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { wsManager } from '../api/websocket'
import { sessionsApi } from '../api/sessions'
import { useTeamStore } from './teamStore'
import { useSessionStore } from './sessionStore'
import { useWorkspaceQueueStore } from './workspaceQueueStore'
import { useSessionRunStateStore } from './sessionRunStateStore'
import { useCLITaskStore } from './cliTaskStore'
import { useSessionRuntimeStore } from './sessionRuntimeStore'
import { useSettingsStore } from './settingsStore'
import { useTabStore } from './tabStore'
import { useUsageStore } from './usageStore'
import { useWorkersStore } from './workersStore'
import { useBrowserPanelStore } from './browserPanelStore'
import { useLspStore } from './lspStore'
import { useUIStore } from './uiStore'
import { t } from '../i18n'
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
  elapsedSeconds: number
  statusVerb: string
  slashCommands: Array<{ name: string; description: string }>
  agentTaskNotifications: Record<string, AgentTaskNotification>
  elapsedTimer: ReturnType<typeof setInterval> | null
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

  subagentTimelines: Record<string, SubagentTimelineBucket>

  activeTaskToolUseId: string | null

  stopRequested: boolean

  debugPiiStats: DebugPiiStats

  pendingResourceWaits: PendingResourceWait[]

  providerRetry: ProviderRetryNotice | null

  historyLoaded?: boolean
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
  activeToolUseId: null,
  activeToolName: null,
  activeThinkingId: null,
  activeThinkingContent: '',
  activeThinkingStartedAt: null,
  activeThinkingLastChunkAt: null,
  pendingPermission: null,
  tokenUsage: { input_tokens: 0, output_tokens: 0 },
  cumulativeTokens: 0,
  elapsedSeconds: 0,
  statusVerb: '',
  slashCommands: [],
  agentTaskNotifications: {},
  elapsedTimer: null,
  composerPrefill: null,
  pendingRewind: null,
  pendingSendAfterRewind: null,
  pendingEdits: [],
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
    pendingRewind: null,
    pendingSendAfterRewind: null,
    pendingEdits: [],
    subagentTimelines: {},
    activeTaskToolUseId: null,
    pendingResourceWaits: [],
  }
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
  pendingSessionCodingMode: PendingSessionCodingMode | null

  getSession: (sessionId: string) => PerSessionState
  connectToSession: (sessionId: string) => void
  connectToWorker: (workerId: string) => void
  disconnectSession: (sessionId: string) => void
  suspendSession: (sessionId: string) => void
  sendMessage: (
    sessionId: string,
    content: string,
    attachments?: AttachmentRef[],
    options?: { displayContent?: string; __internalDrain?: boolean },
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
  dismissAgentTaskNotification: (sessionId: string, toolUseId: string) => void
  stopGeneration: (sessionId: string) => void
  cancelTool: (sessionId: string, toolUseId?: string) => void
  reconcileStuckSession: (sessionId: string) => void
  loadHistory: (sessionId: string) => Promise<void>
  reloadHistory: (sessionId: string) => Promise<void>
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

  undoAllPendingEdits: (sessionId: string) => Promise<void>

  resumePlanExecution: (sessionId: string, planPath: string) => void

  resumeCuratorExecution: (sessionId: string, implBlueprintPath: string) => void

  resetDebugPiiStats: (sessionId: string) => void
}

export const ASK_QUESTION_TOOL_NAMES = new Set(['ask_question', 'AskUserQuestion'])
export function isAskQuestionToolName(name: string | undefined | null): boolean {
  if (!name) return false
  return ASK_QUESTION_TOOL_NAMES.has(name) || name.toLowerCase() === 'ask_question'
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
  const markdown = card.markdown
    ? rewritePlanMarkdownTodos(card.markdown, todos)
    : card.markdown
  return {
    ...card,
    todos,
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
    if (messages[i]!.type === 'curator_card') return i
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
    return { ...card, status: 'writing' }
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
  if (isError) {
    return { ...card, status: 'writing' }
  }
  const text =
    typeof rawContent === 'string' ? rawContent : extractTextFromRawContent(rawContent)
  const parsed = parseCuratorEnvelopeForCard(text)
  if (!parsed) {
    return { ...card, status: 'completed' }
  }
  return {
    ...card,
    status: 'completed',
    slug: parsed.slug || card.slug,
    template: parsed.template || card.template,
    finalMdPath: parsed.finalMdPath || card.finalMdPath,
    implBlueprintPath: parsed.implBlueprintPath || card.implBlueprintPath,
    docxPath: parsed.docxPath ?? card.docxPath,
    title: parsed.title || card.title,
    body: parsed.body || card.body,
  }
}

function findReplaceableCuratorCardIdx(messages: UIMessage[]): number {
  for (let i = messages.length - 1; i >= 0; i--) {
    const m = messages[i]!
    if (m.type === 'curator_card') {
      if (m.status === 'writing') return i
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

function syncTasksAfterTurnEnd(sessionId: string) {
  void useCLITaskStore
    .getState()
    .refreshTasks(sessionId)
    .finally(() => {
      useCLITaskStore.getState().finalizeTasksOnTurnEnd(sessionId)
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

function updateWorkerSubagentTimeline(
  update: (
    fn: (s: PerSessionState) => Partial<PerSessionState> | Record<string, never>,
  ) => void,
  parentToolUseId: string,
  workerId: string,
  action: string,
  detail: string,
) {
  const now = Date.now()
  const delta = detail.trim() ? `${action}: ${detail}` : action
  update((s) => {
    const bucket = s.subagentTimelines[parentToolUseId] ?? {
      parentToolUseId,
      parentToolName: 'spawn_workers',
      agents: {},
    }
    const prevTimeline: AgentTimeline = bucket.agents[workerId] ?? {
      agentId: workerId,
      status: 'running',
      entries: [],
      startedAt: now,
      updatedAt: now,
    }
    const entry = subagentChunkToEntry('Status', delta)
    const nextTimeline = appendTimelineEntry(prevTimeline, entry, now)
    return {
      subagentTimelines: {
        ...s.subagentTimelines,
        [parentToolUseId]: {
          ...bucket,
          agents: {
            ...bucket.agents,
            [workerId]: nextTimeline,
          },
        },
      },
    }
  })
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
      text: `${last.text}${entry.text}`,
    }
    return {
      ...timeline,
      entries: [...entries.slice(0, -1), merged],
      updatedAt: now,
    }
  }
  return {
    ...timeline,
    entries: [...entries, entry],
    updatedAt: now,
  }
}

function extractToolResultText(content: unknown): string | undefined {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    return content
      .map((chunk) => {
        if (typeof chunk === 'string') return chunk
        if (chunk && typeof chunk === 'object' && 'text' in chunk) {
          const t = (chunk as { text?: unknown }).text
          return typeof t === 'string' ? t : ''
        }
        return ''
      })
      .filter(Boolean)
      .join('\n') || undefined
  }
  if (content && typeof content === 'object') {
    try {
      return JSON.stringify(content)
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
  'runtime_config_updated',
  'runtime_config_persist_failed',
  'coding_mode_confirm_required',
  'coding_mode_updated',
  'permission_mode_updated',
  'pii_config_updated',
  'slash_commands',
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

function emitNoModelWarning(): void {
  return
}

const pendingDeltaBySession = new Map<string, string>()
const flushTimerBySession = new Map<string, number>()
const pendingThinkingBySession = new Map<string, string>()
const thinkingFlushTimerBySession = new Map<string, number>()
const pendingDeltaFirstAt = new Map<string, number>()
const pendingThinkingFirstAt = new Map<string, number>()
const FLUSH_HIGH_WATER_CHARS = 96
const FLUSH_HIGH_WATER_MS = 80

function nowMs(): number {
  if (typeof performance !== 'undefined' && performance && typeof performance.now === 'function') {
    return performance.now()
  }
  return Date.now()
}

function scheduleRafCallback(cb: () => void): number {
  if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
    return window.requestAnimationFrame(cb)
  }
  return setTimeout(cb, 16) as unknown as number
}

function cancelScheduledFlush(id: number): void {
  if (typeof window !== 'undefined' && typeof window.cancelAnimationFrame === 'function') {
    window.cancelAnimationFrame(id)
  }
  clearTimeout(id as unknown as ReturnType<typeof setTimeout>)
}

function consumePendingDelta(sessionId: string): string {
  const timer = flushTimerBySession.get(sessionId)
  if (timer !== undefined) {
    cancelScheduledFlush(timer)
    flushTimerBySession.delete(sessionId)
  }
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
  if (!buffered) {
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
  pendingDeltaBySession.delete(sessionId)
  pendingDeltaFirstAt.delete(sessionId)
  const thinkingTimer = thinkingFlushTimerBySession.get(sessionId)
  if (thinkingTimer !== undefined) {
    cancelScheduledFlush(thinkingTimer)
    thinkingFlushTimerBySession.delete(sessionId)
  }
  pendingThinkingBySession.delete(sessionId)
  pendingThinkingFirstAt.delete(sessionId)
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
  if (!activeThinkingId) return messages
  const content = options?.content ?? ''
  const startedAt = options?.startedAt ?? null
  if (content) {
    return commitActiveThinking(messages, activeThinkingId, content, startedAt, true)
  }
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
  if (!activeThinkingId || !content) {
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

function appendAssistantTextMessage(
  messages: UIMessage[],
  content: string,
  timestamp: number,
  model?: string,
): UIMessage[] {
  if (!content.trim()) return messages

  const last = messages[messages.length - 1]
  if (last?.type === 'assistant_text') {
    const prevContent = last.content
    if (prevContent === content) {
      return messages
    }
    if (prevContent.endsWith(content) && content.length > 0) {
      return messages
    }
    let mergedContent: string
    if (content.startsWith(prevContent) && prevContent.length > 0) {
      mergedContent = content
    } else {
      mergedContent = prevContent + content
    }
    const merged: UIMessage = {
      ...last,
      content: mergedContent,
      ...(model ?? last.model ? { model: model ?? last.model } : {}),
    }
    return [...messages.slice(0, -1), merged]
  }

  return [
    ...messages,
    {
      id: nextId(),
      type: 'assistant_text',
      content,
      timestamp,
      ...(model ? { model } : {}),
    },
  ]
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

async function fetchAndMapSessionHistory(sessionId: string) {
  const { messages, pendingRewind } = await sessionsApi.getMessages(sessionId)
  return {
    rawMessages: messages,
    uiMessages: mapHistoryMessagesToUiMessages(messages),
    restoredNotifications: reconstructAgentNotifications(messages),
    lastTodos: extractLastTodoWriteFromHistory(messages),
    hasMessagesAfterTaskCompletion: hasUserMessagesAfterTaskCompletion(messages),
    restoredSubagentTimelines: reconstructSubagentTimelines(messages),
    pendingRewind: pendingRewind ?? null,
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
    if ((msg.type === 'assistant' || msg.type === 'tool_use') && Array.isArray(msg.content)) {
      for (const rawBlock of msg.content as AssistantHistoryBlock[]) {
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
    if ((msg.type === 'user' || msg.type === 'tool_result') && Array.isArray(msg.content)) {
      for (const rawBlock of msg.content as UserHistoryBlock[]) {
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
  pendingSessionCodingMode: null,

  getSession: (sessionId) => get().sessions[sessionId] ?? createDefaultSessionState(),

  connectToSession: (sessionId) => {
    void useCLITaskStore.getState().fetchSessionTasks(sessionId)

    const existing = get().sessions[sessionId]
    if (existing && existing.connectionState !== 'disconnected') {
      void useWorkersStore.getState().fetchByParent(sessionId)
      return
    }

    clearSessionStreamBuffers(sessionId)

    set((s) => ({
      sessions: {
        ...s.sessions,
        [sessionId]: {
          ...createDefaultSessionState(),
          connectionState: 'connecting',
          messages: existing?.messages ?? [],
          historyLoaded: existing?.historyLoaded === true,
          pendingPermission: existing?.pendingPermission ?? null,
        },
      },
    }))

    wsManager.clearHandlers(sessionId)
    wsManager.connect(sessionId)
    useSessionStore.getState().recordBrowseSessionWorkDir(sessionId)
    wsManager.onMessage(sessionId, (msg) => {
      if (msg.type === 'connected') {
        set((s) => ({
          sessions: updateSessionIn(s.sessions, sessionId, () => ({
            connectionState: 'connected',
            pendingResourceWaits: [],
          })),
        }))
      }
      get().handleServerMessage(sessionId, msg)
    })

    const runtimeSelection = useSessionRuntimeStore.getState().selections[sessionId]
    void ensureSessionRuntimeSynced(sessionId, { persist: false }).catch(() => {
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
    wsManager.connect(workerId, { pathPrefix: '/ws/worker' })
    wsManager.onMessage(workerId, (msg) => {
      if (msg.type === 'connected') {
        set((s) => ({
          sessions: updateSessionIn(s.sessions, workerId, () => ({
            connectionState: 'connected',
          })),
        }))
      }
      get().handleServerMessage(workerId, msg)
    })

    get().loadHistory(workerId)
  },

  disconnectSession: (sessionId) => {
    const session = get().sessions[sessionId]
    if (session?.elapsedTimer) clearInterval(session.elapsedTimer)
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
    wsManager.disconnect(sessionId)
    set((s) => {
      const { [sessionId]: _, ...rest } = s.sessions
      return { sessions: rest }
    })
  },

  suspendSession: (sessionId) => {
    const session = get().sessions[sessionId]
    if (!session) return
    if (session.elapsedTimer) clearInterval(session.elapsedTimer)
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
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, () => ({
        messages: sealedMessages,
        connectionState: 'disconnected',
        chatState: 'idle',
        activeThinkingId: null,
        activeThinkingContent: '',
        activeThinkingStartedAt: null,
        activeThinkingLastChunkAt: null,
        elapsedTimer: null,
        statusVerb: '',
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
        emitNoModelWarning()
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

    if (!isMemberSession && !options?.__internalDrain) {
      const queueState = useWorkspaceQueueStore.getState()
      const sameSessionRunning = useSessionRunStateStore
        .getState()
        .running.has(sessionId)
      const ownQueueLen = queueState.getQueueForSession(sessionId).length
      if (sameSessionRunning || ownQueueLen > 0) {
        const passthroughOptions = options
          ? { displayContent: options.displayContent }
          : undefined
        queueState.enqueue(sessionId, content, attachments, passthroughOptions)
        return
      }
    }
    const uiAttachments: UIAttachment[] | undefined =
      attachments && attachments.length > 0
        ? attachments.map((a) => ({
            type: a.type,
            name: a.name || a.path || a.mimeType || a.type,
            data: a.data,
            mimeType: a.mimeType,
          }))
        : undefined

    const taskStore = useCLITaskStore.getState()
    const sessionTasks = taskStore.tasksBySessionId[sessionId] ?? []
    const allTasksDone = sessionTasks.length > 0 && sessionTasks.every((t) => t.status === 'completed')
    const completedTaskSummary = allTasksDone
      ? sessionTasks.map((t) => ({ id: t.id, subject: t.subject, status: t.status, activeForm: t.activeForm }))
      : []

    if (!isMemberSession && allTasksDone) {
      void taskStore.resetCompletedTasks(sessionId)
    }

    set((s) => {
      const session = s.sessions[sessionId] ?? createDefaultSessionState()
      const bufferedDelta = consumePendingDelta(sessionId)
      const pendingAssistantText = `${session.streamingText}${bufferedDelta}`

      const newMessages = pendingAssistantText.trim()
        ? appendAssistantTextMessage(session.messages, pendingAssistantText, Date.now())
        : [...session.messages]
      if (!isMemberSession && allTasksDone) {
        newMessages.push({
          id: nextId(),
          type: 'task_summary',
          tasks: completedTaskSummary,
          timestamp: Date.now(),
        })
      }
      newMessages.push({
        id: nextId(),
        type: 'user_text',
        content: userFacingContent,
        attachments: isMemberSession ? undefined : uiAttachments,
        timestamp: Date.now(),
        ...(isMemberSession ? { pending: true } : {}),
      })

      if (!isMemberSession && session.elapsedTimer) clearInterval(session.elapsedTimer)

      const timer = !isMemberSession
        ? setInterval(() => {
            set((st) => ({ sessions: updateSessionIn(st.sessions, sessionId, (sess) => ({ elapsedSeconds: sess.elapsedSeconds + 1 })) }))
          }, 1000)
        : null

      return {
        sessions: {
          ...s.sessions,
          [sessionId]: {
            ...session,
            messages: newMessages,
            chatState: 'thinking',
            stopRequested: false,
            elapsedSeconds: 0,
            streamingText: '',
            statusVerb: isMemberSession ? '' : randomSpinnerVerb(),
            elapsedTimer: timer,
            connectionState: isMemberSession ? 'connected' : session.connectionState,
          },
        },
      }
    })

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

    queueSessionRuntimeSync(sessionId, { persist: false })
    wsManager.send(sessionId, { type: 'user_message', content, attachments })
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
            get().connectToSession(sessionId)
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
          chatState: allowed ? 'tool_executing' : 'idle',
          messages,
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
      session.chatState === 'streaming'
    if (!isStuckActive) {
      if (session.stopRequested) {
        set((s) => ({
          sessions: updateSessionIn(s.sessions, sessionId, () => ({
            stopRequested: false,
          })),
        }))
      }
      return
    }
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, () => ({
        stopRequested: false,
      })),
    }))
    void get().reloadHistory(sessionId)
  },

  stopGeneration: (sessionId) => {
    wsManager.send(sessionId, { type: 'stop_generation' })
    if (hasPendingDelta(sessionId)) {
      const text = consumePendingDelta(sessionId)
      set((s) => ({ sessions: updateSessionIn(s.sessions, sessionId, (sess) => ({ streamingText: sess.streamingText + text })) }))
    } else {
      consumePendingDelta(sessionId)
    }
    set((s) => {
      const session = s.sessions[sessionId]
      if (!session) return s
      if (session.elapsedTimer) clearInterval(session.elapsedTimer)
      const sealedMessages = sealThinkingForSession(sessionId, session)
      const partialText = session.streamingText
      const committedMessages = partialText.trim()
        ? appendAssistantTextMessage(sealedMessages, partialText, Date.now())
        : sealedMessages
      return {
        sessions: {
          ...s.sessions,
          [sessionId]: {
            ...session,
            messages: committedMessages,
            streamingText: '',
            chatState: 'idle',
            stopRequested: true,
            activeThinkingId: null,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
            elapsedTimer: null,
            pendingResourceWaits: [],
            providerRetry: null,
          },
        },
      }
    })
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
      } = await fetchAndMapSessionHistory(sessionId)
      const taggedMessages = applySupersededFromPendingRewind(uiMessages, pendingRewind)
      set((state) => {
        const session = state.sessions[sessionId]
        if (!session) return state
        if (session.historyLoaded === true) return state
        const liveMessages = session.messages
        const knownIds = new Set<string>()
        for (const m of taggedMessages) {
          if (m && m.id) knownIds.add(m.id)
        }
        const liveOnly = liveMessages.filter((m) => !knownIds.has(m.id))
        const merged: UIMessage[] =
          liveOnly.length === 0 ? taggedMessages : [...taggedMessages, ...liveOnly]
        return {
          sessions: updateSessionIn(state.sessions, sessionId, (s) => ({
            messages: merged,
            agentTaskNotifications: {
              ...s.agentTaskNotifications,
              ...restoredNotifications,
            },
            subagentTimelines: {
              ...s.subagentTimelines,
              ...restoredSubagentTimelines,
            },
            pendingRewind,
            historyLoaded: true,
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
    } catch {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('chat.loadHistoryFailed'),
        duration: 5000,
      })
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
      } = await fetchAndMapSessionHistory(sessionId)
      const taggedMessages = applySupersededFromPendingRewind(uiMessages, pendingRewind)

      set((state) => {
        const session = state.sessions[sessionId]
        if (!session) return state
        if (session.elapsedTimer) clearInterval(session.elapsedTimer)
        return {
          sessions: updateSessionIn(state.sessions, sessionId, () => ({
            messages: taggedMessages,
            agentTaskNotifications: restoredNotifications,
            subagentTimelines: restoredSubagentTimelines,
            chatState: 'idle',
            activeThinkingId: null,
            activeToolUseId: null,
            activeToolName: null,
            streamingText: '',
            pendingPermission: null,
            elapsedTimer: null,
            statusVerb: '',
            pendingRewind,
            historyLoaded: true,
          })),
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
    } catch {
      useUIStore.getState().addToast({
        type: 'error',
        message: t('chat.loadHistoryFailed'),
        duration: 5000,
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
        subagentTimelines: {},
        activeTaskToolUseId: null,
        messages: sess.messages.filter((m) => !m.superseded),
      })),
    }))
    get().sendMessage(sessionId, pending.content, pending.attachments, pending.options)
  },

  cancelSendAfterRewind: (sessionId) => {
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, () => ({
        pendingSendAfterRewind: null,
      })),
    }))
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
      sessions: updateSessionIn(s.sessions, sessionId, () => ({ pendingEdits: [] })),
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
        })),
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
        })),
      }
    })
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
    set((s) => ({
      sessions: updateSessionIn(s.sessions, sessionId, () => ({ pendingEdits: [] })),
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
          msg.type === 'task_update' ||
          msg.type === 'lsp_diagnostics' ||
          msg.type === 'permission_request' ||
          msg.type === 'system_notification'
        if (!passThrough) {
          return
        }
        if (isStatusIdle || msg.type === 'message_complete' || msg.type === 'error') {
          set((s) => ({
            sessions: updateSessionIn(s.sessions, sessionId, () => ({
              stopRequested: false,
            })),
          }))
        }
      }
    }

    switch (msg.type) {
      case 'connected':

        void hydrateCumulativeTokensFromUsage(sessionId)
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
        break

      case 'status':
        update((session) => {
          const pendingText = `${session.streamingText}${consumePendingDelta(sessionId)}`
          const hasPendingStreamText =
            session.chatState === 'streaming' && pendingText.trim().length > 0

          const preserveStreamingTurn = hasPendingStreamText && msg.state !== 'idle'
          const shouldFlush = hasPendingStreamText && msg.state === 'idle'
          const baseMessages =
            msg.state === 'idle'
              ? sealThinkingForSession(sessionId, session)
              : session.messages
          return {
            chatState: preserveStreamingTurn ? 'streaming' : msg.state,
            ...(msg.verb && msg.verb !== 'Thinking' ? { statusVerb: msg.verb } : {}),
            ...(msg.tokens ? { tokenUsage: { ...session.tokenUsage, output_tokens: msg.tokens } } : {}),
            ...(msg.state === 'idle' ? {
              activeThinkingId: null,
              activeThinkingContent: '',
              activeThinkingStartedAt: null,
              activeThinkingLastChunkAt: null,
              statusVerb: '',
            } : {}),
            ...(shouldFlush ? {
              messages: appendAssistantTextMessage(baseMessages, pendingText, Date.now()),
              streamingText: '',
            } : msg.state === 'idle' && baseMessages !== session.messages ? {
              messages: baseMessages,
              ...(pendingText !== session.streamingText ? { streamingText: pendingText } : {}),
            } : pendingText !== session.streamingText ? { streamingText: pendingText } : {}),
          }
        })
        if (msg.state === 'idle') {
          const session = get().sessions[sessionId]
          if (session?.elapsedTimer) {
            clearInterval(session.elapsedTimer)
            update(() => ({ elapsedTimer: null }))
          }
          syncTasksAfterTurnEnd(sessionId)
        }

        useTabStore.getState().updateTabStatus(sessionId, msg.state === 'idle' ? 'idle' : 'running')
        break

      case 'content_start': {
        const session = get().sessions[sessionId]
        if (!session) break
        const pendingText = `${session.streamingText}${consumePendingDelta(sessionId)}`
        if (msg.blockType !== 'text' && pendingText.trim()) {
          update((s) => ({
            messages: appendAssistantTextMessage(s.messages, pendingText, Date.now()),
            streamingText: '',
          }))
        }
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
          }))
        } else if (msg.blockType === 'tool_use') {
          update((s) => {
            return {
              messages: sealThinkingForSession(sessionId, s),
              activeToolUseId: msg.toolUseId ?? null,
              activeToolName: msg.toolName ?? null,
              chatState: 'tool_executing',
              activeThinkingId: null,
              activeThinkingContent: '',
              activeThinkingStartedAt: null,
              activeThinkingLastChunkAt: null,
              providerRetry: null,
            }
          })

          if (msg.toolName && isBrowserFamilyTool(msg.toolName)) {
            void useBrowserPanelStore.getState().openForTool(sessionId, {
              source: 'tool',
              url: null,
              presentOnly: true,
            })
          }
        }
        break
      }

      case 'content_delta':
        if (msg.text !== undefined) {
          const prev = pendingDeltaBySession.get(sessionId) ?? ''
          const next = prev + msg.text
          pendingDeltaBySession.set(sessionId, next)
          if (!pendingDeltaFirstAt.has(sessionId)) {
            pendingDeltaFirstAt.set(sessionId, nowMs())
          }
          const firstAt = pendingDeltaFirstAt.get(sessionId) ?? nowMs()
          const elapsed = nowMs() - firstAt

          const flushDelta = () => {
            flushTimerBySession.delete(sessionId)
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

      case 'content_reset': {
        update((s) => {
          const merged = flushPendingDeltaIntoStreaming(sessionId, s.streamingText)
          const baseMessages = merged.trim()
            ? appendAssistantTextMessage(s.messages, merged, Date.now())
            : s.messages
          const patch = mergePendingThinkingIntoActive(s, sessionId)
          return {
            messages: baseMessages,
            streamingText: '',
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
          const prev = pendingThinkingBySession.get(sessionId) ?? ''
          const next = prev + msg.text
          pendingThinkingBySession.set(sessionId, next)
          if (!pendingThinkingFirstAt.has(sessionId)) {
            pendingThinkingFirstAt.set(sessionId, nowMs())
          }
          const firstAt = pendingThinkingFirstAt.get(sessionId) ?? nowMs()
          const elapsed = nowMs() - firstAt

          const flushThinking = () => {
            thinkingFlushTimerBySession.delete(sessionId)
            const buffered = pendingThinkingBySession.get(sessionId) ?? ''
            pendingThinkingBySession.delete(sessionId)
            pendingThinkingFirstAt.delete(sessionId)
            if (!buffered) return
            const THINKING_IDLE_THRESHOLD_MS = 5000
            update((s) => {
              const pendingText = `${s.streamingText}${consumePendingDelta(sessionId)}`
              const baseMessages = pendingText.trim()
                ? appendAssistantTextMessage(s.messages, pendingText, Date.now())
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
              return {
                messages: baseMessages,
                chatState: 'thinking',
                activeThinkingId: id,
                activeThinkingContent: nextContent,
                activeThinkingStartedAt: startedAt,
                activeThinkingLastChunkAt: now,
                ...(pendingText !== s.streamingText ? { streamingText: '' } : {}),
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

      case 'tool_use_complete': {
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

              return { messages: sealed, activeThinkingId: null }
            }
            if (planIdx >= 0) {
              const cur = sealed[planIdx] as Extract<UIMessage, { type: 'plan_card' }>
              const upgraded = isUpdatePlanSet
                ? applyUpdatePlanSetToCard(cur, (input as Record<string, unknown> | null)?.steps)
                : applyUpdatePlanUpdateToCard(cur, input as Record<string, unknown>)

              if (upgraded === null || upgraded === cur) {
                return { messages: sealed, activeThinkingId: null }
              }
              inlinedToCard = true
              const next = [...sealed]
              next[planIdx] = upgraded
              return {
                messages: next,
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
              return { messages: sealed, activeThinkingId: null }
            }
            inlinedToCard = true
            const next = [...sealed]
            next[curatorIdx] = upgraded
            return {
              messages: next,
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
                activeToolUseId: null,
                activeToolName: null,
                activeThinkingId: null,
              }
            }
            return {
              messages: [...sealed, card],
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
                activeToolUseId: null,
                activeToolName: null,
                activeThinkingId: null,
              }
            }
            return {
              messages: [...sealed, card],
              activeToolUseId: null,
              activeToolName: null,
              activeThinkingId: null,
            }
          }
          if (isPlanSave) {

            const card = makePendingPlanCardFromUpdatePlan(input, toolUseId)
            return {
              messages: [...sealed, card],
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
                type: 'tool_use',
                toolName,
                toolUseId,
                input,
                timestamp: Date.now(),
                parentToolUseId: msg.parentToolUseId,
              },
            ],
            activeToolUseId: null,
            activeToolName: null,
            activeThinkingId: null,
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
          void useBrowserPanelStore.getState().openForTool(sessionId, {
            source: 'tool',
            url: targetUrl,
            presentOnly: true,
          })
        }
        if (isSubagentParentTool(toolName) && toolUseId) {

          update((s) => ({
            subagentTimelines: {
              ...s.subagentTimelines,
              [toolUseId]: {
                parentToolUseId: toolUseId,
                parentToolName: toolName,
                agents: {},
              },
            },
            activeTaskToolUseId: toolUseId,
          }))
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
            chatState: 'thinking',
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
            chatState: 'thinking',
            activeThinkingId: null,
          }))
          break
        }
        update((s) => {
          const sealed = sealThinkingForSession(sessionId, s)

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
              chatState: 'thinking',
              activeThinkingId: null,
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
            const relocated: UIMessage = {
              ...upgraded,
              timestamp: Date.now(),
            }
            const next = [
              ...sealed.slice(0, curatorIdx),
              ...sealed.slice(curatorIdx + 1),
              relocated,
            ]
            return {
              messages: next,
              chatState: 'thinking',
              activeThinkingId: null,
            }
          }
          return {
            messages: [
              ...sealed,
              {
                id: nextId(),
                type: 'tool_result',
                toolUseId: msg.toolUseId,
                content: msg.content,
                isError: msg.isError,
                timestamp: Date.now(),
                parentToolUseId: msg.parentToolUseId,
              },
            ],
            chatState: 'thinking',
            activeThinkingId: null,
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

        update((s) => {
          const bucket = s.subagentTimelines[msg.toolUseId]
          if (!bucket) return {}
          const finalText = extractToolResultText(msg.content)
          const nextTimelines = markSubagentBucketStatus(
            s.subagentTimelines,
            msg.toolUseId,
            msg.isError ? 'error' : 'completed',
            finalText,
          )
          const nextActive =
            s.activeTaskToolUseId === msg.toolUseId
              ? null
              : s.activeTaskToolUseId
          return {
            subagentTimelines: nextTimelines,
            activeTaskToolUseId: nextActive,
          }
        })
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
        const text = `${session.streamingText}${consumePendingDelta(sessionId)}`
        if (text.trim()) {
          update((s) => ({
            messages: appendAssistantTextMessage(
              sealThinkingForSession(sessionId, s),
              text,
              Date.now(),
            ),
            streamingText: '',
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
          }))
        } else if (text !== session.streamingText) {
          update((s) => ({
            messages: sealThinkingForSession(sessionId, s),
            streamingText: text,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
          }))
        } else {
          update((s) => ({
            messages: sealThinkingForSession(sessionId, s),
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
          }))
        }
        if (session.elapsedTimer) clearInterval(session.elapsedTimer)

        const turnDelta =
          (msg.usage?.input_tokens ?? 0) +
          (msg.usage?.output_tokens ?? 0) +
          (msg.usage?.cache_read_tokens ?? 0) +
          (msg.usage?.cache_creation_tokens ?? 0)
        update((s) => ({
          tokenUsage: msg.usage,
          cumulativeTokens: (s.cumulativeTokens ?? 0) + Math.max(0, turnDelta),
          chatState: s.pendingPermission ? s.chatState : 'idle',
          activeThinkingId: null,
          elapsedTimer: null,
          pendingResourceWaits: [],
          providerRetry: null,
        }))

        void useUsageStore.getState().fetch()
        syncTasksAfterTurnEnd(sessionId)
        break
      }

      case 'provider_retry': {
        const now = Date.now()
        update((s) => {
          const merged = flushPendingDeltaIntoStreaming(sessionId, s.streamingText)
          const baseMessages = merged.trim()
            ? appendAssistantTextMessage(s.messages, merged, Date.now())
            : s.messages
          return {
            messages: baseMessages,
            streamingText: '',
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
            update,
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
            update,
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
          update((s) => ({
            chatState: s.chatState === 'awaiting_workers' ? 'thinking' : s.chatState,
          }))
        }
        break
      }

      case 'worker_stopped': {
        useWorkersStore.getState().markStopped(msg.workerId, msg.reason)
        if (!useWorkersStore.getState().hasRunningWorkers(sessionId)) {
          update((s) => ({
            chatState: s.chatState === 'awaiting_workers' ? 'thinking' : s.chatState,
          }))
        }
        break
      }

      case 'parent_resumed': {
        update((s) => ({
          chatState: s.chatState === 'awaiting_workers' ? 'thinking' : s.chatState,
        }))
        break
      }

      case 'error': {
        const isConfigError = isNoModelConfiguredError(msg.message, msg.code)
        update((s) => {
          const pendingText = `${s.streamingText}${consumePendingDelta(sessionId)}`
          let newMessages = sealThinkingForSession(sessionId, s)
          if (pendingText.trim()) {
            newMessages = appendAssistantTextMessage(newMessages, pendingText, Date.now())
          }
          if (!isConfigError) {
            newMessages = [...newMessages, {
              id: nextId(),
              type: 'error',
              message: msg.message,
              code: msg.code,
              detail: msg.detail,
              timestamp: Date.now(),
            }]
          }
          return {
            messages: newMessages,
            chatState: 'idle',
            activeThinkingId: null,
            activeThinkingContent: '',
            activeThinkingStartedAt: null,
            activeThinkingLastChunkAt: null,
            streamingText: '',
            pendingPermission: null,
            pendingResourceWaits: [],
            providerRetry: null,
          }
        })
        if (isConfigError) {
          emitNoModelWarning()
          useTabStore.getState().updateTabStatus(sessionId, 'idle')
        } else {
          useTabStore.getState().updateTabStatus(sessionId, 'error')
        }
        {
          const session = get().sessions[sessionId]
          if (session?.elapsedTimer) {
            clearInterval(session.elapsedTimer)
            update(() => ({ elapsedTimer: null }))
          }
        }
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
      case 'session_title_updated':
        useSessionStore.getState().updateSessionTitle(msg.sessionId, msg.title)
        useTabStore.getState().updateTabTitle(msg.sessionId, msg.title)
        break
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
        if (msg.subtype === 'ws_unreachable') {
          const stuckSession = get().sessions[sessionId]
          if (stuckSession?.elapsedTimer) clearInterval(stuckSession.elapsedTimer)
          update((s) => {
            const merged = flushPendingDeltaIntoStreaming(sessionId, s.streamingText)
            const baseMessages = merged.trim()
              ? appendAssistantTextMessage(sealThinkingForSession(sessionId, s), merged, Date.now())
              : sealThinkingForSession(sessionId, s)
            return {
              connectionState: 'disconnected',
              messages: baseMessages,
              streamingText: '',
              chatState: 'idle',
              activeThinkingId: null,
              activeThinkingContent: '',
              activeThinkingStartedAt: null,
              activeThinkingLastChunkAt: null,
              elapsedTimer: null,
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
                get().connectToSession(sessionId)
              },
            },
          })
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

        if (msg.subtype === 'runtime_config_updated') {
          import('../api/websocket').then(({ wsManager }) => {
            wsManager.notifyRuntimeConfigUpdated(sessionId)
          }).catch(() => {})
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
            set((s) => ({
              sessionCodingMode: {
                ...s.sessionCodingMode,
                [targetSessionId]: mode as CodingModeId,
              },
            }))
          }
          if (mode && perm && scope === 'global') {
            import('../stores/settingsStore').then(({ useSettingsStore }) => {
              useSettingsStore
                .getState()
                .applyCodingMode(mode as CodingModeId, perm as PermissionMode)
            }).catch(() => {})
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
          const scope =
            data && typeof data.scope === 'string' ? (data.scope as string) : undefined
          const mode =
            data && typeof data.mode === 'string'
              ? (data.mode as string)
              : typeof msg.message === 'string'
                ? msg.message.replace(/^Permission mode: /, '')
                : undefined
          if (mode && scope === 'global') {
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
          update((s) => ({
            messages: [
              ...sealThinkingForSession(sessionId, s),
              {
                id: nextId(),
                type: 'file_edit' as const,
                path,
                additions,
                deletions,
                diff: typeof data.diff === 'string' ? data.diff : null,
                editBatchId,
                timestamp: now,
              },
            ],
            pendingEdits: path
              ? mergePendingEdit(s.pendingEdits, {
                  path,
                  additions,
                  deletions,
                  editBatchId,
                  timestamp: now,
                })
              : s.pendingEdits,
          }))
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
          const now = Date.now()
          update((s) => {
            const parentId = parentFromFrame ?? s.activeTaskToolUseId
            const bucketExists = parentId
              ? Boolean(s.subagentTimelines[parentId])
              : false

            const flatMessage: UIMessage = {
              id: nextId(),
              type: 'subagent_chunk' as const,
              agentId,
              delta,
              chunkKind,
              taskId,
              parentToolUseId: parentId ?? undefined,
              timestamp: now,
            }

            let nextTimelines = s.subagentTimelines
            if (parentId && bucketExists) {
              const bucket = s.subagentTimelines[parentId]!
              const prevTimeline: AgentTimeline = bucket.agents[agentId] ?? {
                agentId,
                taskId,
                status: 'running',
                entries: [],
                startedAt: now,
                updatedAt: now,
              }
              const entry = subagentChunkToEntry(chunkKind, delta)
              const nextTimeline = appendTimelineEntry(prevTimeline, entry, now)
              nextTimelines = {
                ...s.subagentTimelines,
                [parentId]: {
                  ...bucket,
                  agents: {
                    ...bucket.agents,
                    [agentId]: { ...nextTimeline, taskId: taskId ?? prevTimeline.taskId },
                  },
                },
              }
            }
            return {
              messages: [
                ...sealThinkingForSession(sessionId, s),
                flatMessage,
              ],
              subagentTimelines: nextTimelines,
            }
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
      case 'workspace_busy': {
        useUIStore.getState().addToast({
          type: 'warning',
          message: t('wsManager.workspaceBusyToast'),
          duration: 4000,
        })
        update(() => ({
          chatState: 'idle',
          stopRequested: false,
          statusVerb: '',
        }))
        break
      }
      case 'lsp_diagnostics':
      case 'lsp_install_progress':
      case 'lsp_server_status':
        useLspStore.getState().handleBroadcastEvent(msg as LspBroadcastEvent)
        break
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

  timestamp_ms?: number
}
type UserHistoryBlock = { type: string; text?: string; tool_use_id?: string; content?: unknown; is_error?: boolean; source?: { data?: string }; mimeType?: string; media_type?: string; name?: string }

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

    last.content += content
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
    if ((msg.type === 'assistant' || msg.type === 'tool_use') && Array.isArray(msg.content)) {
      for (const block of msg.content as AssistantHistoryBlock[]) {
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
    const text = typeof msg.content === 'string'
      ? msg.content
      : Array.isArray(msg.content)
        ? (msg.content as Array<{ type?: string; text?: string }>).filter((b) => b.type === 'text' && b.text).map((b) => b.text).join('\n')
        : ''
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

export function mapHistoryMessagesToUiMessages(
  messages: MessageEntry[],
  options?: HistoryMappingOptions,
): UIMessage[] {
  const includeTeammateMessages = options?.includeTeammateMessages === true
  const uiMessages: UIMessage[] = []
  let liveUserCount = 0

  for (const msg of messages) {
    const timestamp = new Date(msg.timestamp).getTime()
    const tombstoned = msg.tombstoned === true
    const sup = tombstoned ? { superseded: true } : {}
    if (msg.type === 'user' && typeof msg.content === 'string') {
      if (isTeammateMessage(msg.content)) {
        if (!includeTeammateMessages) continue
        const teammateContents = extractVisibleTeammateMessageContents(msg.content)
        if (teammateContents.length === 0) continue
        uiMessages.push({
          id: msg.id || nextId(),
          type: 'user_text',
          content: teammateContents.join('\n\n'),
          timestamp,
          ...(tombstoned ? {} : { userMessageIndex: liveUserCount }),
          ...sup,
        })
        if (!tombstoned) liveUserCount++
        continue
      }
      const parsedAsk = tryParseAskResponseUserText(msg.content)
      if (parsedAsk) {
        uiMessages.push({
          id: msg.id || nextId(),
          type: 'plan_question_answers',
          timestamp,
          items: parsedAsk.items,
          ...(parsedAsk.details ? { details: parsedAsk.details } : {}),
          ...sup,
        })
        continue
      }
      uiMessages.push({
        id: msg.id || nextId(),
        type: 'user_text',
        content: msg.content,
        timestamp,
        ...(tombstoned ? {} : { userMessageIndex: liveUserCount }),
        ...sup,
      })
      if (!tombstoned) liveUserCount++
      continue
    }
    if (msg.type === 'assistant' && typeof msg.content === 'string') {
      uiMessages.push({ id: msg.id || nextId(), type: 'assistant_text', content: msg.content, timestamp, model: msg.model, ...sup })
      continue
    }
    if ((msg.type === 'assistant' || msg.type === 'tool_use') && Array.isArray(msg.content)) {
      for (const block of msg.content as AssistantHistoryBlock[]) {
        if (block.type === 'thinking' && block.thinking) {
          const startedAt =
            typeof block.started_at_ms === 'number' && Number.isFinite(block.started_at_ms)
              ? block.started_at_ms
              : undefined
          const completedAt =
            typeof block.completed_at_ms === 'number' && Number.isFinite(block.completed_at_ms)
              ? block.completed_at_ms
              : undefined
          uiMessages.push({
            id: nextId(),
            type: 'thinking',
            content: block.thinking,
            timestamp,
            ...(startedAt !== undefined ? { startedAt } : {}),
            ...(completedAt !== undefined ? { completedAt } : {}),
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
              timestamp,
              ...sup,
            })
          } else if (isExitPlanModeCall(blockToolName)) {
            uiMessages.push({
              ...makePendingPlanCardFromExitPlanMode(blockInput, blockToolUseId),
              timestamp,
              ...sup,
            })
          } else if (isExitCuratorModeCall(blockToolName)) {
            uiMessages.push({
              ...makePendingCuratorCardFromExitCuratorMode(blockInput, blockToolUseId),
              timestamp,
              ...sup,
            })
          } else if (isUpdatePlanSetCall(blockToolName, blockInput) || isUpdatePlanUpdateCall(blockToolName, blockInput)) {
            const lastPlanIdx = (() => {
              for (let i = uiMessages.length - 1; i >= 0; i--) {
                if (uiMessages[i]!.type === 'plan_card') return i
                const t = uiMessages[i]!.type
                if (t === 'user_text' || t === 'mode_switch_card' || t === 'curator_card' || t === 'plan_question_answers') return -1
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
                id: nextId(),
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
              id: nextId(),
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
            id: nextId(),
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
            id: nextId(),
            type: 'command_preview',
            toolName: block.tool_name,
            input: block.input ?? null,
            timestamp,
            ...sup,
          })
        }
        else if (block.type === 'subagent_chunk' && typeof block.agent_id === 'string') {
          uiMessages.push({
            id: nextId(),
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
            id: nextId(),
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
              id: nextId(),
              planPath: typeof block.plan_path === 'string' ? block.plan_path : '',
              title: typeof block.title === 'string' ? block.title : 'Plan',
              todos: block.todos,
              timestamp,
            }),
            ...sup,
          })
        }
      }
      continue
    }
    if ((msg.type === 'user' || msg.type === 'tool_result') && Array.isArray(msg.content)) {
      const textParts: string[] = []
      const attachments: UIAttachment[] = []
      for (const block of msg.content as UserHistoryBlock[]) {
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
                id: nextId(),
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
            id: nextId(),
            type: 'tool_result',
            toolUseId,
            content: block.content,
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
            id: nextId(),
            type: 'plan_question_answers',
            timestamp,
            items: parsedAsk.items,
            ...(parsedAsk.details ? { details: parsedAsk.details } : {}),
            ...sup,
          })
        } else {
          uiMessages.push({
            id: nextId(),
            type: 'user_text',
            content: joined,
            attachments: attachments.length > 0 ? attachments : undefined,
            timestamp,
            ...(tombstoned ? {} : { userMessageIndex: liveUserCount }),
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
  let userCount = 0
  for (let i = 0; i < messages.length; i++) {
    const m = messages[i]!
    if (m.type === 'user_text') {
      if (userCount === pendingRewind.userMessageIndex) {
        anchorIdx = i
        break
      }
      userCount++
    }
  }
  if (anchorIdx === -1) return messages
  return messages.map((m, i) =>
    i >= anchorIdx ? ({ ...(m as UIMessage), superseded: true } as UIMessage) : m,
  )
}

function extractLastTodoWriteFromHistory(messages: MessageEntry[]): Array<{ content: string; status: string; activeForm?: string }> | null {
  let foundIndex = -1
  let todos: Array<{ content: string; status: string; activeForm?: string }> | null = null
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i]!
    if ((msg.type === 'assistant' || msg.type === 'tool_use') && Array.isArray(msg.content)) {
      const blocks = msg.content as AssistantHistoryBlock[]
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
    if ((msg.type === 'assistant' || msg.type === 'tool_use') && Array.isArray(msg.content)) {
      const blocks = msg.content as AssistantHistoryBlock[]
      if (blocks.some((b) => b.type === 'tool_use' && TASK_RELATED_TOOL_NAMES.has(b.name ?? ''))) { lastTaskIndex = i; break }
    }
  }
  if (lastTaskIndex < 0) return false
  for (let i = lastTaskIndex + 1; i < messages.length; i++) { if (messages[i]!.type === 'user') return true }
  return false
}
