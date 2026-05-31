// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import {
  DEFAULT_SHORTCUT_BINDINGS,
  type ShortcutActionId,
  type ShortcutBinding,
  type ShortcutBindings,
} from '../types/shortcuts'

const STORAGE_KEY = 'keyboard-shortcuts-v1'

function loadStoredBindings(): ShortcutBindings {
  const merged: ShortcutBindings = { ...DEFAULT_SHORTCUT_BINDINGS }
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return merged
    const parsed = JSON.parse(raw) as Partial<Record<ShortcutActionId, ShortcutBinding>>
    for (const id of Object.keys(merged) as ShortcutActionId[]) {
      const stored = parsed[id]
      if (
        stored &&
        typeof stored === 'object' &&
        typeof stored.ctrl === 'boolean' &&
        typeof stored.shift === 'boolean' &&
        typeof stored.alt === 'boolean' &&
        typeof stored.key === 'string' &&
        stored.key.length > 0
      ) {
        merged[id] = stored
      }
    }
  } catch {
    return merged
  }
  return merged
}

function persistBindings(bindings: ShortcutBindings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(bindings))
  } catch {
    return
  }
}

type KeyboardShortcutsStore = {
  bindings: ShortcutBindings
  setBinding: (id: ShortcutActionId, binding: ShortcutBinding) => void
  resetBinding: (id: ShortcutActionId) => void
  resetAll: () => void
}

export const useKeyboardShortcutsStore = create<KeyboardShortcutsStore>((set, get) => ({
  bindings: loadStoredBindings(),

  setBinding: (id, binding) => {
    const next = { ...get().bindings, [id]: binding }
    persistBindings(next)
    set({ bindings: next })
  },

  resetBinding: (id) => {
    const next = { ...get().bindings, [id]: DEFAULT_SHORTCUT_BINDINGS[id] }
    persistBindings(next)
    set({ bindings: next })
  },

  resetAll: () => {
    const next = { ...DEFAULT_SHORTCUT_BINDINGS }
    persistBindings(next)
    set({ bindings: next })
  },
}))
