// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { PerSessionState } from '../stores/chatStore'
import type { SessionListItem } from '../types/session'
import type { CodingModeId } from '../types/codingMode'

export type AgentStatus =
  | 'error'
  | 'waiting'
  | 'waiting_resource'
  | 'tool'
  | 'thinking'
  | 'running'
  | 'queued'
  | 'disconnected'
  | 'missingWorkDir'
  | 'idle'

export type AgentStatusMeta = {
  id: AgentStatus
  glyph: string
  colorVar: string
  i18nKey: string
  pulse: boolean
  priority: number
}

export const AGENT_STATUS_META: Record<AgentStatus, AgentStatusMeta> = {
  error: {
    id: 'error',
    glyph: 'error',
    colorVar: 'var(--color-error)',
    i18nKey: 'agentMonitor.status.error',
    pulse: false,
    priority: 90,
  },
  waiting: {
    id: 'waiting',
    glyph: 'pan_tool',
    colorVar: 'var(--color-warning)',
    i18nKey: 'agentMonitor.status.waiting',
    pulse: true,
    priority: 80,
  },
  waiting_resource: {
    id: 'waiting_resource',
    glyph: 'hourglass_top',
    colorVar: 'var(--color-warning)',
    i18nKey: 'agentMonitor.status.waitingResource',
    pulse: true,
    priority: 75,
  },
  tool: {
    id: 'tool',
    glyph: 'build',
    colorVar: 'var(--color-tertiary)',
    i18nKey: 'agentMonitor.status.tool',
    pulse: true,
    priority: 70,
  },
  thinking: {
    id: 'thinking',
    glyph: 'psychology',
    colorVar: 'var(--color-brand)',
    i18nKey: 'agentMonitor.status.thinking',
    pulse: true,
    priority: 60,
  },
  running: {
    id: 'running',
    glyph: 'play_circle',
    colorVar: 'var(--color-success)',
    i18nKey: 'agentMonitor.status.running',
    pulse: true,
    priority: 50,
  },
  queued: {
    id: 'queued',
    glyph: 'schedule',
    colorVar: 'var(--color-secondary)',
    i18nKey: 'agentMonitor.status.queued',
    pulse: false,
    priority: 40,
  },
  disconnected: {
    id: 'disconnected',
    glyph: 'cloud_off',
    colorVar: 'var(--color-text-tertiary)',
    i18nKey: 'agentMonitor.status.disconnected',
    pulse: false,
    priority: 30,
  },
  missingWorkDir: {
    id: 'missingWorkDir',
    glyph: 'folder_off',
    colorVar: 'var(--color-warning)',
    i18nKey: 'agentMonitor.status.missingWorkDir',
    pulse: false,
    priority: 20,
  },
  idle: {
    id: 'idle',
    glyph: 'radio_button_unchecked',
    colorVar: 'var(--color-text-tertiary)',
    i18nKey: 'agentMonitor.status.idle',
    pulse: false,
    priority: 10,
  },
}

export type AgentStatusContext = {
  session: SessionListItem
  isRunning: boolean
  chatSession?: PerSessionState | null
  queueLen: number
  codingMode?: CodingModeId | null
  resourceProfile?: ResourceProfileInfo | null
}

export type ResourceProfileInfo = {
  browser: boolean
  shell: boolean
  mayWrite: boolean
}

export type AgentSnapshot = {
  sessionId: string
  title: string
  workDir: string | null
  workDirExists: boolean
  status: AgentStatus
  isRunning: boolean
  isAttached: boolean
  queueLen: number
  chatState: PerSessionState['chatState'] | null
  connectionState: PerSessionState['connectionState'] | null
  toolName: string | null
  modifiedAt: string
  hasMissingWorkDir: boolean
  resourceWaitCount: number
  firstResourceWait: {
    kind: 'file' | 'shell' | 'browser'
    target: string
    holderTitle: string
    holderSessionId: string
  } | null
  codingMode: CodingModeId | null
  resourceProfile: ResourceProfileInfo | null
  elapsedSeconds: number
  statusVerb: string
  cumulativeTokens: number
  inputTokens: number
  outputTokens: number
  pendingEditCount: number
  lastEditPath: string | null
}

function hasRecentError(chat: PerSessionState | null | undefined): boolean {
  if (!chat) return false
  const list = chat.messages
  if (!list || list.length === 0) return false
  for (let i = list.length - 1; i >= 0; i -= 1) {
    const msg = list[i]!
    if (msg.superseded) continue
    if (msg.type === 'error') return true
    if (msg.type === 'user_text' || msg.type === 'assistant_text') return false
    if (i < list.length - 10) return false
  }
  return false
}

