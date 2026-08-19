// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useSessionStore } from '../stores/sessionStore'
import { useChatStore } from '../stores/chatStore'
import { useTabStore } from '../stores/tabStore'
import { useUIStore } from '../stores/uiStore'
import { useSettingsStore } from '../stores/settingsStore'
import { useTerminalPanelStore } from '../stores/terminalPanelStore'
import { getActiveTabWorkDir } from './activeWorkDir'
import type { ShortcutActionId } from '../types/shortcuts'
import type { TranslationKey } from '../i18n'

export function runNewSession(): void {
  useSessionStore.getState().setActiveSession(null)
  useUIStore.getState().setActiveView('code')
}

export function runSidebarSearch(): void {
  useUIStore.getState().setSidebarOpen(true)
  requestAnimationFrame(() => {
    const searchInput = document.querySelector('#sidebar-search') as HTMLInputElement | null
    searchInput?.focus()
    searchInput?.select()
  })
}

export function runStopGeneration(): boolean {
  const activeTabId = useTabStore.getState().activeTabId
  if (!activeTabId) return false
  const chatState = useChatStore.getState().sessions[activeTabId]?.chatState ?? 'idle'
  if (chatState === 'idle') return false
  useChatStore.getState().stopGeneration(activeTabId)
  return true
}

export function runToggleTerminal(): void {
  const store = useTerminalPanelStore.getState()
  const wasOpen = store.open
  store.togglePanel()
  if (!wasOpen) {
    const after = useTerminalPanelStore.getState()
    const hasInteractive = after.tabs.some((t) => t.kind === 'pty')
    if (!hasInteractive) {
      const cwd = getActiveTabWorkDir() ?? undefined
      after.openNewTab({ cwd })
    }
  }
}

export function runQuickModeSwitcher(): void {
  const ui = useUIStore.getState()
  if (ui.activeModal === 'quick-mode-switcher') {
    ui.closeModal()
  } else {
    ui.openModal('quick-mode-switcher')
  }
}

export function runModePlan(): void {
  const settings = useSettingsStore.getState()
  const tabId = useTabStore.getState().activeTabId
  void settings.requestSetCodingMode('plan').then(() => {
    if (tabId && useSettingsStore.getState().codingMode === 'plan') {
      useChatStore.getState().setSessionCodingMode(tabId, 'plan')
    }
  })
}

export function runToggleCommandPalette(): void {
  const ui = useUIStore.getState()
  if (ui.activeModal === 'command-palette') {
    ui.closeModal()
  } else {
    ui.openModal('command-palette')
  }
}

export type PaletteCommand = {
  id: string
  titleKey: TranslationKey
  icon: string
  keywords: string[]
  shortcutActionId?: ShortcutActionId
  run: () => void
}

export function paletteCommands(): PaletteCommand[] {
  return [
    {
      id: 'new-session',
      titleKey: 'shortcuts.action.new-session.label' as TranslationKey,
      icon: 'add_circle',
      keywords: ['new', 'session', 'chat', 'create'],
      shortcutActionId: 'new-session',
      run: runNewSession,
    },
    {
      id: 'sidebar-search',
      titleKey: 'shortcuts.action.sidebar-search.label' as TranslationKey,
      icon: 'search',
      keywords: ['search', 'sessions', 'sidebar', 'find'],
      shortcutActionId: 'sidebar-search',
      run: runSidebarSearch,
    },
    {
      id: 'quick-open',
      titleKey: 'commandPalette.cmd.quickOpen' as TranslationKey,
      icon: 'file_open',
      keywords: ['open', 'file', 'quick', 'goto'],
      shortcutActionId: 'quick-open',
      run: () => useUIStore.getState().openWorkspaceFinder('quick-open'),
    },
    {
      id: 'search-in-files',
      titleKey: 'commandPalette.cmd.searchInFiles' as TranslationKey,
      icon: 'manage_search',
      keywords: ['search', 'grep', 'files', 'text'],
      shortcutActionId: 'search-in-files',
      run: () => useUIStore.getState().openWorkspaceFinder('search-in-files'),
    },
    {
      id: 'workspace-symbol',
      titleKey: 'commandPalette.cmd.workspaceSymbol' as TranslationKey,
      icon: 'data_object',
      keywords: ['symbol', 'class', 'function', 'goto'],
      shortcutActionId: 'workspace-symbol',
      run: () => useUIStore.getState().openWorkspaceFinder('workspace-symbol'),
    },
    {
      id: 'toggle-terminal',
      titleKey: 'shortcuts.action.toggle-terminal.label' as TranslationKey,
      icon: 'terminal',
      keywords: ['terminal', 'shell', 'console', 'pty'],
      shortcutActionId: 'toggle-terminal',
      run: runToggleTerminal,
    },
    {
      id: 'quick-mode-switcher',
      titleKey: 'shortcuts.action.quick-mode-switcher.label' as TranslationKey,
      icon: 'tune',
      keywords: ['mode', 'switch', 'coding', 'agent', 'plan'],
      shortcutActionId: 'quick-mode-switcher',
      run: runQuickModeSwitcher,
    },
    {
      id: 'mode-plan',
      titleKey: 'shortcuts.action.mode-plan.label' as TranslationKey,
      icon: 'architecture',
      keywords: ['plan', 'mode'],
      shortcutActionId: 'mode-plan',
      run: runModePlan,
    },
    {
      id: 'stop-generation',
      titleKey: 'shortcuts.action.stop-generation.label' as TranslationKey,
      icon: 'stop_circle',
      keywords: ['stop', 'cancel', 'abort', 'generation'],
      shortcutActionId: 'stop-generation',
      run: () => {
        runStopGeneration()
      },
    },
    {
      id: 'open-settings',
      titleKey: 'commandPalette.cmd.openSettings' as TranslationKey,
      icon: 'settings',
      keywords: ['settings', 'preferences', 'config', 'options'],
      run: () => useUIStore.getState().openSettingsOverlay(),
    },
  ]
}
