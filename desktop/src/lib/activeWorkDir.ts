// SPDX-License-Identifier: MIT

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

export const useActiveWorkspaceRoot = useActiveTabWorkDir
export const getActiveWorkspaceRoot = getActiveTabWorkDir
