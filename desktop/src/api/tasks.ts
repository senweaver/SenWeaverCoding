// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type { CronTask, CreateTaskInput, TaskRun } from '../types/task'

type TasksResponse = { tasks: CronTask[] }
type TaskResponse = { task: CronTask }
type RunsResponse = { runs: TaskRun[] }

type RunWire = Partial<TaskRun> & {
  endedAt?: string
  summary?: string
}

function normalizeRun(r: RunWire): TaskRun {
  const base = r as TaskRun
  return {
    ...base,
    completedAt: base.completedAt ?? r.endedAt,
    output: base.output ?? r.summary,
  }
}

export const tasksApi = {
  list() {
    return api.get<TasksResponse>('/api/scheduled-tasks')
  },

  create(input: CreateTaskInput) {
    return api.post<TaskResponse>('/api/scheduled-tasks', input)
  },

  update(id: string, updates: Partial<CronTask>) {
    return api.put<TaskResponse>(`/api/scheduled-tasks/${id}`, updates)
  },

  delete(id: string) {
    return api.delete<{ ok: true }>(`/api/scheduled-tasks/${id}`)
  },

  runTask(id: string) {
    return api.post<{ ok: true }>(`/api/scheduled-tasks/${id}/run`, {})
  },

  async getTaskRuns(taskId: string) {
    const res = await api.get<RunsResponse>(`/api/scheduled-tasks/${taskId}/runs`)
    return { runs: res.runs.map((x) => normalizeRun(x as RunWire)) }
  },
}
