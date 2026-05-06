import { api } from './client'
import type { SkillMeta, SkillDetail } from '../types/skill'

export const skillsApi = {
  list: (cwd?: string) => {
    const query = cwd ? `?cwd=${encodeURIComponent(cwd)}` : ''
    return api.get<{ skills: SkillMeta[] }>(`/api/skills${query}`, { timeout: 120_000 })
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
}
