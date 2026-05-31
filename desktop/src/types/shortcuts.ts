// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

export type ShortcutActionId =
  | 'new-session'
  | 'sidebar-search'
  | 'stop-generation'
  | 'toggle-terminal'
  | 'quick-mode-switcher'
  | 'mode-plan'
  | 'close-modal'

export type ShortcutBinding = {
  ctrl: boolean
  shift: boolean
  alt: boolean
  key: string
}

export type ShortcutBindings = Record<ShortcutActionId, ShortcutBinding>

export const SHORTCUT_ACTIONS: ShortcutActionId[] = [
  'new-session',
  'sidebar-search',
  'stop-generation',
  'toggle-terminal',
  'quick-mode-switcher',
  'mode-plan',
  'close-modal',
]

export const DEFAULT_SHORTCUT_BINDINGS: ShortcutBindings = {
  'new-session': { ctrl: true, shift: false, alt: false, key: 'N' },
  'sidebar-search': { ctrl: true, shift: false, alt: false, key: 'K' },
  'stop-generation': { ctrl: true, shift: false, alt: false, key: '.' },
  'toggle-terminal': { ctrl: true, shift: false, alt: false, key: '`' },
  'quick-mode-switcher': { ctrl: true, shift: true, alt: false, key: 'M' },
  'mode-plan': { ctrl: true, shift: true, alt: false, key: 'P' },
  'close-modal': { ctrl: false, shift: false, alt: false, key: 'Escape' },
}

export function normalizeShortcutKey(rawKey: string): string {
  if (rawKey.length === 1) return rawKey.toUpperCase()
  return rawKey
}

export function matchesBinding(e: KeyboardEvent, b: ShortcutBinding): boolean {
  const meta = e.metaKey || e.ctrlKey
  if (b.ctrl !== meta) return false
  if (b.shift !== e.shiftKey) return false
  if (b.alt !== e.altKey) return false
  const eventKey = normalizeShortcutKey(e.key)
  return eventKey === b.key
}

export function bindingFromKeyboardEvent(e: KeyboardEvent): ShortcutBinding | null {
  const key = normalizeShortcutKey(e.key)
  if (
    key === 'Control' ||
    key === 'Shift' ||
    key === 'Alt' ||
    key === 'Meta' ||
    key === 'CapsLock' ||
    key === 'Tab'
  ) {
    return null
  }
  return {
    ctrl: e.metaKey || e.ctrlKey,
    shift: e.shiftKey,
    alt: e.altKey,
    key,
  }
}

export function formatBinding(b: ShortcutBinding): string {
  const parts: string[] = []
  if (b.ctrl) parts.push('Ctrl')
  if (b.shift) parts.push('Shift')
  if (b.alt) parts.push('Alt')
  let display = b.key
  if (display === ' ') display = 'Space'
  if (display === '`') display = '`'
  parts.push(display)
  return parts.join(' + ')
}

export function bindingsEqual(a: ShortcutBinding, b: ShortcutBinding): boolean {
  return a.ctrl === b.ctrl && a.shift === b.shift && a.alt === b.alt && a.key === b.key
}
