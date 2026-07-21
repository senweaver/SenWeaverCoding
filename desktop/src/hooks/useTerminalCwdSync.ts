// SPDX-License-Identifier: MIT

import { useEffect, useRef } from 'react'

import { terminalApi } from '../api/terminal'
import { useActiveTabWorkDir } from '../lib/activeWorkDir'
import { useTabStore } from '../stores/tabStore'
import { useTerminalPanelStore } from '../stores/terminalPanelStore'

function isWindows(): boolean {
  if (typeof navigator === 'undefined') return false
  const platform = navigator.platform || navigator.userAgent || ''
  return /win/i.test(platform)
}

function buildCdCommand(path: string): string {
  if (isWindows()) {
    // Default Windows shell is cmd.exe: backslashes are literal inside quotes and
    // `/d` also switches drive. Windows paths cannot contain a double quote, so
    // strip any stray ones defensively rather than mis-escape them.
    const safe = path.replace(/"/g, '')
    return `cd /d "${safe}"\r`
  }
  // POSIX shells: escape the characters that are special inside double quotes.
  const safe = path.replace(/([\\"$`])/g, '\\$1')
  return `cd "${safe}"\r`
}

export function useTerminalCwdSync() {
  const activeTabId = useTabStore((s) => s.activeTabId)
  const activeWorkDir = useActiveTabWorkDir()

  const lastWorkDirRef = useRef<string | null>(null)

  useEffect(() => {
    if (!activeWorkDir) {
      lastWorkDirRef.current = null
      return
    }
    const previous = lastWorkDirRef.current
    lastWorkDirRef.current = activeWorkDir
    if (previous === null || previous === activeWorkDir) return

    if (!terminalApi.isAvailable()) return

    const { tabs, setTabCwd } = useTerminalPanelStore.getState()
    const cmd = buildCdCommand(activeWorkDir)
    for (const tab of tabs) {
      if (tab.kind !== 'pty') continue
      if (tab.sessionId == null) continue
      // Never write into a terminal the user has already typed into: it may be
      // running vim / a REPL / a dev server, where `cd ...` would be injected as
      // input. Only pristine, idle-at-prompt terminals are safe to redirect.
      if (tab.interacted) continue
      if (tab.status !== 'running') continue
      void terminalApi.write(tab.sessionId, cmd).catch(() => {})
      setTabCwd(tab.id, activeWorkDir)
    }
  }, [activeTabId, activeWorkDir])
}
