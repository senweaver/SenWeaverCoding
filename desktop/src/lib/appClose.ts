// SPDX-License-Identifier: MIT

import { isTauriRuntime } from './desktopRuntime'
import { getStoredCloseBehavior, useSettingsStore } from '../stores/settingsStore'
import { useUIStore } from '../stores/uiStore'
import { useSessionRunStateStore } from '../stores/sessionRunStateStore'
import { useChatStore } from '../stores/chatStore'
import { waitForSessionsIdle } from './sessionLifecycle'
import type { CloseBehavior } from '../types/settings'

const SAFE_EXIT_TIMEOUT_MS = 12_000

let exiting = false

export function getCloseBehavior(): CloseBehavior {
  try {
    const fromStore = useSettingsStore.getState().closeBehavior
    if (fromStore) return fromStore
  } catch {  }
  return getStoredCloseBehavior()
}

export async function minimizeToTray(): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    await getCurrentWindow().hide()
  } catch (err) {
    console.warn('[appClose] minimize to tray failed', err)
  }
}

export async function forceQuit(): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('quit_app')
  } catch (err) {
    console.warn('[appClose] quit_app failed', err)
  }
}

export async function performSafeExit(): Promise<void> {
  if (exiting) return
  exiting = true
  const ui = useUIStore.getState()
  ui.setClosePromptOpen(false)
  try {
    const runningIds = Array.from(useSessionRunStateStore.getState().running)
    if (runningIds.length > 0) {
      ui.setSafeExiting(true)
      const chat = useChatStore.getState()
      for (const id of runningIds) {
        try {
          chat.stopGeneration(id)
        } catch (err) {
          console.warn('[appClose] stopGeneration failed', id, err)
        }
      }
      await waitForSessionsIdle(runningIds, SAFE_EXIT_TIMEOUT_MS)
    }
    await forceQuit()
  } catch (err) {
    console.warn('[appClose] safe exit failed', err)
    exiting = false
    ui.setSafeExiting(false)
  }
}

export async function handleCloseRequest(): Promise<void> {
  if (exiting) return
  const ui = useUIStore.getState()
  if (ui.closePromptOpen || ui.safeExiting) return
  const behavior = getCloseBehavior()
  if (behavior === 'minimize') {
    await minimizeToTray()
    return
  }
  if (behavior === 'exit') {
    await performSafeExit()
    return
  }
  ui.setClosePromptOpen(true)
}
