// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect } from 'react'
import { useSettingsStore, syncLocaleToShell } from '../stores/settingsStore'
import { useSessionRunStateStore } from '../stores/sessionRunStateStore'
import { useTabStore } from '../stores/tabStore'
import { useProviderStore } from '../stores/providerStore'
import { useMinimalStore } from '../stores/minimalStore'
import { useMinimalComputerStore } from '../stores/minimalComputerStore'
import { useMinimalRecorderStore } from '../stores/minimalRecorderStore'
import { useComputerUseStore } from '../stores/computerUseStore'
import { isTauriRuntime, subscribeServerStatus } from '../lib/desktopRuntime'
import { getBaseUrl, setAuthToken, setBaseUrl } from '../api/client'
import { wsManager } from '../api/websocket'
import {
  MINIMAL_EVENT_ACTIVATE,
  MINIMAL_EVENT_ACTIVE_SESSION,
  MINIMAL_EVENT_COMPUTER_PROGRESS,
  MINIMAL_EVENT_COMPUTER_SYNC,
  MINIMAL_EVENT_RECORDER_PROGRESS,
  MINIMAL_EVENT_RECORDER_SYNC,
} from '../lib/minimalMode'
import type {
  MinimalActivatePayload,
  MinimalActiveSession,
  MinimalComputerProgress,
  MinimalRecorderProgress,
} from '../lib/minimalMode'

let serverAuthReady: Promise<void> | null = null
let serverAuthGeneration = 0

function ensureServerAuth(options?: { force?: boolean }): Promise<void> {
  if (options?.force) {
    serverAuthReady = null
  }
  if (serverAuthReady) return serverAuthReady
  serverAuthGeneration += 1
  const generation = serverAuthGeneration
  const attempt = (async () => {
    const { invoke } = await import('@tauri-apps/api/core')
    const [url, token] = await Promise.all([
      invoke<string>('get_server_url').catch(() => null),
      invoke<string>('get_server_token').catch(() => null),
    ])
    const stale = generation !== serverAuthGeneration
    if (typeof token !== 'string' || token.length === 0) {
      if (!stale && options?.force) {
        setAuthToken(null)
      }
      throw new Error('bridge token unavailable')
    }
    if (stale) {
      throw new Error('bridge auth attempt superseded')
    }
    setAuthToken(token)
    if (typeof url === 'string' && /^https?:\/\//.test(url)) {
      setBaseUrl(url.replace(/\/$/, ''))
    }
  })()
  const guarded = attempt.catch((err) => {
    if (generation === serverAuthGeneration) {
      serverAuthReady = null
    }
    throw err
  })
  serverAuthReady = guarded
  return guarded
}

export function useMinimalWindowBridge() {
  useEffect(() => {
    if (!isTauriRuntime()) return
    let ran = false
    let disposed = false
    let unlisten: (() => void) | null = null

    void ensureServerAuth().catch(() => {})

    const bootstrap = async () => {
      if (ran) return
      ran = true
      try {
        await ensureServerAuth().catch(() => {})
        await useSettingsStore.getState().fetchAll().catch(() => {})
        useSessionRunStateStore.getState().start()
        void useProviderStore.getState().fetchProviders().catch(() => {})
        void useComputerUseStore.getState().loadModels().catch(() => {})
        void syncLocaleToShell(useSettingsStore.getState().locale).catch(() => {})
      } catch (err) {
        console.warn('[minimal] bootstrap failed', err)
      }
    }

    void (async () => {
      const { listen, emit } = await import('@tauri-apps/api/event')
      const off = await listen<MinimalActivatePayload>(MINIMAL_EVENT_ACTIVATE, (event) => {
        const variant = event.payload?.variant === 'computer' ? 'computer' : 'code'
        useMinimalStore.getState().setVariant(variant)
        if (variant === 'computer') {
          void emit(MINIMAL_EVENT_COMPUTER_SYNC).catch(() => {})
          void emit(MINIMAL_EVENT_RECORDER_SYNC).catch(() => {})
        }
        void bootstrap()
      })
      if (disposed) off()
      else unlisten = off
    })()

    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      const [{ listen }, i18n] = await Promise.all([
        import('@tauri-apps/api/event'),
        import('../i18n'),
      ])
      const off = await listen<string>(i18n.LOCALE_CHANGED_EVENT, (event) => {
        const next = event.payload === 'en' || event.payload === 'zh' ? event.payload : null
        if (!next) return
        if (useSettingsStore.getState().locale === next) return
        void i18n
          .ensureLocaleLoaded(next)
          .then(() => {
            useSettingsStore.setState({ locale: next })
          })
          .catch((err) => {
            console.warn('[minimal] locale sync failed to load dictionary', err)
          })
      })
      if (disposed) off()
      else unlisten = off
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      const off = await subscribeServerStatus((snap) => {
        if (snap.state !== 'ready') return
        const url = snap.url?.trim()
        if (!url) return
        const normalized = url.replace(/\/$/, '')
        if (normalized === getBaseUrl().replace(/\/$/, '')) return
        setBaseUrl(normalized)
        void ensureServerAuth({ force: true })
          .then(() => {
            setBaseUrl(normalized)
            useSessionRunStateStore.getState().stop()
            useSessionRunStateStore.getState().start()
            wsManager.forceReconnectAll()
          })
          .catch((err) => {
            console.warn('[minimal] server auth refresh failed; skipping reconnect', err)
          })
      })
      if (disposed) off()
      else unlisten = off
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      const { listen } = await import('@tauri-apps/api/event')
      const off = await listen<MinimalActiveSession | null>(MINIMAL_EVENT_ACTIVE_SESSION, (event) => {
        const active = event.payload
        if (!active?.id) {
          useTabStore.setState({ activeTabId: null })
          return
        }
        const { id, title } = active
        useTabStore.setState((state) => {
          const existing = state.tabs.find((tab) => tab.sessionId === id)
          if (existing) {
            const tabs =
              title && existing.title !== title
                ? state.tabs.map((tab) =>
                    tab.sessionId === id ? { ...tab, title } : tab,
                  )
                : state.tabs
            return { tabs, activeTabId: id }
          }
          return {
            tabs: [
              ...state.tabs,
              {
                sessionId: id,
                title: title ?? 'Session',
                type: 'session' as const,
                status: 'idle' as const,
              },
            ],
            activeTabId: id,
          }
        })
      })
      if (disposed) off()
      else unlisten = off
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      const { listen, emit } = await import('@tauri-apps/api/event')
      const off = await listen<MinimalComputerProgress>(
        MINIMAL_EVENT_COMPUTER_PROGRESS,
        (event) => {
          if (event.payload) useMinimalComputerStore.getState().applyProgress(event.payload)
        },
      )
      if (disposed) {
        off()
        return
      }
      unlisten = off
      try {
        await emit(MINIMAL_EVENT_COMPUTER_SYNC)
      } catch {

      }
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [])

  useEffect(() => {
    if (!isTauriRuntime()) return
    let disposed = false
    let unlisten: (() => void) | null = null
    void (async () => {
      const { listen, emit } = await import('@tauri-apps/api/event')
      const off = await listen<MinimalRecorderProgress>(
        MINIMAL_EVENT_RECORDER_PROGRESS,
        (event) => {
          if (event.payload) useMinimalRecorderStore.getState().applyProgress(event.payload)
        },
      )
      if (disposed) {
        off()
        return
      }
      unlisten = off
      try {
        await emit(MINIMAL_EVENT_RECORDER_SYNC)
      } catch {

      }
    })()
    return () => {
      disposed = true
      if (unlisten) unlisten()
    }
  }, [])
}
