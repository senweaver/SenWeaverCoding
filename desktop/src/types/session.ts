// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



export type SessionListItem = {
  id: string
  title: string
  createdAt: string
  modifiedAt: string
  messageCount: number
  projectPath: string
  workDir: string | null
  workDirExists: boolean
  running?: boolean
}

export type MessageEntry = {
  id: string
  type: 'user' | 'assistant' | 'system' | 'tool_use' | 'tool_result'
  content: unknown
  timestamp: string
  model?: string
  parentUuid?: string
  parentToolUseId?: string
  isSidechain?: boolean

  tombstoned?: boolean

  attachments?: Array<{
    type: 'file' | 'image'
    name?: string
    path?: string
    data?: string
    mimeType?: string
  }>

  designRef?: string
  designRefName?: string
  designRefElement?: string
  designRefElementLabel?: string
}

export type PendingRewindSummary = {
  rewindId: string
  userMessageIndex: number
  filesChanged: string[]
  insertions: number
  deletions: number
}

export type SessionDetail = SessionListItem & {
  messages: MessageEntry[]
  pendingRewind?: PendingRewindSummary | null
}
