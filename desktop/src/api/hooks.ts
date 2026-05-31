// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type { HooksConfig, HooksPatch } from '../types/hooks'

export const hooksApi = {
  get: () => api.get<HooksConfig>('/api/hooks'),
  update: (patch: HooksPatch) => api.put<HooksConfig>('/api/hooks', patch),
}
