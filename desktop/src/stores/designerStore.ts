// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import {
  designerApi,
  type DesignerSubmode,
  type DesignSystemMeta,
  type PromptTemplateMeta,
  type HtmlTemplateMeta,
} from '../api/designer'

export type DesignerSessionState = {
  selectedSubmodeId: string | null
  paramsBySubmode: Record<string, Record<string, unknown>>
}

const EMPTY_SESSION_STATE: DesignerSessionState = {
  selectedSubmodeId: null,
  paramsBySubmode: {},
}

type DesignerState = {
  catalog: DesignerSubmode[]
  designSystems: DesignSystemMeta[]
  promptTemplates: PromptTemplateMeta[]
  htmlTemplates: HtmlTemplateMeta[]
  loaded: boolean
  loading: boolean
  sessions: Record<string, DesignerSessionState>

  load: () => Promise<void>
  refresh: () => Promise<void>
  selectSubmode: (sessionId: string, id: string) => void
  setParam: (sessionId: string, submodeId: string, key: string, value: unknown) => void
}

function defaultsFor(submode: DesignerSubmode): Record<string, unknown> {
  const out: Record<string, unknown> = {}
  for (const field of submode.fields) {
    if (field.default !== undefined) {
      out[field.key] = field.default
    } else if (field.type === 'multiselect') {
      out[field.key] = []
    } else if (field.type === 'toggle') {
      out[field.key] = false
    } else {
      out[field.key] = ''
    }
  }
  return out
}

export const useDesignerStore = create<DesignerState>((set, get) => ({
  catalog: [],
  designSystems: [],
  promptTemplates: [],
  htmlTemplates: [],
  loaded: false,
  loading: false,
  sessions: {},

  load: async () => {
    if (get().loaded || get().loading) return
    set({ loading: true })
    try {
      const [res, ds, pt, ht] = await Promise.all([
        designerApi.submodes(),
        designerApi.designSystems().catch(() => ({ designSystems: [] })),
        designerApi.promptTemplates().catch(() => ({ promptTemplates: [] })),
        designerApi.htmlTemplates().catch(() => ({ htmlTemplates: [] })),
      ])
      set({
        catalog: res.submodes ?? [],
        designSystems: ds.designSystems ?? [],
        promptTemplates: pt.promptTemplates ?? [],
        htmlTemplates: ht.htmlTemplates ?? [],
        loaded: true,
        loading: false,
      })
    } catch {
      set({ loading: false })
    }
  },

  refresh: async () => {
    if (get().loading) return
    set({ loading: true })
    try {
      const [res, ds, pt, ht] = await Promise.all([
        designerApi.submodes(),
        designerApi.designSystems().catch(() => ({ designSystems: [] })),
        designerApi.promptTemplates().catch(() => ({ promptTemplates: [] })),
        designerApi.htmlTemplates().catch(() => ({ htmlTemplates: [] })),
      ])
      set({
        catalog: res.submodes ?? [],
        designSystems: ds.designSystems ?? [],
        promptTemplates: pt.promptTemplates ?? [],
        htmlTemplates: ht.htmlTemplates ?? [],
        loaded: true,
        loading: false,
      })
    } catch {
      set({ loading: false })
    }
  },

  selectSubmode: (sessionId, id) => {
    if (!sessionId) return
    const submode = get().catalog.find((s) => s.id === id)
    set((state) => {
      const session = state.sessions[sessionId] ?? EMPTY_SESSION_STATE
      const defaults = submode ? defaultsFor(submode) : {}
      const params = { ...defaults, ...(session.paramsBySubmode[id] ?? {}) }
      return {
        sessions: {
          ...state.sessions,
          [sessionId]: {
            selectedSubmodeId: id,
            paramsBySubmode: { ...session.paramsBySubmode, [id]: params },
          },
        },
      }
    })
  },

  setParam: (sessionId, submodeId, key, value) => {
    if (!sessionId) return
    set((state) => {
      const session = state.sessions[sessionId] ?? EMPTY_SESSION_STATE
      return {
        sessions: {
          ...state.sessions,
          [sessionId]: {
            ...session,
            paramsBySubmode: {
              ...session.paramsBySubmode,
              [submodeId]: {
                ...(session.paramsBySubmode[submodeId] ?? {}),
                [key]: value,
              },
            },
          },
        },
      }
    })
  },
}))
