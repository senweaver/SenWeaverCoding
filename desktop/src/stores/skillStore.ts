// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { skillsApi } from '../api/skills'
import type { SkillMeta, SkillDetail } from '../types/skill'

export type SkillDetailReturnTab = 'skills' | 'plugins'

type SkillStore = {
  skills: SkillMeta[]
  workspaceSkillsDir: string | null
  userSkillsDir: string | null
  selectedSkill: SkillDetail | null
  selectedSkillReturnTab: SkillDetailReturnTab
  isLoading: boolean
  isDetailLoading: boolean
  error: string | null

  fetchSkills: (cwd?: string) => Promise<void>
  fetchSkillDetail: (
    source: string,
    name: string,
    cwd?: string,
    returnTab?: SkillDetailReturnTab,
  ) => Promise<void>
  clearSelection: () => void
  setDisabledSkills: (disabledSkills: string[]) => Promise<void>
  fetchUserSkillContent: (name: string) => Promise<string | null>
  upsertUserSkill: (name: string, content: string) => Promise<void>
  deleteUserSkill: (name: string) => Promise<void>
}

export const useSkillStore = create<SkillStore>((set, get) => ({
  skills: [],
  workspaceSkillsDir: null,
  userSkillsDir: null,
  selectedSkill: null,
  selectedSkillReturnTab: 'skills',
  isLoading: false,
  isDetailLoading: false,
  error: null,

  fetchSkills: async (cwd) => {
    set({ isLoading: true, error: null })
    try {
      const response = await skillsApi.list(cwd)
      set({
        skills: response.skills,
        workspaceSkillsDir: response.workspace_skills_dir ?? null,
        userSkillsDir: response.user_skills_dir ?? null,
        isLoading: false,
      })
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : String(err),
        isLoading: false,
      })
    }
  },

  fetchSkillDetail: async (source, name, cwd, returnTab = 'skills') => {
    set({ isDetailLoading: true, error: null })
    try {
      const { detail } = await skillsApi.detail(source, name, cwd)
      set({
        selectedSkill: detail,
        selectedSkillReturnTab: returnTab,
        isDetailLoading: false,
      })
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : String(err),
        isDetailLoading: false,
      })
    }
  },

  clearSelection: () => set({ selectedSkill: null, selectedSkillReturnTab: 'skills' }),

  setDisabledSkills: async (disabledSkills) => {
    await skillsApi.setDisabledSkills(disabledSkills)
  },

  fetchUserSkillContent: async (name) => {
    try {
      const res = await skillsApi.getUserSkill(name)
      return res.content
    } catch (err) {
      set({ error: err instanceof Error ? err.message : String(err) })
      return null
    }
  },

  upsertUserSkill: async (name, content) => {
    await skillsApi.upsertUserSkill(name, content)
    await get().fetchSkills(undefined)
  },

  deleteUserSkill: async (name) => {
    await skillsApi.deleteUserSkill(name)
    await get().fetchSkills(undefined)
  },
}))
