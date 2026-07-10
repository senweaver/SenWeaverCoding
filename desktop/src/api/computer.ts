// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type VisionModel = {
  provider: string
  provider_name: string
  model: string
  explicit_vision: boolean
  recommended: boolean
}

export async function listVisionModels(): Promise<VisionModel[]> {
  const res = await api.get<{ models: VisionModel[] }>('/api/computer/vision-models')
  return res.models ?? []
}

export async function stopComputerRun(runId: string): Promise<void> {
  await api.post('/api/computer/stop', { runId })
}

export type RecordingSummary = {
  name: string
  task: string
  created_at: string
  step_count: number
  has_skill: boolean
  has_trace: boolean
}

export async function listRecordings(): Promise<RecordingSummary[]> {
  const res = await api.get<{ recordings: RecordingSummary[] }>('/api/computer/recordings')
  return res.recordings ?? []
}

export async function deleteRecording(name: string): Promise<void> {
  await api.delete(`/api/computer/recordings/${encodeURIComponent(name)}`)
}

export async function renameRecording(name: string, newName: string): Promise<string> {
  const res = await api.post<{ ok: boolean; name: string }>(
    `/api/computer/recordings/${encodeURIComponent(name)}/rename`,
    { newName },
  )
  return res.name
}

export async function generateRecordingSkill(
  name: string,
  provider?: string,
  model?: string,
): Promise<void> {
  await api.post(`/api/computer/recordings/${encodeURIComponent(name)}/generate`, {
    ...(provider ? { provider } : {}),
    ...(model ? { model } : {}),
  })
}
