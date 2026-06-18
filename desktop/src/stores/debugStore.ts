// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { debugApi, type DebugSubmode } from '../api/debug'

export type DebugSessionState = {
  selectedSubmodeId: string
  paramsBySubmode: Record<string, Record<string, unknown>>
}

const DEFAULT_SUBMODE_ID = 'auto'

const EMPTY_SESSION_STATE: DebugSessionState = {
  selectedSubmodeId: DEFAULT_SUBMODE_ID,
  paramsBySubmode: {},
}

type DebugState = {
  catalog: DebugSubmode[]
  loaded: boolean
  loading: boolean
  sessions: Record<string, DebugSessionState>

  load: () => Promise<void>
  selectSubmode: (sessionId: string, id: string) => void
  setParam: (sessionId: string, submodeId: string, key: string, value: unknown) => void
}

function defaultsFor(submode: DebugSubmode): Record<string, unknown> {
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

export const useDebugStore = create<DebugState>((set, get) => ({
  catalog: [],
  loaded: false,
  loading: false,
  sessions: {},

  load: async () => {
    if (get().loaded || get().loading) return
    set({ loading: true })
    try {
      const res = await debugApi.submodes()
      set({
        catalog: res.submodes ?? [],
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
