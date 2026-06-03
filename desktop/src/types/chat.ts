// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { PermissionMode } from './settings'
import type { CodingModeId } from './codingMode'
import type { RuntimeSelection } from './runtime'

export type ClientMessage =
  | { type: 'prewarm_session' }
  | { type: 'user_message'; content: string; attachments?: AttachmentRef[] }
  | {
      type: 'permission_response'
      requestId: string
      allowed: boolean
      rule?: string
      updatedInput?: Record<string, unknown>
    }
  | {
      type: 'computer_use_permission_response'
      requestId: string
      response: ComputerUsePermissionResponse
    }
  | { type: 'set_permission_mode'; mode: PermissionMode }
  | { type: 'set_coding_mode'; mode: CodingModeId; scope?: 'session' | 'global' }
  | ({ type: 'set_runtime_config'; persist?: boolean } & RuntimeSelection)
  | { type: 'set_pii_config'; data: { enabled: boolean; disabledKinds: string[] } }
  | { type: 'stop_generation' }
  | { type: 'cancel_tool'; sessionId?: string; toolUseId?: string }

  | {
      type: 'start_plan_execution'
      planPath: string
      resume?: boolean
      kind?: 'plan' | 'curator'
    }
  | { type: 'debug_bind_tab'; tab_id: number }
  | { type: 'debug_unbind_tab'; tab_id: number }
  | { type: 'debug_bind_prototype_ref'; tab_id: number }
  | { type: 'debug_unbind_prototype_ref' }
  | { type: 'ping' }

export type AttachmentRef = {
  type: 'file' | 'image'
  name?: string
  path?: string
  data?: string
  mimeType?: string
}

export type UIAttachment = {
  type: 'file' | 'image'
  name: string
  data?: string
  mimeType?: string
}

export type ServerMessage =
  | { type: 'connected'; sessionId: string }
  | { type: 'content_start'; blockType: 'text' | 'tool_use'; toolName?: string; toolUseId?: string; parentToolUseId?: string }
  | { type: 'content_delta'; text?: string }
  | { type: 'content_reset' }
  | { type: 'tool_use_complete'; toolName: string; toolUseId: string; input: unknown; parentToolUseId?: string; sessionId?: string }
  | { type: 'tool_result'; toolUseId: string; content: unknown; isError: boolean; parentToolUseId?: string }
  | { type: 'plan_progress'; planPath: string; title: string; todos: unknown; timestampMs?: number; handoffKind?: 'plan' | 'curator' }
  | {
      type: 'permission_request'
      requestId: string
      toolName: string
      toolUseId?: string
      input: unknown
      description?: string
    }
  | {
      type: 'computer_use_permission_request'
      requestId: string
      request: ComputerUsePermissionRequest
    }
  | { type: 'message_complete'; usage: TokenUsage }
  | { type: 'thinking'; text: string }
  | { type: 'status'; state: ChatState; verb?: string; elapsed?: number; tokens?: number }
  | { type: 'error'; message: string; code: string; detail?: string; retryable?: boolean }
  | {
      type: 'provider_retry'
      attempt: number
      maxAttempts: number
      waitMs: number
      class: 'engine_overloaded' | 'account_rate_limited' | 'transient' | string
      provider: string
      model: string
      message: string
    }
  | {
      type: 'workspace_busy'
      workspaceKey: string
      currentSessionId?: string | null
    }
  | {
      type: 'system_notification'
      subtype: string
      level?: 'info' | 'warning' | 'error'
      code?: string
      section?: string
      message?: string
      data?: unknown
    }
  | {
      type: 'usage_updated'
      sessionId?: string | null
      codingMode?: string | null
      model?: string
      inputTokens?: number
      outputTokens?: number
      totalTokens?: number
      costUsd?: number
      timestamp?: string
    }
  | {
      type: 'debug_pii_stats'
      total?: number
      counts?: Record<string, number>
    }
  | { type: 'pong' }
  | { type: 'task_update'; taskId: string; status: string; progress?: string }
  | {
      type: 'todo_snapshot'
      sessionId: string
      todos: Array<{
        id?: string
        content: string
        status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
        activeForm?: string
        priority?: string | null
      }>
    }
  | { type: 'session_title_updated'; sessionId: string; title: string }
  | {
      type: 'worker_spawned'
      sessionId?: string
      parentToolUseId: string
      workerId: string
      title: string
      model: string
    }
  | {
      type: 'worker_status'
      sessionId?: string
      workerId: string
      status: WorkerStatus
      detail?: string | null
    }
  | {
      type: 'worker_progress'
      sessionId?: string
      workerId: string
      action: string
      detail: string
    }
  | {
      type: 'worker_completed'
      sessionId?: string
      workerId: string
      success: boolean
      summary: string
    }
  | {
      type: 'worker_stopped'
      sessionId?: string
      workerId: string
      reason: string
    }
  | {
      type: 'parent_resumed'
      sessionId?: string
      reason: string
    }

  | {
      type: 'lsp_diagnostics'
      serverId: string
      uri: string
      version?: number | null
      diagnostics: unknown[]
    }
  | { type: 'lsp_install_progress'; serverId: string; phase: string; [key: string]: unknown }
  | {
      type: 'lsp_server_status'
      serverId: string
      languageId: string
      status: string
      reason?: string | null
    }

