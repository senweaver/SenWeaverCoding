// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { isTauriRuntime } from './desktopRuntime'
import { t } from '../i18n'
import { useUIStore, type AppMode } from '../stores/uiStore'
import { useTabStore, SCHEDULED_TAB_ID } from '../stores/tabStore'
import type { AttachmentRef } from '../types/chat'
import type { useChatStore } from '../stores/chatStore'
import type { ComputerAttachment, ComputerStatus } from '../stores/computerUseStore'
import type { RecorderStatus } from '../stores/computerRecorderStore'

type ChatSendOptions = Parameters<
  ReturnType<typeof useChatStore.getState>['sendMessage']
>[3]

function notifyMinimal(type: 'error' | 'warning' | 'info', message: string): void {
  try {
    useUIStore.getState().addToast({ type, message, duration: type === 'error' ? 8000 : 5000 })
  } catch {

  }
}

export const MINIMAL_WINDOW_LABEL = 'minimal'
export const MINIMAL_INPUT_WINDOW_LABEL = 'minimal-input'
export const MAIN_WINDOW_LABEL = 'main'

export const MINIMAL_COLLAPSED_WIDTH = 200
export const MINIMAL_COLLAPSED_HEIGHT = 96

export const MINIMAL_INPUT_SIZE: Record<MinimalVariant, { width: number; height: number }> = {
  code: { width: 404, height: 380 },
  computer: { width: 404, height: 470 },
}

export const MINIMAL_EVENT_OPEN_SETTINGS = 'minimal://open-settings'
export const MINIMAL_EVENT_ACTIVE_SESSION = 'minimal://active-session'
export const MINIMAL_EVENT_ACTIVATE = 'minimal://activate'
export const MINIMAL_EVENT_SUBMIT = 'minimal://submit'
export const MINIMAL_EVENT_STOP = 'minimal://stop'
export const MINIMAL_EVENT_INPUT_HIDDEN = 'minimal://input-hidden'
export const MINIMAL_EVENT_INPUT_SHOW = 'minimal://input-show'

export const MINIMAL_EVENT_COMPUTER_PROGRESS = 'minimal://computer-progress'
export const MINIMAL_EVENT_COMPUTER_START = 'minimal://computer-start'
export const MINIMAL_EVENT_COMPUTER_STOP = 'minimal://computer-stop'
export const MINIMAL_EVENT_COMPUTER_REPLY = 'minimal://computer-reply'
export const MINIMAL_EVENT_COMPUTER_STEER = 'minimal://computer-steer'
export const MINIMAL_EVENT_COMPUTER_EXIT = 'minimal://computer-exit'
export const MINIMAL_EVENT_COMPUTER_SYNC = 'minimal://computer-sync'

export const MINIMAL_EVENT_RECORDER_PROGRESS = 'minimal://recorder-progress'
export const MINIMAL_EVENT_RECORDER_CONTROL = 'minimal://recorder-control'
export const MINIMAL_EVENT_RECORDER_SYNC = 'minimal://recorder-sync'
export const MINIMAL_EVENT_COMPUTER_REPLAY = 'minimal://computer-replay'

export type MinimalVariant = AppMode

export interface MinimalActivatePayload {
  variant: MinimalVariant
  sessionId: string
}

export interface MinimalActiveSession {
  id: string
  title: string | null
}

export interface MinimalSubmitPayload {
  sessionId: string
  content: string
  attachments?: AttachmentRef[]
  options?: ChatSendOptions
}

export interface MinimalComputerProgress {
  status: ComputerStatus
  statusMessage: string | null
  error: string | null
  lastThought: string | null
  lastAction: string | null
  stepCount: number
  pendingSteer?: string | null
  lastUserUpdate?: string | null
}

export interface MinimalComputerStart {
  task: string
  provider: string
  model: string
  attachments?: ComputerAttachment[]
}

export interface MinimalComputerReply {
  text: string
}

export interface MinimalComputerSteer {
  text: string
  attachments?: ComputerAttachment[]
}

export interface MinimalRecorderProgress {
  status: RecorderStatus
  error: string | null
  statusMessage: string | null
  stepCount: number
  lastActionType: string | null
  lastActionValue: string | null
  savedRecordingName: string | null
  savedSkillName: string | null
  startedAt: number | null
}

export interface MinimalRecorderControl {
  action: 'start' | 'stop' | 'discard' | 'generate' | 'reset'
  task?: string
}

