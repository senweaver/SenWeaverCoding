// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type ComputerUseConfig = {
  enabled: boolean
  endpoint: string
  timeoutMs: number
  allowRemoteEndpoint: boolean
  windowAllowlist: string[]
  apiKeySet: boolean
}

export type ComputerUseUpdate = {
  enabled?: boolean
  endpoint?: string
  timeoutMs?: number
  allowRemoteEndpoint?: boolean
  windowAllowlist?: string[]
}

export const browserApi = {
  getComputerUse() {
    return api.get<ComputerUseConfig>('/api/browser-config')
  },
  updateComputerUse(patch: ComputerUseUpdate) {
    return api.put<ComputerUseConfig>('/api/browser-config', patch)
  },
}
