// SPDX-License-Identifier: MIT

import { useEffect, useRef } from 'react'

import { terminalApi } from '../api/terminal'
import { useActiveTabWorkDir } from '../lib/activeWorkDir'
import { useTabStore } from '../stores/tabStore'
import { useTerminalPanelStore } from '../stores/terminalPanelStore'

function buildCdCommand(path: string): string {
  const escaped = path.replace(/"/g, '\\"')
  return `cd "${escaped}"\r`
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
      void terminalApi.write(tab.sessionId, cmd).catch(() => {})
      setTabCwd(tab.id, activeWorkDir)
    }
  }, [activeTabId, activeWorkDir])
}
