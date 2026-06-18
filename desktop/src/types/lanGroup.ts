// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

export type LanGroupRole = 'owner' | 'manager' | 'member' | 'viewer'

export type LanGroupSummary = {
  id: string
  name: string
  description: string
  role: LanGroupRole
  memberCount: number
  docCount: number
  taskCount: number
  openTaskCount: number
  phaseCount: number
  progress: number
  unread: number
  createdAt: number
  updatedAt: number
}

export type LanGroupMember = {
  userId: string
  nickname: string
  role: LanGroupRole
  online: boolean
  joinedAt: number
}

export type LanPhase = {
  id: string
  name: string
  order: number
  status: string
  color: string
  percent: number
  docCount: number
  taskCount: number
}

export type LanGroupDocument = {
  id: string
  name: string
  isDir: boolean
  size: number
  phaseId: string
  uploader: string
  uploaderNickname: string
  contentHash: string
  version: number
  note: string
  available: boolean
  updatedAt: number
}

export type LanTask = {
  id: string
  title: string
  description: string
  phaseId: string
  assignee: string
  assigneeNickname: string
  status: string
  priority: string
  dueMs: number
  deps: string[]
  parent: string
  kind: string
  progress: number
  createdAt: number
  updatedAt: number
}

export type LanGroupMessage = {
  id: string
  author: string
  authorNickname: string
  body: string
  kind: string
  docId: string
  tsMs: number
}

export type LanGroupSnapshot = {
  group: LanGroupSummary
  members: LanGroupMember[]
  phases: LanPhase[]
  documents: LanGroupDocument[]
  tasks: LanTask[]
}

export type TaskInputPayload = {
  taskId?: string
  title: string
  description?: string
  phaseId?: string
  assignee?: string
  status?: string
  priority?: string
  dueMs?: number
  deps?: string[]
  parent?: string
  kind?: string
  progress?: number
}
