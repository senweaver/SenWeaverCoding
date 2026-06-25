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
