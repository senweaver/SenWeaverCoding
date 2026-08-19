// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef } from 'react'
import { useChatStore } from '../stores/chatStore'
import { useTabStore } from '../stores/tabStore'
import { useUIStore } from '../stores/uiStore'
import { useKeyboardShortcutsStore } from '../stores/keyboardShortcutsStore'
import { useWorkspaceFilesStore } from '../stores/workspaceFilesStore'
import { matchesBinding } from '../types/shortcuts'
import {
  runModePlan,
  runNewSession,
  runQuickModeSwitcher,
  runSidebarSearch,
  runStopGeneration,
  runToggleCommandPalette,
  runToggleTerminal,
} from '../lib/commandRegistry'

export function useKeyboardShortcuts() {
  const closeModal = useUIStore((s) => s.closeModal)
  const activeModal = useUIStore((s) => s.activeModal)
  const stopGeneration = useChatStore((s) => s.stopGeneration)
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
        runNewSession()
        return
      }

      if (matchesBinding(e, bindings['sidebar-search'])) {
        if (isTypingContext || isMonacoEarly) return
        e.preventDefault()
        runSidebarSearch()
        return
      }

      if (matchesBinding(e, bindings['close-modal'])) {
        if (activeModalRef.current) {
          closeModal()
          return
        }
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
          runStopGeneration()
        }
        return
      }

      if (matchesBinding(e, bindings['toggle-terminal'])) {
        e.preventDefault()
        e.stopPropagation()
        runToggleTerminal()
        return
      }

      if (matchesBinding(e, bindings['quick-mode-switcher'])) {
        if (isTypingContext) return
        e.preventDefault()
        e.stopPropagation()
        runQuickModeSwitcher()
        return
      }

      if (matchesBinding(e, bindings['command-palette'])) {
        e.preventDefault()
        e.stopPropagation()
        runToggleCommandPalette()
        return
      }

      if (matchesBinding(e, bindings['mode-plan'])) {
        if (isTypingContext) return
        e.preventDefault()
        e.stopPropagation()
        runModePlan()
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

      if (matchesBinding(e, bindings['quick-open'])) {
        e.preventDefault()
        e.stopPropagation()
        useUIStore.getState().openWorkspaceFinder('quick-open')
        return
      }
      if (matchesBinding(e, bindings['search-in-files'])) {
        if (isEditingForGlobalSearch) {
          return
        }
        e.preventDefault()
        e.stopPropagation()
        useUIStore.getState().openWorkspaceFinder('search-in-files')
        return
      }
      if (matchesBinding(e, bindings['workspace-symbol'])) {
        if (isEditing) return
        e.preventDefault()
        e.stopPropagation()
        useUIStore.getState().openWorkspaceFinder('workspace-symbol')
        return
      }
      if (matchesBinding(e, bindings['close-editor-tab'])) {
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
  }, [closeModal, stopGeneration])
}
