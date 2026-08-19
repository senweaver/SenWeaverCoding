// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useRef, useState } from 'react'
import { useTranslation } from '../i18n'
import { useSessionStore } from '../stores/sessionStore'
import { useChatStore } from '../stores/chatStore'
import { useProviderStore } from '../stores/providerStore'
import { useSessionRuntimeStore, DRAFT_RUNTIME_SELECTION_KEY } from '../stores/sessionRuntimeStore'
import { useSettingsStore } from '../stores/settingsStore'
import { useUIStore } from '../stores/uiStore'
import { useTabStore } from '../stores/tabStore'
import { DirectoryPicker } from '../components/shared/DirectoryPicker'
import { ChatInput } from '../components/chat/ChatInput'
import type { RuntimeSelection } from '../types/runtime'
import { isValidRuntimeSelection, pickFirstConfiguredSelection } from '../utils/runtimeSelection'

export function EmptySession() {
  const t = useTranslation()
  const [workDir, setWorkDir] = useState('')
  const submittingRef = useRef(false)
  const createSession = useSessionStore((state) => state.createSession)
  const sendMessage = useChatStore((state) => state.sendMessage)
  const setSessionRuntime = useChatStore((state) => state.setSessionRuntime)
  const connectToSession = useChatStore((state) => state.connectToSession)
  const setActiveView = useUIStore((state) => state.setActiveView)
  const addToast = useUIStore((state) => state.addToast)

  const handleHeroSubmit: ReturnType<typeof useChatStore.getState>['sendMessage'] = (
    _sessionId,
    text,
    attachments,
    opts,
  ) => {
    if (submittingRef.current) return
    submittingRef.current = true
    void (async () => {
      try {
        const settings = useSettingsStore.getState()
        let providerState = useProviderStore.getState()
        if (
          settings.activeProviderName &&
          providerState.providers.length === 0 &&
          !providerState.isLoading
        ) {
          await providerState.fetchProviders()
          providerState = useProviderStore.getState()
        }
        const inferredProviderId = providerState.activeId ?? (
          settings.activeProviderName
            ? providerState.providers.find((provider) => provider.name === settings.activeProviderName)?.id ?? null
            : null
        )

        const persistedDraft =
          useSessionRuntimeStore.getState().selections[DRAFT_RUNTIME_SELECTION_KEY]
        const validDraft = isValidRuntimeSelection(persistedDraft, providerState.providers)
          ? persistedDraft
          : null
        const fallbackSelection =
          pickFirstConfiguredSelection(providerState.providers, inferredProviderId)
        const draftSelection: RuntimeSelection =
          validDraft
          ?? fallbackSelection
          ?? { providerId: inferredProviderId, modelId: '' }
        const sessionId = await createSession(workDir || undefined)
        setActiveView('code')
        useTabStore.getState().openTab(sessionId, t('sidebar.untitled'))
        connectToSession(sessionId)
        useSessionRuntimeStore.getState().setSelection(sessionId, draftSelection)

        setSessionRuntime(sessionId, draftSelection, { persist: false })
        sendMessage(sessionId, text, attachments, opts)
      } catch (error) {
        addToast({
          type: 'error',
          message: error instanceof Error ? error.message : t('empty.failedToCreate'),
        })
      } finally {
        submittingRef.current = false
      }
    })()
  }

  return (
    <div className="relative flex flex-1 flex-col overflow-hidden bg-[var(--color-surface)]">
      <div className="flex flex-1 flex-col items-center justify-center p-8 pb-32">
        <div className="flex max-w-md flex-col items-center text-center">
          <img src="/app-icon.png" alt="SenWeaverCoding" className="mb-6 h-24 w-24" />
          <h1 className="mb-2 text-3xl font-extrabold tracking-tight text-[var(--color-text-primary)]" style={{ fontFamily: 'var(--font-headline)' }}>
            {t('empty.title')}
          </h1>
          <p className="mx-auto max-w-xs text-[var(--color-text-secondary)]" style={{ fontFamily: 'var(--font-body)' }}>
            {t('empty.subtitle')}
          </p>
        </div>
      </div>

      <div className="absolute bottom-4 left-0 right-0 flex justify-center px-8">
        <div className="flex w-full max-w-3xl flex-col gap-1.5">
          <ChatInput
            variant="hero"
            draftWorkDir={workDir || undefined}
            onSubmit={handleHeroSubmit}
          />
          <div>
            <DirectoryPicker
              value={workDir}
              onChange={(path) => {
                setWorkDir(path)
                useSessionStore.getState().setUserPinnedSessionWorkDir(path)
              }}
            />
          </div>
        </div>
      </div>
    </div>
  )
}
