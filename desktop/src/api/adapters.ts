// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type { AdapterFileConfig } from '../types/adapter'

export const adaptersApi = {
  getConfig() {
    return api.get<AdapterFileConfig>('/api/adapters')
  },

  updateConfig(patch: Partial<AdapterFileConfig> | Record<string, unknown>) {
    return api.put<AdapterFileConfig>('/api/adapters', patch)
  },
}
