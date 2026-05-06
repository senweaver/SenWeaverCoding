// SPDX-License-Identifier: MIT
//
// Single source of truth for "the workspace directory of whatever
// chat tab is currently focused". This matches what ChatInput shows
// in the picker chip below the composer (resolvedWorkDir there is
// `activeSession.workDir || gitInfo.workDir`).
//
// Several call sites need this same value:
//   - useTerminalCwdSync   : auto-cd PTY tabs on tab switch
//   - TerminalPanel        : default cwd for new PTY tabs
//   - useKeyboardShortcuts : Ctrl+` first-open auto spawn cwd
//
// Centralising it here ensures they all agree.
//
// We deliberately key off useTabStore.activeTabId (not
// useSessionStore.activeSessionId) because the chat composer also
// keys off tabStore — `sessionStore.activeSessionId` reflects the
// sidebar focus and can drift from the active tab.

import { useSessionStore } from '../stores/sessionStore'
import { useTabStore } from '../stores/tabStore'

export function useActiveTabWorkDir(): string | null {
  const activeTabId = useTabStore((s) => s.activeTabId)
  return useSessionStore((s) => {
    if (!activeTabId) return null
    const session = s.sessions.find((x) => x.id === activeTabId)
    const wd = session?.workDir?.trim()
    return wd && wd.length > 0 ? wd : null
  })
}

export function getActiveTabWorkDir(): string | null {
  const activeTabId = useTabStore.getState().activeTabId
  if (!activeTabId) return null
  const session = useSessionStore
    .getState()
    .sessions.find((x) => x.id === activeTabId)
  const wd = session?.workDir?.trim()
  return wd && wd.length > 0 ? wd : null
}
