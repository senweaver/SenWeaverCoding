// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type { WorkerSnapshot, WorkerStatus, WorkerSummaryPayload } from '../types/chat'

type WorkerEvent = {
  timestamp: string
  kind: { type: string; [key: string]: unknown }
}

type ListResponse = {
  workers: WorkerSummaryPayload[]
}

type DetailResponse = {
  meta: WorkerSummaryPayload & {
    prompt: string
    context?: string | null
    output?: string | null
    error?: string | null
  }
  summary?: WorkerSummaryPayload | null
}

type EventsResponse = {
  worker_id: string
  events: WorkerEvent[]
}

type CancelResponse = {
  worker_id: string
  cancelled: boolean
}

function snapshotFromSummary(s: WorkerSummaryPayload): WorkerSnapshot {
  return {
    workerId: s.worker_id,
    parentSessionId: s.parent_session_id,
    parentToolUseId: s.parent_tool_use_id,
    title: s.title,
    model: s.model,
    status: s.status as WorkerStatus,
    lastAction: s.last_action ?? null,
    lastDetail: s.last_detail ?? null,
    startedAt: new Date(s.started_at).getTime(),
    finishedAt: s.finished_at ? new Date(s.finished_at).getTime() : null,
  }
}

export const workersApi = {
  list: async (sessionId?: string): Promise<WorkerSnapshot[]> => {
    const qs = sessionId ? `?session_id=${encodeURIComponent(sessionId)}` : ''
    const data = await api.get<ListResponse>(`/api/workers${qs}`)
    return data.workers.map(snapshotFromSummary)
  },
  get: async (
    workerId: string,
  ): Promise<{ snapshot: WorkerSnapshot; prompt: string; output: string | null; error: string | null }> => {
    const data = await api.get<DetailResponse>(
      `/api/workers/${encodeURIComponent(workerId)}`,
    )
    return {
      snapshot: snapshotFromSummary(data.meta),
      prompt: data.meta.prompt,
      output: data.meta.output ?? null,
      error: data.meta.error ?? null,
    }
  },
  cancel: async (workerId: string): Promise<boolean> => {
    const data = await api.post<CancelResponse>(
      `/api/workers/${encodeURIComponent(workerId)}/cancel`,
      {},
    )
    return data.cancelled
  },
  events: async (workerId: string): Promise<WorkerEvent[]> => {
    const data = await api.get<EventsResponse>(
      `/api/workers/${encodeURIComponent(workerId)}/events`,
    )
    return data.events
  },
}
