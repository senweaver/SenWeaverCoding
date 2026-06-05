// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type IntegrationStatus = 'Available' | 'Active' | 'ComingSoon'

export type IntegrationInfo = {
  name: string
  description: string
  category: string
  status: IntegrationStatus
}

export const integrationsApi = {
  list() {
    return api.get<{ integrations: IntegrationInfo[] }>('/api/integrations')
  },
}
