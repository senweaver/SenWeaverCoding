// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'
import type { SkillMeta, SkillDetail } from '../types/skill'

export type SkillsListResponse = {
  skills: SkillMeta[]
  workspace_skills_dir?: string
  user_skills_dir?: string | null
  open_skills_enabled?: boolean
  allow_scripts?: boolean
  disabled_skills?: string[]
  prompt_injection_mode?: 'full' | 'compact'
}

export const skillsApi = {
  list: (cwd?: string) => {
    const query = cwd ? `?cwd=${encodeURIComponent(cwd)}` : ''
    return api.get<SkillsListResponse>(`/api/skills${query}`, { timeout: 120_000 })
  },

  detail: (source: string, name: string, cwd?: string) => {
    const query = new URLSearchParams({
      source,
      name,
    })
    if (cwd) query.set('cwd', cwd)

    return api.get<{ detail: SkillDetail }>(
      `/api/skills/detail?${query.toString()}`,
      { timeout: 120_000 },
    )
  },

  setDisabledSkills: (disabledSkills: string[]) =>
    api.put<{ status: string }>('/api/skills', { disabled_skills: disabledSkills }),

  setPromptInjectionMode: (mode: 'full' | 'compact') =>
    api.put<{ status: string }>('/api/skills', { prompt_injection_mode: mode }),

  getUserSkill: (name: string) =>
    api.get<{ name: string; path: string; content: string }>(
      `/api/skills/file?name=${encodeURIComponent(name)}`,
    ),

  upsertUserSkill: (name: string, content: string) =>
    api.put<{ status: string; name: string; path: string }>('/api/skills/file', {
      name,
      content,
    }),

  deleteUserSkill: (name: string) =>
    api.delete<{ status: string }>(
      `/api/skills/file?name=${encodeURIComponent(name)}`,
    ),

  installUserSkills: (
    sources: string[],
    mode: SkillInstallMode = 'abort',
  ) =>
    api.post<{ results: SkillInstallReport[] }>('/api/skills/install', {
      sources,
      mode,
    }),
}

export type SkillInstallMode = 'abort' | 'overwrite' | 'rename'

export type SkillInstallReport = {
  source: string
  name: string | null
  target: string | null
  status: 'installed' | 'overwritten' | 'renamed' | 'exists' | 'duplicate' | 'error'
  error: string | null
}
