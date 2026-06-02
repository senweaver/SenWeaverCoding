// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState, useRef, useEffect, useCallback } from 'react'
import { createPortal } from 'react-dom'
import { useSettingsStore } from '../../stores/settingsStore'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation } from '../../i18n'
import type { CodingModeId } from '../../types/codingMode'
import { isVisibleCodingMode, CODING_MODE_ACCENT } from '../../types/codingMode'
import type { TranslationKey } from '../../i18n'

type Props = {

  value?: CodingModeId
  onChange?: (mode: CodingModeId) => void
}

const FALLBACK_MODES: { id: CodingModeId; label: string; descriptionKey: TranslationKey }[] = [
  { id: 'agent', label: 'Agent', descriptionKey: 'codingMode.agent.description' },
  { id: 'spec', label: 'Spec', descriptionKey: 'codingMode.spec.description' },
  { id: 'plan', label: 'Plan', descriptionKey: 'codingMode.plan.description' },
  { id: 'curator', label: 'Curator', descriptionKey: 'codingMode.curator.description' },
  { id: 'ask', label: 'Ask', descriptionKey: 'codingMode.ask.description' },
  { id: 'debug', label: 'Debug', descriptionKey: 'codingMode.debug.description' },
  { id: 'harness', label: 'Harness', descriptionKey: 'codingMode.harness.description' },
]

const MODE_BADGE_GLYPH: Record<CodingModeId, string> = {
  vibe: 'bolt',
  agent: 'robot_2',
  spec: 'description',
  plan: 'architecture',
  ask: 'help',
  tdd: 'science',
  debug: 'bug_report',
  architect: 'design_services',
  pair: 'group',
  context: 'data_object',
  mvai: 'hub',
  harness: 'precision_manufacturing',
  curator: 'auto_stories',
}


const FALLBACK_READONLY_MODES = new Set<CodingModeId>(['ask'])

const FALLBACK_AUTONOMOUS_MODES = new Set<CodingModeId>(['agent', 'harness'])

