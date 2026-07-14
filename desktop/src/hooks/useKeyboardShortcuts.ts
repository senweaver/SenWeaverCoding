// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef } from 'react'
import { useSessionStore } from '../stores/sessionStore'
import { useChatStore } from '../stores/chatStore'
import { useTabStore } from '../stores/tabStore'
import { useUIStore } from '../stores/uiStore'
import { useSettingsStore } from '../stores/settingsStore'
import { useTerminalPanelStore } from '../stores/terminalPanelStore'
import { useKeyboardShortcutsStore } from '../stores/keyboardShortcutsStore'
import { useWorkspaceFilesStore } from '../stores/workspaceFilesStore'
import { matchesBinding } from '../types/shortcuts'
import { getActiveTabWorkDir } from '../lib/activeWorkDir'

export function useKeyboardShortcuts() {
  const setActiveSession = useSessionStore((s) => s.setActiveSession)
  const setActiveView = useUIStore((s) => s.setActiveView)
  const setSidebarOpen = useUIStore((s) => s.setSidebarOpen)
  const closeModal = useUIStore((s) => s.closeModal)
  const openModal = useUIStore((s) => s.openModal)
  const activeModal = useUIStore((s) => s.activeModal)
  const stopGeneration = useChatStore((s) => s.stopGeneration)
  const setSessionCodingMode = useChatStore((s) => s.setSessionCodingMode)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const chatState = useChatStore((s) =>
    activeTabId ? s.sessions[activeTabId]?.chatState ?? 'idle' : 'idle',
  )

  const activeModalRef = useRef(activeModal)
  activeModalRef.current = activeModal
  const chatStateRef = useRef(chatState)
  chatStateRef.current = chatState
  const activeTabIdRef = useRef(activeTabId)
  activeTabIdRef.current = activeTabId

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const bindings = useKeyboardShortcutsStore.getState().bindings

      // Detect the typing context up front so configurable shortcuts that
      // collide with normal typing (Ctrl+K, Ctrl+N, mode switch) don't steal
      // focus while the user is composing a message or editing a file.
      const targetElEarly = e.target as HTMLElement | null
      const tagEarly = targetElEarly?.tagName?.toLowerCase()
      const isContentEditableEarly = targetElEarly?.isContentEditable === true
      const isMonacoEarly = !!targetElEarly?.closest?.(
        '[data-workspace-editor], .monaco-editor',
      )
      const isChatInputEarly =
        !!targetElEarly?.closest?.(
          '[data-role="chat-composer"], [data-chat-input], [data-chat-textarea]',
        ) || (tagEarly === 'textarea' && !isMonacoEarly)
      const isTypingContext =
        (tagEarly === 'input' ||
          tagEarly === 'textarea' ||
          isContentEditableEarly ||
          isChatInputEarly) &&
        !isMonacoEarly

      if (matchesBinding(e, bindings['new-session'])) {
        if (isTypingContext) return
        e.preventDefault()
        setActiveSession(null)
        setActiveView('code')
        return
      }

      if (matchesBinding(e, bindings['sidebar-search'])) {
        if (isTypingContext) return
        e.preventDefault()
        setSidebarOpen(true)
        requestAnimationFrame(() => {
          const searchInput = document.querySelector(
            '#sidebar-search',
          ) as HTMLInputElement | null
          searchInput?.focus()
          searchInput?.select()
        })
        return
      }

      if (matchesBinding(e, bindings['close-modal'])) {
        if (activeModalRef.current) {
          closeModal()
          return
        }
        // Esc-to-interrupt (Claude Code muscle memory): only when the composer
        // itself is focused and no in-composer popover (@ file search or /
        // slash command) is open. Scoping this way avoids hijacking Esc from
        // menus, the Monaco editor, dropdowns, and other native Esc consumers.
        const composerFocused =
          isChatInputEarly &&
          !document.getElementById('file-search-menu') &&
          !document.getElementById('slash-command-menu')
        if (
          e.key === 'Escape' &&
          composerFocused &&
          chatStateRef.current !== 'idle' &&
          activeTabIdRef.current
        ) {
          e.preventDefault()
          stopGeneration(activeTabIdRef.current)
        }
        return
      }

      if (matchesBinding(e, bindings['stop-generation'])) {
        if (chatStateRef.current !== 'idle' && activeTabIdRef.current) {
          e.preventDefault()
          stopGeneration(activeTabIdRef.current)
        }
        return
      }

      if (matchesBinding(e, bindings['toggle-terminal'])) {
        e.preventDefault()
        e.stopPropagation()
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
        return
      }

      if (matchesBinding(e, bindings['quick-mode-switcher'])) {
        if (isTypingContext) return
        e.preventDefault()
        e.stopPropagation()
        const ui = useUIStore.getState()
        if (ui.activeModal === 'quick-mode-switcher') {
          ui.closeModal()
        } else {
          ui.openModal('quick-mode-switcher')
        }
        return
      }

      if (matchesBinding(e, bindings['mode-plan'])) {
        if (isTypingContext) return
        e.preventDefault()
        e.stopPropagation()
        const settings = useSettingsStore.getState()
        const tabId = activeTabIdRef.current
        void settings.requestSetCodingMode('plan').then(() => {
          if (tabId && useSettingsStore.getState().codingMode === 'plan') {
            setSessionCodingMode(tabId, 'plan')
          }
        })
        return
      }

      const ctrlOrMeta = e.ctrlKey || e.metaKey
      const targetEl = e.target as HTMLElement | null
      const tag = targetEl?.tagName?.toLowerCase()
      const isContentEditable = targetEl?.isContentEditable === true
      const isMonacoEditor = !!targetEl?.closest?.('[data-workspace-editor], .monaco-editor')
      const isChatInput =
        !!targetEl?.closest?.('[data-role="chat-composer"], [data-chat-input], [data-chat-textarea]') ||
        (tag === 'textarea' && !isMonacoEditor)
      const isFormInput = tag === 'input' || tag === 'textarea' || isContentEditable
      const isEditing = isFormInput && !isMonacoEditor
      const isEditingForGlobalSearch = isChatInput

      if (ctrlOrMeta && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'p') {
        e.preventDefault()
        e.stopPropagation()
        useUIStore.getState().openWorkspaceFinder('quick-open')
        return
      }
      if (ctrlOrMeta && e.shiftKey && !e.altKey && e.key.toLowerCase() === 'f') {
        if (isEditingForGlobalSearch) {
          return
        }
        e.preventDefault()
        e.stopPropagation()
        useUIStore.getState().openWorkspaceFinder('search-in-files')
        return
      }
      if (ctrlOrMeta && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 't') {
        if (isEditing) return
        e.preventDefault()
        e.stopPropagation()
        useUIStore.getState().openWorkspaceFinder('workspace-symbol')
        return
      }
      if (ctrlOrMeta && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'w') {
        const workspaceState = useWorkspaceFilesStore.getState()
        const tab = workspaceState.activeTab
        const finderOpen = useUIStore.getState().workspaceFinderMode !== null
        if (tab && !isEditing && !finderOpen) {
          e.preventDefault()
          e.stopPropagation()
          useUIStore.getState().requestEditorTabClose(tab)
          return
        }
      }
      if (ctrlOrMeta && !e.altKey && e.key === 'Tab') {
        const workspaceState = useWorkspaceFilesStore.getState()
        if (workspaceState.openTabs.length > 1) {
          e.preventDefault()
          e.stopPropagation()
          const tabs = workspaceState.openTabs
          const current = workspaceState.activeTab
          const idx = current ? tabs.indexOf(current) : -1
          const dir = e.shiftKey ? -1 : 1
          const nextIdx = (idx + dir + tabs.length) % tabs.length
          const nextRel = tabs[nextIdx] ?? tabs[0]
          if (nextRel) {
            void workspaceState.selectFile(nextRel)
          }
          return
        }
        // No multi-file editor context: cycle chat/session tabs instead.
        const tabState = useTabStore.getState()
        if (tabState.tabs.length > 1) {
          e.preventDefault()
          e.stopPropagation()
          const ids = tabState.tabs.map((tb) => tb.sessionId)
          const curIdx = tabState.activeTabId
            ? ids.indexOf(tabState.activeTabId)
            : -1
          const dir = e.shiftKey ? -1 : 1
          const nextIdx = (curIdx + dir + ids.length) % ids.length
          const nextId = ids[nextIdx] ?? ids[0]
          if (nextId) {
            tabState.setActiveTab(nextId)
          }
          return
        }
      }
      if (
        ctrlOrMeta &&
        !e.altKey &&
        !e.shiftKey &&
        /^[1-9]$/.test(e.key)
      ) {
        const workspaceState = useWorkspaceFilesStore.getState()
        const idx = parseInt(e.key, 10) - 1
        const target = workspaceState.openTabs[idx]
        if (target) {
          e.preventDefault()
          e.stopPropagation()
          void workspaceState.selectFile(target)
          return
        }
      }
    }

    document.addEventListener('keydown', handler, { capture: true })
    return () =>
      document.removeEventListener('keydown', handler, {
        capture: true,
      } as EventListenerOptions)
  }, [
    closeModal,
    openModal,
    setActiveSession,
    setActiveView,
    setSidebarOpen,
    setSessionCodingMode,
    stopGeneration,
  ])
}
