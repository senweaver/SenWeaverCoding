// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useRef, useState } from 'react'
import { useSettingsStore, PII_KIND_LABELS, type PiiKindLabel } from '../../stores/settingsStore'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation, useCodingModeText, type TranslationKey } from '../../i18n'
import { Button } from '../shared/Button'
import type { CodingModeId } from '../../types/codingMode'
import { sortByCodingModeOrder } from '../../types/codingMode'

export function CodingModeSettings() {
  const codingMode = useSettingsStore((s) => s.codingMode)
  const codingModes = useSettingsStore((s) => s.codingModes)
  const codingModeOrder = useSettingsStore((s) => s.codingModeOrder)
  const setCodingModeOrder = useSettingsStore((s) => s.setCodingModeOrder)
  const requestSetCodingMode = useSettingsStore((s) => s.requestSetCodingMode)
  const permissionMode = useSettingsStore((s) => s.permissionMode)
  const t = useTranslation()
  const tCodingMode = useCodingModeText()

  const MODE_GLYPH: Record<string, string> = {
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
  }

  const modes = useMemo(
    () => sortByCodingModeOrder(codingModes, codingModeOrder),
    [codingModes, codingModeOrder],
  )

  const [dragId, setDragId] = useState<CodingModeId | null>(null)
  const [dragOverId, setDragOverId] = useState<CodingModeId | null>(null)
  const dragStateRef = useRef<{
    id: CodingModeId
    startX: number
    startY: number
    pointerId: number
    active: boolean
  } | null>(null)
  const suppressClickRef = useRef(false)

  const modeIds = modes.map((m) => m.id)

  const findModeAtPoint = (x: number, y: number): CodingModeId | null => {
    const el = document.elementFromPoint(x, y) as HTMLElement | null
    const row = el?.closest('[data-coding-mode-row]') as HTMLElement | null
    const id = row?.getAttribute('data-coding-mode-row')
    return id && modeIds.includes(id as CodingModeId) ? (id as CodingModeId) : null
  }

  const reorder = (sourceId: CodingModeId, targetId: CodingModeId) => {
    if (sourceId === targetId) return
    const fromIdx = modeIds.indexOf(sourceId)
    const toIdx = modeIds.indexOf(targetId)
    if (fromIdx === -1 || toIdx === -1) return
    const next = [...modeIds]
    next.splice(fromIdx, 1)
    next.splice(toIdx, 0, sourceId)
    setCodingModeOrder(next)
  }

  const handlePointerDown = (e: React.PointerEvent, id: CodingModeId) => {
    if (e.button !== 0) return
    suppressClickRef.current = false
    dragStateRef.current = {
      id,
      startX: e.clientX,
      startY: e.clientY,
      pointerId: e.pointerId,
      active: false,
    }
  }

  const handlePointerMove = (e: React.PointerEvent) => {
    const st = dragStateRef.current
    if (!st) return
    if (!st.active) {
      const dx = e.clientX - st.startX
      const dy = e.clientY - st.startY
      if (Math.hypot(dx, dy) < 5) return
      st.active = true
      setDragId(st.id)
      try {
        ;(e.currentTarget as HTMLElement).setPointerCapture(st.pointerId)
      } catch {
      }
    }
    const targetId = findModeAtPoint(e.clientX, e.clientY)
    setDragOverId(targetId && targetId !== st.id ? targetId : null)
  }

  const handlePointerUp = (e: React.PointerEvent) => {
    const st = dragStateRef.current
    dragStateRef.current = null
    if (!st) return
    try {
      ;(e.currentTarget as HTMLElement).releasePointerCapture(st.pointerId)
    } catch {
    }
    if (st.active) {
      suppressClickRef.current = true
      const targetId = findModeAtPoint(e.clientX, e.clientY)
      setDragId(null)
      setDragOverId(null)
      if (targetId) reorder(st.id, targetId)
    }
  }

  return (
    <div>
      <h2 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">
        {t('settings.codingMode.title')}
      </h2>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-2">
        {t('settings.codingMode.description')}
      </p>

      <div className="mb-3 px-3 py-2 rounded-lg bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-xs text-[var(--color-text-tertiary)] flex items-center gap-2">
        <span className="material-symbols-outlined text-[14px]">drag_indicator</span>
        {t('settings.codingMode.reorderHint')}
      </div>

      <div className="mb-3 px-3 py-2 rounded-lg bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-xs text-[var(--color-text-tertiary)] flex items-center gap-2">
        <span className="material-symbols-outlined text-[14px]">shield</span>
        {t('settings.codingMode.derivedPermission', { mode: permissionMode })}
      </div>

      <div className="flex flex-col gap-2">
        {modes.map((m) => {
          const isSelected = codingMode === m.id
          const isDragging = dragId === m.id
          const isDragOver = dragOverId === m.id && dragId !== null && dragId !== m.id
          return (
            <div
              key={m.id}
              role="button"
              tabIndex={0}
              data-coding-mode-row={m.id}
              onClick={() => {
                if (suppressClickRef.current) {
                  suppressClickRef.current = false
                  return
                }
                void requestSetCodingMode(m.id)
              }}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') {
                  e.preventDefault()
                  void requestSetCodingMode(m.id)
                }
              }}
              onPointerDown={(e) => handlePointerDown(e, m.id)}
              onPointerMove={handlePointerMove}
              onPointerUp={handlePointerUp}
              style={{ touchAction: 'none' }}
              className={`flex items-center gap-2 px-3 py-2.5 rounded-xl border cursor-pointer select-none transition-all text-left ${
                isSelected
                  ? 'border-[var(--color-brand)] bg-[var(--color-surface-container)] shadow-[var(--shadow-focus-ring)]'
                  : 'border-[var(--color-border)] hover:border-[var(--color-border-focus)] hover:bg-[var(--color-surface-hover)]'
              }${isDragging ? ' opacity-50' : ''}${
                isDragOver ? ' border-[var(--color-brand)] border-dashed' : ''
              }`}
            >
              <span
                className="material-symbols-outlined text-[18px] text-[var(--color-text-tertiary)] cursor-grab active:cursor-grabbing shrink-0"
                title={t('settings.codingMode.dragHandle')}
              >
                drag_indicator
              </span>
              <span className="material-symbols-outlined text-[18px] text-[var(--color-text-secondary)] shrink-0">
                {MODE_GLYPH[m.id] ?? 'tune'}
              </span>
              <div className="flex-1 min-w-0">
                <div className="text-xs font-semibold text-[var(--color-text-primary)] flex items-center gap-2">
                  {tCodingMode(m.id, 'label', m.label)}
                  <span className="text-[10px] uppercase tracking-wider px-1.5 py-0.5 rounded bg-[var(--color-surface-container-low)] text-[var(--color-text-tertiary)]">
                    {m.permissionMode}
                  </span>
                </div>
                <div className="text-xs text-[var(--color-text-tertiary)]">
                  {tCodingMode(m.id, 'description', m.description ?? '')}
                </div>
              </div>
              {isSelected && (
                <span
                  className="material-symbols-outlined text-[18px] text-[var(--color-brand)] shrink-0"
                  style={{ fontVariationSettings: "'FILL' 1" }}
                >
                  check_circle
                </span>
              )}
            </div>
          )
        })}
        {modes.length === 0 && (
          <div className="text-xs text-[var(--color-text-tertiary)] py-4 text-center">
            {t('settings.codingMode.loading')}
          </div>
        )}
      </div>

      <DebugPrivacySettings />
    </div>
  )
}

