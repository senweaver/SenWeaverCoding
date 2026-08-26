// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api, getAuthToken, getBaseUrl } from './client'

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

export type RecordingAnalysisSummary = {
  revision: number
  createdAt: number | null
  title: string
  intent: string
  intentConfidence: string
  stepCount: number
  approved: boolean
  narrationSourceUpdatedAt: number | null
}

export type RecordingSummary = {
  name: string
  task: string
  created_at: string
  step_count: number
  has_skill: boolean
  has_trace: boolean
  processed?: boolean
  has_narration?: boolean
  has_audio?: boolean
  has_analysis?: boolean
  has_automation?: boolean
  duration_ms?: number
  event_count?: number
  size_bytes?: number
  analysis?: RecordingAnalysisSummary | null
}

export async function listRecordings(): Promise<RecordingSummary[]> {
  const res = await api.get<{ recordings: RecordingSummary[] }>('/api/computer/recordings')
  return res.recordings ?? []
}

export type AnalysisStep = {
  id: string
  title: string
  detail: string
  startMs?: number
  endMs?: number
  apps: string[]
  evidence: string[]
  confidence: string
}

export type Analysis = {
  version: number
  sessionId: string
  revision: number
  createdAt: number
  narrationSourceUpdatedAt: number | null
  title: string
  intent: string
  intentConfidence: string
  intentRationale: string
  steps: AnalysisStep[]
  approved: boolean
}

export type SensitiveFinding = {
  category: string
  label: string
  severity: string
  source: string
  redactedValue: string
  snippet: string
  atMs: number | null
  occurrences: number
}

export type SensitiveReport = {
  sessionId: string
  scannedAt: number
  totalFindings: number
  highSeverityCount: number
  counts: Record<string, number>
  findings: SensitiveFinding[]
  images?: { framesBlurred: number; regionsBlurred: number }
}

export async function getAnalysis(
  name: string,
): Promise<{ analysis: Analysis | null; sensitiveReport: SensitiveReport | null }> {
  return api.get(`/api/computer/recordings/${encodeURIComponent(name)}/analysis`)
}

export async function saveAnalysis(
  name: string,
  patch: {
    title?: string
    intent?: string
    steps?: AnalysisStep[]
    approved?: boolean
  },
): Promise<Analysis> {
  const res = await api.put<{ ok: boolean; analysis: Analysis }>(
    `/api/computer/recordings/${encodeURIComponent(name)}/analysis`,
    patch,
  )
  return res.analysis
}

export async function getSensitiveReport(name: string): Promise<SensitiveReport | null> {
  const res = await api.get<{ report: SensitiveReport | null }>(
    `/api/computer/recordings/${encodeURIComponent(name)}/sensitive-report`,
  )
  return res.report ?? null
}

export async function transcribeRecording(name: string): Promise<number> {
  const res = await api.post<{ ok: boolean; segmentCount: number }>(
    `/api/computer/recordings/${encodeURIComponent(name)}/transcribe`,
  )
  return res.segmentCount ?? 0
}

export async function uploadAudioSegment(
  name: string,
  blob: Blob,
  params: { language: string; startEpoch: number; stopEpoch: number },
): Promise<void> {
  const query = new URLSearchParams({
    language: params.language,
    startEpoch: String(params.startEpoch),
    stopEpoch: String(params.stopEpoch),
  })
  const url = `${getBaseUrl()}/api/computer/recordings/${encodeURIComponent(name)}/audio?${query.toString()}`
  const headers: Record<string, string> = { 'Content-Type': 'application/octet-stream' }
  const token = getAuthToken()
  if (token) headers['X-Sen-Gateway-Token'] = token
  const res = await fetch(url, { method: 'POST', headers, body: blob })
  if (!res.ok) {
    throw new Error(`audio upload failed (${res.status})`)
  }
}

export type PrivacySettings = { advancedProtection: boolean }

export async function getPrivacySettings(): Promise<PrivacySettings> {
  return api.get('/api/computer/privacy')
}

export async function setPrivacySettings(advancedProtection: boolean): Promise<void> {
  await api.put('/api/computer/privacy', { advancedProtection })
}

export type BuildTarget = {
  architecture: string
  label: string
  kind: string
  placements: string[]
}

export async function listBuildTargets(): Promise<BuildTarget[]> {
  const res = await api.get<{ targets: BuildTarget[] }>('/api/computer/build-targets')
  return res.targets ?? []
}

export type DoctorReport = {
  platform: string
  recordingSupported: boolean
  visionModelCount: number
  visionRecommended: { provider: string; model: string } | null
  transcriptionConfigured: boolean
  ocrAvailable: boolean
}

export async function getDoctor(): Promise<DoctorReport> {
  return api.get('/api/computer/doctor')
}

export async function exportDebugBundle(name: string, destDir: string): Promise<string> {
  const res = await api.post<{ ok: boolean; path: string }>(
    `/api/computer/recordings/${encodeURIComponent(name)}/export-debug`,
    { destDir },
  )
  return res.path
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