export interface MinimalComputerReplay {
  name: string
  mode: 'smart' | 'exact'
  useSkill?: boolean
  inputs?: string
  provider?: string
  model?: string
}

export async function emitMinimalEvent(name: string, payload?: unknown): Promise<void> {
  try {
    const { emit } = await import('@tauri-apps/api/event')
    await emit(name, payload)
  } catch (err) {
    console.warn(`[minimal] emit ${name} failed`, err)
  }
}

export function isMinimalWindow(): boolean {
  if (typeof window === 'undefined') return false
  try {
    if (/(?:^|[?&])minimal=1(?:&|$)/.test(window.location.search)) return true
  } catch {

  }
  try {
    if (window.location.hash.replace(/^#/, '').split('?')[0] === MINIMAL_WINDOW_LABEL) return true
  } catch {

  }
  try {
    const internals = (window as unknown as {
      __TAURI_INTERNALS__?: { metadata?: { currentWindow?: { label?: unknown } } }
    }).__TAURI_INTERNALS__
    return internals?.metadata?.currentWindow?.label === MINIMAL_WINDOW_LABEL
  } catch {
    return false
  }
}

export function currentActiveSession(): MinimalActiveSession | null {
  try {
    const state = useTabStore.getState()
    const id = state.activeTabId
    if (!id || id === SCHEDULED_TAB_ID) return null
    const tab = state.tabs.find((t) => t.sessionId === id)
    if (tab && tab.type !== 'session') return null
    return { id, title: tab?.title ?? null }
  } catch {
    return null
  }
}

export async function enterMinimalMode(variant?: MinimalVariant): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const [{ WebviewWindow }, { emit }] = await Promise.all([
      import('@tauri-apps/api/webviewWindow'),
      import('@tauri-apps/api/event'),
    ])

    const minimal = await WebviewWindow.getByLabel(MINIMAL_WINDOW_LABEL)
    if (!minimal) {
      notifyMinimal('error', t('minimal.error.windowMissing'))
      return
    }

    const resolvedVariant: MinimalVariant = variant ?? useUIStore.getState().appMode

    const active = currentActiveSession()
    try {
      const payload: MinimalActivatePayload = {
        variant: resolvedVariant,
        sessionId: active?.id ?? '',
      }
      await emit(MINIMAL_EVENT_ACTIVATE, payload)
      await emit(MINIMAL_EVENT_ACTIVE_SESSION, active)
    } catch (err) {
      console.warn('[minimalMode] emit activate failed', err)
    }

    try {
      await minimal.show()
      await minimal.setFocus()
      await hideMainWindow()
      void prewarmMinimalInputWindow()
    } catch (err) {
      console.warn('[minimalMode] reveal minimal window failed', err)
      notifyMinimal(
        'error',
        t('minimal.error.enterFailed', {
          error: err instanceof Error ? err.message : String(err),
        }),
      )
    }
  } catch (err) {
    console.warn('[minimalMode] enterMinimalMode failed', err)
    notifyMinimal(
      'error',
      t('minimal.error.enterFailed', {
        error: err instanceof Error ? err.message : String(err),
      }),
    )
  }
}

async function hideMainWindow(): Promise<void> {
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    const current = getCurrentWindow()
    if (current.label === MAIN_WINDOW_LABEL) {
      await current.hide()
      return
    }
    const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow')
    const main = await WebviewWindow.getByLabel(MAIN_WINDOW_LABEL)
    await main?.hide()
  } catch (err) {
    console.warn('[minimalMode] hide main window failed', err)
  }
}

export async function showMinimalInput(variant: MinimalVariant): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const { emit } = await import('@tauri-apps/api/event')
    const active = currentActiveSession()
    const payload: MinimalActivatePayload = {
      variant,
      sessionId: active?.id ?? '',
    }
    await emit(MINIMAL_EVENT_ACTIVATE, payload).catch(() => {})
    await emit(MINIMAL_EVENT_ACTIVE_SESSION, active).catch(() => {})
    await emit(MINIMAL_EVENT_INPUT_SHOW, { variant })
  } catch (err) {
    console.warn('[minimalMode] showMinimalInput failed', err)
  }
}

export async function revealMinimalInputWindow(variant: MinimalVariant): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    const size = MINIMAL_INPUT_SIZE[variant] ?? MINIMAL_INPUT_SIZE.code
    await invoke('minimal_input_show', {
      width: size.width,
      height: size.height,
    })
  } catch (err) {
    console.warn('[minimalMode] revealMinimalInputWindow failed', err)
  }
}

