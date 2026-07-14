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

export type PlanDraftAttachment = {
  name: string
  mime: string
  dataBase64?: string
  text?: string
}

export async function draftPlan(
  task: string,
  attachments: PlanDraftAttachment[],
  provider?: string,
  model?: string,
): Promise<string> {
  const res = await api.post<{ steps: string }>('/api/computer/plan-draft', {
    task,
    attachments: attachments.map((a) => ({
      name: a.name,
      mime: a.mime,
      ...(a.dataBase64 ? { data_base64: a.dataBase64 } : {}),
      ...(a.text ? { text: a.text } : {}),
    })),
    ...(provider ? { provider } : {}),
    ...(model ? { model } : {}),
  })
  return res.steps ?? ''
}

export type RecordedStepWire = {
  index: number
  action_type: string
  x_norm?: number
  y_norm?: number
  to_x_norm?: number
  to_y_norm?: number
  x_abs?: number
  y_abs?: number
  to_x_abs?: number
  to_y_abs?: number
  value?: string
  amount?: number
  delay_ms: number
  screenshot_file?: string
  element_description?: string
  monitor?: unknown
}

export type RecordingRunConfig = {
  loop_count: number
  interval_ms: number
}

export type RecordingStepsResponse = {
  name: string
  task: string
  display_w: number
  display_h: number
  run_config: RecordingRunConfig | null
  steps: RecordedStepWire[]
}

export async function getRecordingSteps(name: string): Promise<RecordingStepsResponse> {
  return api.get<RecordingStepsResponse>(
    `/api/computer/recordings/${encodeURIComponent(name)}/steps`,
  )
}

export async function saveRecordingSteps(
  name: string,
  steps: RecordedStepWire[],
  runConfig: { loopCount: number; intervalMs: number } | null,
): Promise<void> {
  await api.put(`/api/computer/recordings/${encodeURIComponent(name)}/steps`, {
    steps,
    runConfig,
  })
}

export type ComputerScheduleSpec = {
  mode: 'replay' | 'agent'
  recording?: string
  task?: string
  smart?: boolean
  provider?: string
  model?: string
  loop_count?: number
  interval_ms?: number
}

export type ComputerScheduleTrigger =
  | { triggerType: 'cron'; cron: string }
  | { triggerType: 'interval'; everyMs: number }
  | { triggerType: 'once'; runAt: string }

export async function createComputerScheduledTask(
  name: string,
  trigger: ComputerScheduleTrigger,
  spec: ComputerScheduleSpec,
): Promise<void> {
  await api.post('/api/scheduled-tasks', {
    name,
    type: 'computer',
    computer: spec,
    ...trigger,
  })
}
