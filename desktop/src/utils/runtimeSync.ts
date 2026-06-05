// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { modelsApi } from '../api/models'
import { providersApi } from '../api/providers'
import { wsManager } from '../api/websocket'
import { useChatStore } from '../stores/chatStore'
import { useProviderStore } from '../stores/providerStore'
import { useSettingsStore } from '../stores/settingsStore'
import { useUIStore } from '../stores/uiStore'
import { t } from '../i18n'
import type { RuntimeSelection } from '../types/runtime'
import {
  persistRuntimeSelection,
  resolveEffectiveRuntimeSelection,
} from './runtimeSelection'

function isPersistableSessionId(sessionId: string | null | undefined): sessionId is string {
  return !!sessionId && !sessionId.startsWith('__')
}

export async function syncRuntimeSelectionToBackend(
  selection: RuntimeSelection,
  sessionId?: string | null,
  persist = true,
): Promise<void> {
  const providerId = selection.providerId?.trim() ?? ''
  const modelId = selection.modelId?.trim() ?? ''
  if (!providerId || !modelId) return

  if (isPersistableSessionId(sessionId)) {
    if (wsManager.isConnected(sessionId)) {
      const confirmed = await wsManager.sendRuntimeConfig(
        sessionId,
        { providerId, modelId },
        { persist },
      )
      if (!confirmed) {
        useUIStore.getState().addToast({
          type: 'warning',
          message: t('runtime.syncTimeout'),
          duration: 5000,
        })
      }
      return
    }
    useChatStore.getState().setSessionRuntime(sessionId, { providerId, modelId }, { persist })
    return
  }

  await providersApi.activate(providerId)
  await modelsApi.setCurrent(modelId)
}

export function resolveSessionRuntimeSelection(
  sessionId: string | null | undefined,
): RuntimeSelection | null {
  const providers = useProviderStore.getState().providers
  const activeId = useProviderStore.getState().activeId
  const settingsModelId = useSettingsStore.getState().currentModel?.id
  return resolveEffectiveRuntimeSelection(
    sessionId,
    providers,
    activeId,
    settingsModelId,
  )
}

export async function ensureSessionRuntimeSynced(
  sessionId: string,
  options?: { persist?: boolean },
): Promise<RuntimeSelection | null> {
  const selection = resolveSessionRuntimeSelection(sessionId)
  if (!selection?.providerId || !selection.modelId.trim()) return null
  persistRuntimeSelection(sessionId, selection)
  await syncRuntimeSelectionToBackend(selection, sessionId, options?.persist ?? false)
  return selection
}

export function queueSessionRuntimeSync(
  sessionId: string,
  options?: { persist?: boolean },
): RuntimeSelection | null {
  if (!isPersistableSessionId(sessionId)) return null
  const selection = resolveSessionRuntimeSelection(sessionId)
  if (!selection?.providerId || !selection.modelId.trim()) return null
  persistRuntimeSelection(sessionId, selection)
  wsManager.send(sessionId, {
    type: 'set_runtime_config',
    persist: options?.persist ?? false,
    providerId: selection.providerId,
    modelId: selection.modelId,
  })
  return selection
}
