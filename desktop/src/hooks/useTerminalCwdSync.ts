// SPDX-License-Identifier: MIT
//
// Auto-syncs every interactive PTY tab in the bottom Terminal Panel
// to the workspace directory of the currently active chat session.
//
// When the user switches sessions (each session has its own workDir
// pinned in sessionStore), this hook injects a `cd "<workDir>"` line
// into the stdin of every PTY tab so the user does not have to
// manually re-cd. agent-mirror tabs are read-only and skipped.
//
// First-time activation does not inject (the user has not "switched"
// yet — the initial cwd was already chosen at tab spawn time via
// openNewTab({ cwd })). PTY sessions that are mid-edit (vim/less/...)
// will visibly receive the keystrokes; that is expected and matches
// VS Code's behaviour: the cd is sent to the foreground process and
// it is up to the user to quit interactive programs first.

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
