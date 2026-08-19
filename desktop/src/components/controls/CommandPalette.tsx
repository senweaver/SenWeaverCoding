// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useShallow } from 'zustand/react/shallow'
import { useUIStore } from '../../stores/uiStore'
import { useKeyboardShortcutsStore } from '../../stores/keyboardShortcutsStore'
import { useTranslation } from '../../i18n'
import { useDockSuspend } from '../../hooks/useDockSuspend'
import { paletteCommands } from '../../lib/commandRegistry'
import { formatBinding } from '../../types/shortcuts'

export function CommandPalette() {
  const t = useTranslation()
  const { activeModal, closeModal } = useUIStore(
    useShallow((s) => ({ activeModal: s.activeModal, closeModal: s.closeModal })),
  )
  const bindings = useKeyboardShortcutsStore((s) => s.bindings)
  const [query, setQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([])

  const open = activeModal === 'command-palette'
  useDockSuspend(open)

  useEffect(() => {
    if (!open) return
    setQuery('')
    setSelectedIndex(0)
    requestAnimationFrame(() => inputRef.current?.focus())
  }, [open])

  const commands = useMemo(() => paletteCommands(), [])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return commands
    return commands.filter((command) => {
      const title = t(command.titleKey).toLowerCase()
      return (
        title.includes(q) ||
        command.id.includes(q) ||
        command.keywords.some((keyword) => keyword.includes(q))
      )
    })
  }, [commands, query, t])

  useEffect(() => {
    setSelectedIndex(0)
  }, [query])

  useEffect(() => {
    const activeItem = itemRefs.current[selectedIndex]
    if (activeItem && typeof activeItem.scrollIntoView === 'function') {
      activeItem.scrollIntoView({ block: 'nearest' })
    }
  }, [selectedIndex])

  if (!open) return null

  const runCommand = (index: number) => {
    const command = filtered[index]
    if (!command) return
    closeModal()
    command.run()
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setSelectedIndex((prev) => (filtered.length ? (prev + 1) % filtered.length : 0))
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setSelectedIndex((prev) =>
        filtered.length ? (prev - 1 + filtered.length) % filtered.length : 0,
      )
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      runCommand(selectedIndex)
      return
    }
    if (e.key === 'Escape') {
      e.preventDefault()
      closeModal()
    }
  }

  return createPortal(
    <div
      className="fixed inset-0 z-[120] flex items-start justify-center bg-black/40 pt-[12vh]"
      onClick={closeModal}
    >
      <div
        className="w-[560px] max-h-[70vh] overflow-hidden rounded-2xl bg-[var(--color-surface-container-lowest)] border border-[var(--color-border)] shadow-[var(--shadow-dropdown)]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 px-4 py-3 border-b border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
          <span className="material-symbols-outlined text-[18px] text-[var(--color-text-secondary)]">
            keyboard_command_key
          </span>
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={t('commandPalette.placeholder')}
            className="flex-1 border-none bg-transparent text-sm text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)]"
          />
          <span className="ml-auto text-[10px] uppercase tracking-wider text-[var(--color-text-tertiary)]">
            {t('quickMode.escHint')}
          </span>
        </div>
        <div className="overflow-y-auto max-h-[calc(70vh-52px)] py-2">
          {filtered.length === 0 && (
            <div className="px-4 py-6 text-center text-sm text-[var(--color-text-tertiary)]">
              {t('commandPalette.noResults')}
            </div>
          )}
          {filtered.map((command, index) => {
            const binding = command.shortcutActionId
              ? bindings[command.shortcutActionId]
              : null
            return (
              <button
                key={command.id}
                ref={(el) => {
                  itemRefs.current[index] = el
                }}
                onClick={() => runCommand(index)}
                onMouseEnter={() => setSelectedIndex(index)}
                className={`w-full flex items-center gap-3 px-4 py-2.5 text-left transition-colors ${
                  index === selectedIndex
                    ? 'bg-[var(--color-surface-hover)]'
                    : 'hover:bg-[var(--color-surface-hover)]'
                }`}
              >
                <span className="material-symbols-outlined text-[18px] shrink-0 text-[var(--color-text-secondary)]">
                  {command.icon}
                </span>
                <span className="min-w-0 flex-1 truncate text-sm font-medium text-[var(--color-text-primary)]">
                  {t(command.titleKey)}
                </span>
                {binding && (
                  <span className="shrink-0 rounded-md border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] px-1.5 py-0.5 text-[10px] font-semibold text-[var(--color-text-tertiary)] tabular-nums">
                    {formatBinding(binding)}
                  </span>
                )}
              </button>
            )
          })}
        </div>
      </div>
    </div>,
    document.body,
  )
}
