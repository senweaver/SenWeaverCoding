// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { usageApi } from '../api/usage'
import { getBaseUrl } from '../api/client'
import type { UsageSummary } from '../types/usage'

type UsageStore = {
  summary: UsageSummary | null
  isLoading: boolean
  error: string | null

  fetch: () => Promise<void>

  subscribeRealtime: () => void
  unsubscribeRealtime: () => void
}

let eventSource: EventSource | null = null
let subscriberCount = 0
let pendingRefetchTimer: ReturnType<typeof setTimeout> | null = null

function scheduleRefetch(fetchFn: () => Promise<void>): void {
  if (pendingRefetchTimer) return
  pendingRefetchTimer = setTimeout(() => {
    pendingRefetchTimer = null
    void fetchFn().catch(() => {})
  }, 250)
}

export const useUsageStore = create<UsageStore>((set, get) => ({
  summary: null,
  isLoading: false,
  error: null,

  fetch: async () => {
    set({ isLoading: true, error: null })
    try {
      const summary = await usageApi.get('all')
      set({ summary, isLoading: false })
    } catch (err) {
      set({
        isLoading: false,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  },

  subscribeRealtime: () => {
    subscriberCount += 1
    if (eventSource) return
    try {
      const baseUrl = getBaseUrl()
      const url = `${baseUrl}/api/gateway/events`
      const es = new EventSource(url)
      eventSource = es
      es.onmessage = (ev) => {
        try {
          const payload = JSON.parse(ev.data)
          const eventType =
            typeof payload?.type === 'string' ? payload.type : null
          if (eventType === 'usage_updated' || eventType === 'agent_end') {
            scheduleRefetch(get().fetch)
          }
        } catch {
        }
      }
      es.onerror = () => {
      }
    } catch {
      eventSource = null
    }
  },

  unsubscribeRealtime: () => {
    subscriberCount = Math.max(0, subscriberCount - 1)
    if (subscriberCount === 0 && eventSource) {
      eventSource.close()
      eventSource = null
      if (pendingRefetchTimer) {
        clearTimeout(pendingRefetchTimer)
        pendingRefetchTimer = null
      }
    }
  },
}))
