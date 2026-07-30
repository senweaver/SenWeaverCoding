// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import {
  credentialsApi,
  type CredentialFieldInput,
  type CredentialKind,
  type CredentialMeta,
  type CredentialPutBody,
} from '../api/credentials'

type CredentialsStore = {
  credentials: CredentialMeta[]
  isLoading: boolean
  error: string | null
  hasFetched: boolean
  fetchAll: () => Promise<void>
  upsert: (input: CredentialPutBody) => Promise<void>
  upsertSingle: (input: {
    name: string
    kind: CredentialKind
    value: string
  }) => Promise<void>
  upsertGroup: (input: {
    name: string
    fields: CredentialFieldInput[]
  }) => Promise<void>
  remove: (name: string) => Promise<void>
}

export const useCredentialsStore = create<CredentialsStore>((set, get) => ({
  credentials: [],
  isLoading: false,
  error: null,
  hasFetched: false,

  fetchAll: async () => {
    set({ isLoading: true, error: null })
    try {
      const res = await credentialsApi.list()
      set({
        credentials: res.credentials ?? [],
        isLoading: false,
        hasFetched: true,
      })
    } catch (err) {
      set({
        error: err instanceof Error ? err.message : String(err),
        isLoading: false,
        hasFetched: true,
      })
    }
  },

  upsert: async (input) => {
    await credentialsApi.upsert(input)
    await get().fetchAll()
  },

  upsertSingle: async ({ name, kind, value }) => {
    await get().upsert({ name, kind, value })
  },

  upsertGroup: async ({ name, fields }) => {
    await get().upsert({ name, fields })
  },

  remove: async (name) => {
    await credentialsApi.remove(name)
    await get().fetchAll()
  },
}))
