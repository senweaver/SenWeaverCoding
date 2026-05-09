// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type UserRuleTier = 'always' | 'on_demand'

export type UserRuleFile = {
  name: string
  path: string
  size: number
  summary: string
  description?: string | null
  alwaysApply: boolean
  tier: UserRuleTier
}

export type UserRulesListResponse = {
  directory: string
  exists: boolean
  files: UserRuleFile[]
}

export type UserRuleFileContent = {
  name: string
  path: string
  content: string
}

export const userRulesApi = {
  list: () => api.get<UserRulesListResponse>('/api/rules'),
  get: (name: string) =>
    api.get<UserRuleFileContent>(`/api/rules/file?name=${encodeURIComponent(name)}`),
  upsert: (name: string, content: string) =>
    api.put<{ status: string; name: string; path: string }>('/api/rules/file', {
      name,
      content,
    }),
  delete: (name: string) =>
    api.delete<{ status: string }>(`/api/rules/file?name=${encodeURIComponent(name)}`),
}
