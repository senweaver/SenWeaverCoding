// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type DreamPriority = 'low' | 'normal' | 'high'

export type DreamTrigger =
  | { type: 'idle'; afterIdleMs: number }
  | { type: 'interval'; everyMs: number }
  | { type: 'once'; atMs: number }
  | { type: 'on_session_end' }

export type DreamTask = {
  id: string
  prompt: string
  priority: DreamPriority
  trigger: DreamTrigger
  maxDurationMs: number
  allowedTools: string[]
  createdAtMs: number
  lastRunMs: number | null
  runCount: number
  enabled: boolean
}

export type AutoDreamState = {
  enabled: boolean
  maxConcurrent: number
  tasks: DreamTask[]
}

export type DreamTaskInput = {
  prompt: string
  priority: DreamPriority
  trigger: DreamTrigger
  maxDurationMs: number
  allowedTools: string[]
  enabled: boolean
}

export const autoDreamApi = {
  get() {
    return api.get<AutoDreamState>('/api/auto-dream')
  },
  setEnabled(enabled: boolean) {
    return api.put<AutoDreamState>('/api/auto-dream', { enabled })
  },
  createTask(input: DreamTaskInput) {
    return api.post<DreamTask>('/api/auto-dream/tasks', input)
  },
  updateTask(id: string, input: DreamTaskInput) {
    return api.put<DreamTask>(
      `/api/auto-dream/tasks/${encodeURIComponent(id)}`,
      input,
    )
  },
  removeTask(id: string) {
    return api.delete<{ status: string; id: string }>(
      `/api/auto-dream/tasks/${encodeURIComponent(id)}`,
    )
  },
}