export function CodingModeSelector({ value, onChange }: Props = {}) {
  const t = useTranslation()
  const storeMode = useSettingsStore((s) => s.codingMode)
  const requestSetCodingMode = useSettingsStore((s) => s.requestSetCodingMode)
  const codingModes = useSettingsStore((s) => s.codingModes)
  const setSessionCodingMode = useChatStore((s) => s.setSessionCodingMode)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const [dropdownPos, setDropdownPos] = useState<{
    top: number
    left: number
    direction: 'up' | 'down'
  } | null>(null)

  const isControlled = value !== undefined
  const currentMode: CodingModeId = isControlled ? value : storeMode

  const sourceModes =
    codingModes.length > 0
      ? codingModes.map((m) => ({ id: m.id, permissionMode: m.permissionMode }))
      : FALLBACK_MODES.map((m) => ({ id: m.id, permissionMode: undefined }))

  const items = sourceModes
    .filter((m) => isVisibleCodingMode(m.id))
    .map((m) => {
      const isAutonomous =
        m.permissionMode !== undefined
          ? m.permissionMode === 'bypassPermissions' || m.permissionMode === 'dontAsk'
          : FALLBACK_AUTONOMOUS_MODES.has(m.id)
      const isReadOnly =
        m.permissionMode !== undefined
          ? m.permissionMode === 'plan' && m.id === 'ask'
          : FALLBACK_READONLY_MODES.has(m.id)
      return {
        id: m.id,
        label: t(`codingMode.${m.id}.label` as TranslationKey),
        description: t(`codingMode.${m.id}.description` as TranslationKey),
        isAutonomous,
        isReadOnly,
      }
    })

  const currentLabel = items.find((i) => i.id === currentMode)?.label ?? currentMode

  const updateDropdownPos = useCallback(() => {
    if (!triggerRef.current) return
    const rect = triggerRef.current.getBoundingClientRect()
    const DROPDOWN_HEIGHT = 480
    const spaceAbove = rect.top
    const spaceBelow = window.innerHeight - rect.bottom
    const direction = spaceBelow >= DROPDOWN_HEIGHT || spaceBelow >= spaceAbove ? 'down' : 'up'
    setDropdownPos({
      top: direction === 'down' ? rect.bottom + 4 : rect.top - 4,
      left: rect.left,
      direction,
    })
  }, [])

  useEffect(() => {
    if (!open) return
    const handleClick = (e: MouseEvent) => {
      const target = e.target as Node
      if (ref.current?.contains(target)) return
      if (menuRef.current?.contains(target)) return
      setOpen(false)
    }
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    document.addEventListener('keydown', handleEsc)
    return () => {
      document.removeEventListener('mousedown', handleClick)
      document.removeEventListener('keydown', handleEsc)
    }
  }, [open])

  useEffect(() => {
    if (!open) return
    updateDropdownPos()
    window.addEventListener('scroll', updateDropdownPos, true)
    window.addEventListener('resize', updateDropdownPos)
    return () => {
      window.removeEventListener('scroll', updateDropdownPos, true)
      window.removeEventListener('resize', updateDropdownPos)
    }
  }, [open, updateDropdownPos])

  function applyMode(modeId: CodingModeId) {
    if (isControlled) {
      onChange?.(modeId)
    } else {
      void requestSetCodingMode(modeId)
      if (activeTabId) setSessionCodingMode(activeTabId, modeId)
    }
  }

  function handleSelect(modeId: CodingModeId) {
    applyMode(modeId)
    setOpen(false)
  }

  const accentTokens = CODING_MODE_ACCENT[currentMode]
  const triggerClass = accentTokens
    ? 'flex items-center gap-1 rounded-full px-2 py-0.5 text-[11px] font-medium transition-colors hover:brightness-95'
    : 'flex items-center gap-1 rounded-full bg-[var(--color-surface-container-low)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]'
  const triggerStyle = accentTokens
    ? { backgroundColor: accentTokens.container, color: accentTokens.onContainer }
    : undefined

  return (
    <div ref={ref} className="relative">
      <button
        ref={triggerRef}
        onClick={() => setOpen(!open)}
        className={triggerClass}
        style={triggerStyle}
        title={t('codingMode.selectorTitle')}
      >
        <span className="material-symbols-outlined text-[12px]">
          {MODE_BADGE_GLYPH[currentMode] ?? 'tune'}
        </span>
        <span>{currentLabel}</span>
        <span className="material-symbols-outlined text-[11px]">expand_more</span>
      </button>

      {open && dropdownPos && createPortal(
        <div
          ref={menuRef}
          role="menu"
          className="w-[220px] max-h-[480px] overflow-y-auto rounded-xl bg-[var(--color-surface-container-lowest)] border border-[var(--color-border)] shadow-[var(--shadow-dropdown)] py-2"
          style={{
            position: 'fixed',
            left: dropdownPos.left,
            ...(dropdownPos.direction === 'down'
              ? { top: dropdownPos.top }
              : { bottom: window.innerHeight - dropdownPos.top }),
            zIndex: 9999,
          }}
        >
          <div className="px-3 py-1.5 text-[10px] font-bold uppercase tracking-widest text-[var(--color-outline)]">
            {t('codingMode.title')}
          </div>
          {items.map((item) => {
            const readOnly = item.isReadOnly
            const autonomous = item.isAutonomous
            const itemAccent = CODING_MODE_ACCENT[item.id]
            const iconColorClass = itemAccent
              ? ''
              : autonomous
                ? 'text-[var(--color-error)]'
                : readOnly
                  ? 'text-[var(--color-text-tertiary)]'
                  : 'text-[var(--color-text-secondary)]'
            return (
              <button
                key={item.id}
                onClick={() => handleSelect(item.id)}
                className={`
                  w-full flex items-start gap-2 px-3 py-2 text-left transition-colors
                  hover:bg-[var(--color-surface-hover)]
                  ${item.id === currentMode ? 'bg-[var(--color-surface-selected)]' : ''}
                `}
              >
                <span
                  className={`material-symbols-outlined text-[18px] mt-0.5 shrink-0 ${iconColorClass}`}
                  style={itemAccent ? { color: itemAccent.accent } : undefined}
                >
                  {MODE_BADGE_GLYPH[item.id] ?? 'tune'}
                </span>
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-semibold text-[var(--color-text-primary)] flex items-center gap-1.5">
                    <span className="truncate" title={item.label}>
                      {item.label}
                    </span>
                    {autonomous && (
                      <span className="shrink-0 text-[9px] uppercase tracking-wider px-1.5 py-0.5 rounded-full bg-[var(--color-error)]/12 text-[var(--color-error)]">
                        {t('codingMode.tag.autonomous')}
                      </span>
                    )}
                    {readOnly && (
                      <span className="shrink-0 text-[9px] uppercase tracking-wider px-1.5 py-0.5 rounded-full bg-[var(--color-surface-container)] text-[var(--color-text-tertiary)]">
                        {t('codingMode.tag.readOnly')}
                      </span>
                    )}
                  </div>
                  <div
                    className="text-xs text-[var(--color-text-tertiary)] mt-0.5 truncate"
                    title={item.description}
                  >
                    {item.description}
                  </div>
                </div>
                {item.id === currentMode && (
                  <span
                    className="material-symbols-outlined text-[16px] text-[var(--color-brand)] mt-0.5 shrink-0"
                    style={{ fontVariationSettings: "'FILL' 1" }}
                  >
                    check_circle
                  </span>
                )}
              </button>
            )
          })}
        </div>,
        document.body,
      )}

    </div>
  )
}