export type TokenUsage = {
  input_tokens: number
  output_tokens: number
  cache_read_tokens?: number
  cache_creation_tokens?: number
}

export type ChatState =
  | 'idle'
  | 'thinking'
  | 'tool_executing'
  | 'streaming'
  | 'permission_pending'
  | 'awaiting_workers'

export type WorkerStatus = 'pending' | 'running' | 'completed' | 'failed' | 'stopped'

export type WorkerSnapshot = {
  workerId: string
  parentSessionId: string
  parentToolUseId: string
  title: string
  model: string
  status: WorkerStatus
  lastAction?: string | null
  lastDetail?: string | null
  startedAt: number
  finishedAt?: number | null
}

export type WorkerSummaryPayload = {
  worker_id: string
  parent_session_id: string
  parent_tool_use_id: string
  title: string
  model: string
  status: WorkerStatus
  last_action?: string | null
  last_detail?: string | null
  started_at: string
  finished_at?: string | null
}

export type TeamMemberStatus = {
  agentId: string
  role: string
  status: 'running' | 'idle' | 'completed' | 'error'
  currentTask?: string
}

export type ComputerUseGrantFlags = {
  clipboardRead: boolean
  clipboardWrite: boolean
  systemKeyCombos: boolean
}

export type ComputerUseResolvedApp = {
  bundleId: string
  displayName: string
  path?: string
  iconDataUrl?: string
}

export type ComputerUseResolvedAppRequest = {
  requestedName: string
  resolved?: ComputerUseResolvedApp
  isSentinel: boolean
  alreadyGranted: boolean
  proposedTier: 'read' | 'click' | 'full'
}

export type ComputerUsePermissionRequest = {
  requestId: string
  reason: string
  apps: ComputerUseResolvedAppRequest[]
  requestedFlags: Partial<ComputerUseGrantFlags>
  screenshotFiltering: 'native' | 'none'
  tccState?: {
    accessibility: boolean
    screenRecording: boolean
  }
  willHide?: Array<{ bundleId: string; displayName: string }>
  autoUnhideEnabled?: boolean
}

export type ComputerUsePermissionResponse = {
  granted: Array<{
    bundleId: string
    displayName: string
    grantedAt: number
    tier?: 'read' | 'click' | 'full'
  }>
  denied: Array<{
    bundleId: string
    reason: 'user_denied' | 'not_installed'
  }>
  flags: ComputerUseGrantFlags
  userConsented?: boolean
}

export type AgentTaskNotification = {
  taskId: string
  toolUseId: string
  status: 'completed' | 'failed' | 'stopped'
  summary?: string
  outputFile?: string
}

export type TaskSummaryItem = {
  id: string
  subject: string
  status: 'pending' | 'in_progress' | 'completed'
  activeForm?: string
}

export type UIMessageCommon = {

  superseded?: boolean
}

