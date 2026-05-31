// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



export type TaskStatus = 'pending' | 'in_progress' | 'completed'

export type CLITask = {
  id: string
  subject: string
  description: string
  activeForm?: string
  owner?: string
  status: TaskStatus
  blocks: string[]
  blockedBy: string[]
  metadata?: Record<string, unknown>
  taskListId: string
}

export type TaskListSummary = {
  id: string
  taskCount: number
  completedCount: number
  inProgressCount: number
  pendingCount: number
}
