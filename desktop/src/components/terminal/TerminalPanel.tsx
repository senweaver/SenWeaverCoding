// SPDX-License-Identifier: MIT

import { useCallback, useEffect, useMemo, useRef } from 'react'

import { useTranslation } from '../../i18n'
import { useActiveTabWorkDir } from '../../lib/activeWorkDir'
import {
  TERMINAL_AGENT_MIRROR_TAB_ID,
  sessionIdFromMirrorTabId,
  useTerminalPanelStore,
  type TerminalTab,
} from '../../stores/terminalPanelStore'
import { useTabStore } from '../../stores/tabStore'
import { XtermView, type XtermViewHandle } from './XtermView'

const HEIGHT_MIN = 120
const HEIGHT_MAX = 800

export function TerminalPanel() {
  const t = useTranslation()
  const open = useTerminalPanelStore((s) => s.open)
  const heightPx = useTerminalPanelStore((s) => s.heightPx)
  const tabs = useTerminalPanelStore((s) => s.tabs)
  const activeTabId = useTerminalPanelStore((s) => s.activeTabId)
  const togglePanel = useTerminalPanelStore((s) => s.togglePanel)
  const setHeight = useTerminalPanelStore((s) => s.setHeight)
  const openNewTab = useTerminalPanelStore((s) => s.openNewTab)
  const closeTab = useTerminalPanelStore((s) => s.closeTab)
  const setActiveTab = useTerminalPanelStore((s) => s.setActiveTab)
  const setTabSession = useTerminalPanelStore((s) => s.setTabSession)
  const setTabStatus = useTerminalPanelStore((s) => s.setTabStatus)
  const setTabTitle = useTerminalPanelStore((s) => s.setTabTitle)
  const ensureAgentMirrorTab = useTerminalPanelStore((s) => s.ensureAgentMirrorTab)
  const clearAgentMirror = useTerminalPanelStore((s) => s.clearAgentMirror)
  const syncAgentMirrorForChatSession = useTerminalPanelStore((s) => s.syncAgentMirrorForChatSession)
  const activeChatTabId = useTabStore((s) => s.activeTabId)

  const handleRefs = useRef<Record<string, XtermViewHandle | null>>({})

  const activeTabWorkDir = useActiveTabWorkDir()

  useEffect(() => {
    ensureAgentMirrorTab(activeChatTabId ?? null)
  }, [ensureAgentMirrorTab, activeChatTabId])

  useEffect(() => {
    syncAgentMirrorForChatSession(activeChatTabId ?? null)
  }, [syncAgentMirrorForChatSession, activeChatTabId])

  const handleNewTab = useCallback(() => {
    openNewTab({ cwd: activeTabWorkDir ?? undefined })
  }, [openNewTab, activeTabWorkDir])

  const dragStateRef = useRef<{ startY: number; startH: number } | null>(null)

  const handleDragStart = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault()
      dragStateRef.current = { startY: e.clientY, startH: heightPx }
      const onMove = (ev: MouseEvent) => {
        const ds = dragStateRef.current
        if (!ds) return
        const delta = ds.startY - ev.clientY
        const next = Math.max(HEIGHT_MIN, Math.min(HEIGHT_MAX, ds.startH + delta))
        setHeight(next)
      }
      const onUp = () => {
        dragStateRef.current = null
        window.removeEventListener('mousemove', onMove)
        window.removeEventListener('mouseup', onUp)
      }
      window.addEventListener('mousemove', onMove)
      window.addEventListener('mouseup', onUp)
    },
    [heightPx, setHeight],
  )

  const activeTab: TerminalTab | undefined = useMemo(
    () => tabs.find((tab) => tab.id === activeTabId) ?? tabs[0],
    [tabs, activeTabId],
  )

  // Never unmount when the panel is hidden: unmounting XtermView kills the
  // underlying PTY (and its scrollback). Toggling the panel must not destroy
  // running shells, so we keep everything mounted and hide with CSS. Until the
  // user has ever opened the panel (no real tabs yet) we stay unmounted to
  // avoid paying for terminal setup that was never requested.
  const hasRealTabs = tabs.some((tab) => tab.kind !== 'agent-mirror')
  if (!open && !hasRealTabs) return null

  return (
    <div
      className={`relative flex w-full flex-shrink-0 flex-col border-t border-[var(--color-border)] bg-[var(--color-surface-container-low)] ${
        open ? '' : 'hidden'
      }`}
      style={{ height: heightPx }}
    >
      <div
        role="separator"
        aria-orientation="horizontal"
        onMouseDown={handleDragStart}
        title={t('terminal.panel.resize')}
        className="absolute inset-x-0 top-[-3px] z-10 h-[6px] cursor-ns-resize hover:bg-[var(--color-text-tertiary)]/40"
      />

      <div className="flex h-9 flex-shrink-0 items-center gap-1 border-b border-[var(--color-border)] bg-[var(--color-surface)] px-1.5">
        <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
          {tabs.map((tab) => {
            const isActive = tab.id === activeTab?.id
            const label = tabLabel(tab, t)
            return (
              <div
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`group flex h-7 max-w-[200px] flex-shrink-0 cursor-pointer items-center gap-1 rounded-[var(--radius-sm)] border px-2 text-xs transition-colors ${
                  isActive
                    ? 'border-[var(--color-border)] bg-[var(--color-surface-container-high)] text-[var(--color-text-primary)]'
                    : 'border-transparent text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
                }`}
              >
                <span className="material-symbols-outlined text-[14px]">
                  {tab.kind === 'agent-mirror' ? 'smart_toy' : 'terminal'}
                </span>
                <span className="truncate font-mono text-[11px]">{label}</span>
                {tab.kind !== 'agent-mirror' && (
                  <button
                    type="button"
                    onClick={(e) => {
                      e.stopPropagation()
                      closeTab(tab.id)
                    }}
                    title={t('terminal.panel.closeTab')}
                    className="ml-0.5 inline-flex h-4 w-4 items-center justify-center rounded text-[12px] text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
                  >
                    <span className="material-symbols-outlined text-[14px]">close</span>
                  </button>
                )}
              </div>
            )
          })}

          <button
            type="button"
            onClick={handleNewTab}
            title={t('terminal.panel.newTab')}
            className="ml-1 inline-flex h-7 w-7 flex-shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[16px]">add</span>
          </button>
        </div>

        <div className="flex flex-shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={() => {
              if (!activeTab) return
              if (activeTab.kind === 'agent-mirror') {
                clearAgentMirror(sessionIdFromMirrorTabId(activeTab.id))
              } else {
                handleRefs.current[activeTab.id]?.clear()
              }
            }}
            title={t('terminal.panel.clear')}
            className="inline-flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[16px]">mop</span>
          </button>
          <button
            type="button"
            onClick={togglePanel}
            title={t('terminal.panel.hide')}
            className="inline-flex h-7 w-7 items-center justify-center rounded-[var(--radius-sm)] text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[16px]">keyboard_arrow_down</span>
          </button>
        </div>
      </div>

      <div className="relative min-h-0 flex-1">
        {tabs.map((tab) => {
          const isActive = tab.id === activeTab?.id
          return (
            <div
              key={tab.id}
              className="absolute inset-0"
              style={{ display: isActive ? 'block' : 'none' }}
            >
              <XtermView
                tabId={tab.id}
                mode={tab.kind}
                active={isActive}
                initialCwd={tab.cwd}
                forwardRef={(handle) => {
                  handleRefs.current[tab.id] = handle
                }}
                onSpawned={(info) => {
                  setTabSession(tab.id, info.sessionId)
                  setTabTitle(tab.id, formatPtyTitle(info.cwd, info.shell))
                }}
                onExited={() => {
                  setTabStatus(tab.id, 'exited')
                }}
                onError={() => {
                  setTabStatus(tab.id, 'error')
                }}
              />
            </div>
          )
        })}
      </div>
    </div>
  )
}

function tabLabel(
  tab: TerminalTab,
  t: ReturnType<typeof useTranslation>,
): string {
  if (tab.id === TERMINAL_AGENT_MIRROR_TAB_ID) return t('terminal.tab.agentMirror')
  if (tab.kind === 'agent-mirror') {
    const sid = sessionIdFromMirrorTabId(tab.id)
    if (sid) {
      const label = t('terminal.tab.agentMirror')
      return `${label} · ${sid.slice(0, 6)}`
    }
    return t('terminal.tab.agentMirror')
  }
  if (tab.title?.trim()) return tab.title
  return t('terminal.tab.untitled')
}

function formatPtyTitle(cwd: string, shell: string): string {
  const last = cwd.split(/[\\/]/).filter(Boolean).pop() ?? cwd
  const shellName = shell.split(/[\\/]/).filter(Boolean).pop() ?? shell
  return `${last} — ${shellName}`
}
