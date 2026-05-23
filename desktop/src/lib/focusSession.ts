// SPDX-License-Identifier: MIT

import { useTabStore } from '../stores/tabStore'
import { useChatStore } from '../stores/chatStore'
import { useSessionStore } from '../stores/sessionStore'

export type FocusSessionOptions = {
  skipConnect?: boolean
}

export function focusSession(
  sessionId: string | null | undefined,
  options: FocusSessionOptions = {},
): void {
  if (!sessionId) {
    useSessionStore.getState().setActiveSession(null)
    return
  }
  useTabStore.getState().setActiveTab(sessionId)
  useSessionStore.getState().setActiveSession(sessionId)
  if (!options.skipConnect) {
    try {
      useChatStore.getState().connectToSession(sessionId)
    } catch {
    }
  }
}