export type UIMessage =
  | (UIMessageCommon & { id: string; type: 'user_text'; content: string; timestamp: number; attachments?: UIAttachment[]; pending?: boolean; userMessageIndex?: number })
  | (UIMessageCommon & { id: string; type: 'assistant_text'; content: string; timestamp: number; model?: string })
  | (UIMessageCommon & {
      id: string
      type: 'thinking'
      content: string
      timestamp: number

      startedAt?: number

      completedAt?: number
    })
  | (UIMessageCommon & { id: string; type: 'tool_use'; toolName: string; toolUseId: string; input: unknown; timestamp: number; parentToolUseId?: string })
  | (UIMessageCommon & { id: string; type: 'tool_result'; toolUseId: string; content: unknown; isError: boolean; timestamp: number; parentToolUseId?: string })
  | (UIMessageCommon & { id: string; type: 'system'; content: string; timestamp: number })
  | (UIMessageCommon & {
      id: string
      type: 'permission_request'
      requestId: string
      toolName: string
      toolUseId?: string
      input: unknown
      description?: string
      timestamp: number
    })
  | (UIMessageCommon & { id: string; type: 'error'; message: string; code: string; detail?: string; timestamp: number })
  | (UIMessageCommon & { id: string; type: 'task_summary'; tasks: TaskSummaryItem[]; timestamp: number })

  | (UIMessageCommon & {
      id: string
      type: 'file_edit'
      path: string
      additions: number
      deletions: number
      diff?: string | null
      editBatchId?: string | null
      timestamp: number
    })

  | (UIMessageCommon & {
      id: string
      type: 'command_preview'
      toolName: string
      input: unknown
      timestamp: number
    })

  | (UIMessageCommon & {
      id: string
      type: 'subagent_chunk'
      agentId: string
      delta: string
      chunkKind: string
      taskId?: string
      parentToolUseId?: string
      timestamp: number
    })

  | (UIMessageCommon & {
      id: string
      type: 'plan_question_answers'
      timestamp: number
      items: Array<{ question: string; answer: string | string[] }>
      details?: string
    })

  | (UIMessageCommon & {
      id: string
      type: 'plan_card'
      timestamp: number
      planPath: string
      fileName: string
      title: string
      overview: string
      todos: Array<{
        id: string
        content: string
        status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
        notes?: string | null
      }>

      markdown?: string
      modelLabel?: string
      status: 'writing' | 'completed'

      pendingHydration?: boolean

      sourceToolUseId?: string

      source?: 'update_plan_save' | 'exit_plan_mode'

      wasExecuted?: boolean
    })

  | (UIMessageCommon & {
      id: string
      type: 'mode_switch_card'
      timestamp: number
      planPath: string
      targetMode: CodingModeId
      status: 'pending' | 'switched' | 'dismissed'
      handoffKind?: 'plan' | 'curator'
    })

  | (UIMessageCommon & {
      id: string
      type: 'plan_progress'
      timestamp: number
      planPath: string
      title: string
      todos: Array<{
        id: string
        content: string
        status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
        notes?: string | null
      }>
      handoffKind?: 'plan' | 'curator'
    })

  | (UIMessageCommon & {
      id: string
      type: 'curator_card'
      timestamp: number
      slug: string
      template: string
      finalMdPath: string
      implBlueprintPath: string
      docxPath?: string
      title: string
      body: string
      status: 'writing' | 'completed'
      sourceToolUseId?: string

      todos?: Array<{
        id: string
        content: string
        status: 'pending' | 'in_progress' | 'completed' | 'cancelled'
        notes?: string | null
      }>

      pendingHydration?: boolean

      wasExecuted?: boolean
    })

  | (UIMessageCommon & {
      id: string
      type: 'plan_mode_blocked'
      timestamp: number
      tools: Array<{ name: string; input?: unknown }>
      mode?: string
      reason?: 'plan' | 'read_only' | 'tool_not_allowed'
      detail?: string
    })

export type PendingEdit = {
  path: string
  additions: number
  deletions: number
  editBatchIds: string[]
  firstSeenAt: number
  lastSeenAt: number
}

export type AgentTimelineEntry =
  | { kind: 'text'; text: string }
  | { kind: 'thinking'; text: string }
  | { kind: 'tool_call'; name: string; summary: string }
  | { kind: 'tool_result'; name: string; preview: string; isError: boolean }
  | { kind: 'status'; text: string }

export type AgentTimeline = {
  agentId: string
  taskId?: string
  status: 'running' | 'completed' | 'error'
  entries: AgentTimelineEntry[]
  startedAt: number
  updatedAt: number
  finalOutput?: string
}

export type SubagentTimelineBucket = {
  parentToolUseId: string
  parentToolName: string
  agents: Record<string, AgentTimeline>
}
