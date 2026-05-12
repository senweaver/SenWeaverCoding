import { create } from 'zustand'
import {
  credentialsApi,
  type CredentialKind,
  type CredentialMeta,
} from '../api/credentials'

type CredentialsStore = {
  credentials: CredentialMeta[]
  isLoading: boolean
  error: string | null
  hasFetched: boolean
  fetchAll: () => Promise<void>
  upsert: (input: { name: string; kind: CredentialKind; value: string }) => Promise<void>
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

  upsert: async ({ name, kind, value }) => {
    await credentialsApi.upsert({ name, kind, value })
    await get().fetchAll()
  },

  remove: async (name) => {
    await credentialsApi.remove(name)
    await get().fetchAll()
  },
}))