function DebugPrivacySettings() {
  const t = useTranslation()
  const piiSanitizer = useSettingsStore((s) => s.piiSanitizer)
  const setPiiEnabled = useSettingsStore((s) => s.setPiiEnabled)
  const setPiiKindEnabled = useSettingsStore((s) => s.setPiiKindEnabled)
  const resetPiiSanitizer = useSettingsStore((s) => s.resetPiiSanitizer)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const sessionStats = useChatStore((s) =>
    activeTabId ? s.sessions[activeTabId]?.debugPiiStats : undefined,
  )
  const resetDebugPiiStats = useChatStore((s) => s.resetDebugPiiStats)

  const disabledSet = useMemo(
    () => new Set<PiiKindLabel>(piiSanitizer.disabledKinds),
    [piiSanitizer.disabledKinds],
  )

  return (
    <div className="mt-6 border-t border-[var(--color-border)] pt-4">
      <h3 className="text-xs font-semibold text-[var(--color-text-primary)] mb-1">
        {t('settings.debugPrivacy.title')}
      </h3>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-1">
        {t('settings.debugPrivacy.description')}
      </p>
      <p className="text-xs text-[var(--color-text-tertiary)] mb-4">
        {t('settings.debugPrivacy.scopeHint')}
      </p>

      <label className="flex items-center justify-between gap-3 mb-3 px-3 py-2 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
        <div className="flex flex-col">
          <span className="text-xs font-medium text-[var(--color-text-primary)]">
            {t('settings.debugPrivacy.enable')}
          </span>
          <span className="text-xs text-[var(--color-text-tertiary)]">
            {t('settings.debugPrivacy.enableHint')}
          </span>
        </div>
        <input
          type="checkbox"
          checked={piiSanitizer.enabled}
          onChange={(e) => setPiiEnabled(e.target.checked)}
          className="h-4 w-4"
        />
      </label>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 mb-3">
        {PII_KIND_LABELS.map((kind) => {
          const enabled = !disabledSet.has(kind)
          return (
            <label
              key={kind}
              className={`flex items-center justify-between gap-2 px-2.5 py-1.5 rounded border text-xs ${
                piiSanitizer.enabled
                  ? 'border-[var(--color-border)] bg-[var(--color-surface)]'
                  : 'border-[var(--color-border)] bg-[var(--color-surface-container-low)] opacity-60'
              }`}
            >
              <span className="text-[var(--color-text-secondary)]">
                {t(`debug.privacy.categories.${kind}` as TranslationKey)}
              </span>
              <input
                type="checkbox"
                disabled={!piiSanitizer.enabled}
                checked={enabled}
                onChange={(e) => setPiiKindEnabled(kind, e.target.checked)}
                className="h-3.5 w-3.5"
              />
            </label>
          )
        })}
      </div>

      <div className="flex items-center justify-between gap-2 mb-3 px-3 py-2 rounded border border-[var(--color-border)] bg-[var(--color-surface-container-low)] text-xs">
        <span className="text-[var(--color-text-secondary)]">
          {t('settings.debugPrivacy.sessionStats')}
        </span>
        <span className="font-mono text-[var(--color-text-primary)]">
          {sessionStats?.total ?? 0}
        </span>
      </div>

      <div className="flex items-center gap-2">
        <Button
          variant="ghost"
          size="sm"
          onClick={() => activeTabId && resetDebugPiiStats(activeTabId)}
          disabled={!activeTabId || (sessionStats?.total ?? 0) === 0}
        >
          {t('settings.debugPrivacy.clearStats')}
        </Button>
        <Button variant="ghost" size="sm" onClick={resetPiiSanitizer}>
          {t('settings.debugPrivacy.resetDefaults')}
        </Button>
      </div>
    </div>
  )
}
