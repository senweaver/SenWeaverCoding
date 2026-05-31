// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { adaptersApi } from '../api/adapters'
import type { AdapterFileConfig, ChannelId, PairingChannelId } from '../types/adapter'
import { PAIRING_CHANNELS } from '../types/adapter'

async function notifyTauriRestartAdapters(): Promise<void> {
  try {

    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('restart_adapters_sidecar')
  } catch (err) {

    if (typeof console !== 'undefined') {
      console.warn('[adapterStore] restart_adapters_sidecar failed:', err)
    }
  }
}

const SAFE_ALPHABET = 'ABCDEFGHJKMNPQRSTUVWXYZ23456789'
const CODE_LENGTH = 6
const CODE_TTL_MS = 60 * 60 * 1000

function generateCode(): string {
  const maxValid = Math.floor(256 / SAFE_ALPHABET.length) * SAFE_ALPHABET.length
  let code = ''
  while (code.length < CODE_LENGTH) {
    const array = new Uint8Array(1)
    crypto.getRandomValues(array)
    if (array[0]! < maxValid) {
      code += SAFE_ALPHABET[array[0]! % SAFE_ALPHABET.length]
    }
  }
  return code
}

type AdapterStore = {
  config: AdapterFileConfig
  isLoading: boolean
  error: string | null

  fetchConfig: () => Promise<void>
  updateConfig: (patch: Partial<AdapterFileConfig> | Record<string, unknown>) => Promise<void>

  disableChannel: (channel: ChannelId) => Promise<void>
  generatePairingCode: () => Promise<string>
  removePairedUser: (platform: PairingChannelId, userId: string | number) => Promise<void>
}

export const useAdapterStore = create<AdapterStore>((set, get) => ({
  config: {},
  isLoading: false,
  error: null,

  fetchConfig: async () => {
    set({ isLoading: true, error: null })
    try {
      const config = await adaptersApi.getConfig()
      set({ config, isLoading: false })
    } catch (err) {
      const message = err instanceof Error ? err.message : 'Failed to load config'
      set({ isLoading: false, error: message })
    }
  },

  updateConfig: async (patch) => {
    const config = await adaptersApi.updateConfig(patch)
    set({ config })

    void notifyTauriRestartAdapters()
  },

  generatePairingCode: async () => {
    const code = generateCode()
    const now = Date.now()
    await get().updateConfig({
      pairing: {
        code,
        expiresAt: now + CODE_TTL_MS,
        createdAt: now,
      },
    })
    return code
  },

  removePairedUser: async (platform, userId) => {
    const { config } = get()
    const known = PAIRING_CHANNELS as readonly ChannelId[]
    if (!known.includes(platform)) return
    const platformConfig = config[platform] as
      | { pairedUsers?: Array<{ userId: string | number }> }
      | null
      | undefined
    if (!platformConfig) return

    const pairedUsers = (platformConfig.pairedUsers ?? []).filter(
      (u) => String(u.userId) !== String(userId),
    )

    await get().updateConfig({
      [platform]: { ...platformConfig, pairedUsers },
    })
  },

  disableChannel: async (channel) => {
    await get().updateConfig({ [channel]: null })
  },
}))