export async function prewarmMinimalInputWindow(): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('minimal_input_prewarm')
  } catch (err) {
    console.warn('[minimalMode] prewarmMinimalInputWindow failed', err)
  }
}

export async function hideMinimalInput(): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const { invoke } = await import('@tauri-apps/api/core')
    await invoke('minimal_input_hide')
  } catch (err) {
    console.warn('[minimalMode] hideMinimalInput failed', err)
  }
}

export async function exitMinimalMode(): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const [{ WebviewWindow }, { getCurrentWindow }] = await Promise.all([
      import('@tauri-apps/api/webviewWindow'),
      import('@tauri-apps/api/window'),
    ])
    await hideMinimalInput()
    const main = await WebviewWindow.getByLabel(MAIN_WINDOW_LABEL)
    if (main) {
      try {
        if (await main.isMinimized()) await main.unminimize()
      } catch {

      }
      await main.show()
      await main.setFocus()
    }
    const minimal = await WebviewWindow.getByLabel(MINIMAL_WINDOW_LABEL)
    await minimal?.hide()
    const current = getCurrentWindow()
    if (current.label !== MAIN_WINDOW_LABEL && current.label !== MINIMAL_WINDOW_LABEL) {
      await current.hide()
    }
  } catch (err) {
    console.warn('[minimalMode] exitMinimalMode failed', err)
  }
}

export async function hideMinimalToTray(): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window')
    await hideMinimalInput()
    await getCurrentWindow().hide()
  } catch (err) {
    console.warn('[minimalMode] hideMinimalToTray failed', err)
  }
}

export async function requestMainSettings(): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const [{ WebviewWindow }, { emit }, { getCurrentWindow }] = await Promise.all([
      import('@tauri-apps/api/webviewWindow'),
      import('@tauri-apps/api/event'),
      import('@tauri-apps/api/window'),
    ])
    await hideMinimalInput()
    const main = await WebviewWindow.getByLabel(MAIN_WINDOW_LABEL)
    if (main) {
      try {
        if (await main.isMinimized()) await main.unminimize()
      } catch {

      }
      await main.show()
      await main.setFocus()
    }
    await emit(MINIMAL_EVENT_OPEN_SETTINGS)
    await getCurrentWindow().hide()
  } catch (err) {
    console.warn('[minimalMode] requestMainSettings failed', err)
  }
}

const MINIMAL_MARGIN_X = 24
const MINIMAL_MARGIN_BOTTOM = 56

export async function positionMinimalInitial(): Promise<void> {
  if (!isTauriRuntime()) return
  try {
    const { getCurrentWindow, LogicalSize, PhysicalPosition, currentMonitor } = await import(
      '@tauri-apps/api/window'
    )
    const win = getCurrentWindow()
    await win.setSize(new LogicalSize(MINIMAL_COLLAPSED_WIDTH, MINIMAL_COLLAPSED_HEIGHT))
    const monitor = await currentMonitor()
    if (!monitor) return
    const scale = monitor.scaleFactor || 1
    const widthPhys = MINIMAL_COLLAPSED_WIDTH * scale
    const heightPhys = MINIMAL_COLLAPSED_HEIGHT * scale
    const marginXPhys = MINIMAL_MARGIN_X * scale
    const marginBottomPhys = MINIMAL_MARGIN_BOTTOM * scale
    const x = monitor.position.x + monitor.size.width - widthPhys - marginXPhys
    const y = monitor.position.y + monitor.size.height - heightPhys - marginBottomPhys
    await win.setPosition(new PhysicalPosition(Math.round(x), Math.round(y)))
  } catch (err) {
    console.warn('[minimalMode] positionMinimalInitial failed', err)
  }
}

let resizeChain: Promise<void> = Promise.resolve()
let resizeSeq = 0

export async function resizeMinimalWindow(
  widthLogical: number,
  heightLogical: number,
): Promise<void> {
  if (!isTauriRuntime()) return
  const seq = ++resizeSeq
  resizeChain = resizeChain.then(async () => {
    if (seq !== resizeSeq) return
    try {
      const { invoke } = await import('@tauri-apps/api/core')
      await invoke('minimal_resize_anchored', {
        width: widthLogical,
        height: heightLogical,
      })
    } catch (err) {
      console.warn('[minimalMode] resizeMinimalWindow failed', err)
    }
  })
  return resizeChain
}