export function deriveAgentStatus(ctx: AgentStatusContext): AgentStatus {
  const { session, isRunning, chatSession, queueLen } = ctx

  if (hasRecentError(chatSession)) return 'error'

  if (chatSession?.pendingPermission) {
    return 'waiting'
  }
  if (chatSession?.chatState === 'permission_pending') return 'waiting'

  if (chatSession?.pendingResourceWaits && chatSession.pendingResourceWaits.length > 0) {
    return 'waiting_resource'
  }

  if (chatSession?.chatState === 'tool_executing') return 'tool'
  if (chatSession?.chatState === 'thinking' || chatSession?.chatState === 'streaming') {
    return 'thinking'
  }

  if (isRunning) return 'running'

  if (queueLen > 0) return 'queued'

  const workDirMissing = !session.workDirExists && Boolean((session.workDir ?? '').trim())
  if (workDirMissing) return 'missingWorkDir'

  if (chatSession && chatSession.connectionState === 'disconnected') return 'disconnected'

  return 'idle'
}

export function buildAgentSnapshot(
  ctx: AgentStatusContext,
  options?: { isAttached?: boolean },
): AgentSnapshot {
  const status = deriveAgentStatus(ctx)
  const { session, isRunning, chatSession, queueLen, codingMode, resourceProfile } = ctx
  const pendingEdits = chatSession?.pendingEdits ?? []
  const lastEdit = pendingEdits.length > 0 ? pendingEdits[pendingEdits.length - 1] : null
  const tokenUsage = chatSession?.tokenUsage
  return {
    sessionId: session.id,
    title: session.title,
    workDir: session.workDir ?? null,
    workDirExists: session.workDirExists,
    status,
    isRunning,
    isAttached: Boolean(options?.isAttached),
    queueLen,
    chatState: chatSession?.chatState ?? null,
    connectionState: chatSession?.connectionState ?? null,
    toolName: chatSession?.activeToolName ?? null,
    modifiedAt: session.modifiedAt,
    hasMissingWorkDir:
      !session.workDirExists && Boolean((session.workDir ?? '').trim()),
    resourceWaitCount: chatSession?.pendingResourceWaits?.length ?? 0,
    firstResourceWait: (() => {
      const first = chatSession?.pendingResourceWaits?.[0]
      if (!first) return null
      return {
        kind: first.kind,
        target: first.target,
        holderTitle: first.holderTitle,
        holderSessionId: first.holderSessionId,
      }
    })(),
    codingMode: codingMode ?? null,
    resourceProfile: resourceProfile ?? null,
    elapsedSeconds: chatSession?.elapsedSeconds ?? 0,
    statusVerb: chatSession?.statusVerb ?? '',
    cumulativeTokens: chatSession?.cumulativeTokens ?? 0,
    inputTokens: tokenUsage?.input_tokens ?? 0,
    outputTokens: tokenUsage?.output_tokens ?? 0,
    pendingEditCount: pendingEdits.length,
    lastEditPath: lastEdit?.path ?? null,
  }
}

export type AgentStatusSummary = {
  total: number
  active: number
  running: number
  thinking: number
  tool: number
  waiting: number
  waitingResource: number
  queued: number
  error: number
  disconnected: number
  idle: number
  missingWorkDir: number
}

export function summarizeAgents(snapshots: AgentSnapshot[]): AgentStatusSummary {
  const summary: AgentStatusSummary = {
    total: snapshots.length,
    active: 0,
    running: 0,
    thinking: 0,
    tool: 0,
    waiting: 0,
    waitingResource: 0,
    queued: 0,
    error: 0,
    disconnected: 0,
    idle: 0,
    missingWorkDir: 0,
  }
  for (const snap of snapshots) {
    switch (snap.status) {
      case 'running':
        summary.running += 1
        summary.active += 1
        break
      case 'thinking':
        summary.thinking += 1
        summary.active += 1
        break
      case 'tool':
        summary.tool += 1
        summary.active += 1
        break
      case 'waiting':
        summary.waiting += 1
        summary.active += 1
        break
      case 'waiting_resource':
        summary.waitingResource += 1
        summary.active += 1
        break
      case 'queued':
        summary.queued += 1
        break
      case 'error':
        summary.error += 1
        break
      case 'disconnected':
        summary.disconnected += 1
        break
      case 'idle':
        summary.idle += 1
        break
      case 'missingWorkDir':
        summary.missingWorkDir += 1
        break
    }
  }
  return summary
}

export function compareAgentSnapshots(a: AgentSnapshot, b: AgentSnapshot): number {
  const ap = AGENT_STATUS_META[a.status].priority
  const bp = AGENT_STATUS_META[b.status].priority
  if (ap !== bp) return bp - ap
  const at = new Date(a.modifiedAt).getTime()
  const bt = new Date(b.modifiedAt).getTime()
  return bt - at
}
