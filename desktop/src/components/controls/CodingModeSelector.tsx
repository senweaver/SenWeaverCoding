import { useState, useRef, useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useSettingsStore } from '../../stores/settingsStore'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation } from '../../i18n'
import { useDockSuspend } from '../../hooks/useDockSuspend'
import type { CodingModeId } from '../../types/codingMode'
import { isVisibleCodingMode } from '../../types/codingMode'
import type { TranslationKey } from '../../i18n'

type Props = {

  value?: CodingModeId
  onChange?: (mode: CodingModeId) => void
}

const FALLBACK_MODES: { id: CodingModeId; label: string; descriptionKey: TranslationKey }[] = [
  { id: 'agent', label: 'Agent', descriptionKey: 'codingMode.agent.description' },
  { id: 'spec', label: 'Spec', descriptionKey: 'codingMode.spec.description' },
  { id: 'plan', label: 'Plan', descriptionKey: 'codingMode.plan.description' },
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
  const [pendingAutonomous, setPendingAutonomous] = useState<CodingModeId | null>(null)
  useDockSuspend(pendingAutonomous !== null)
  const ref = useRef<HTMLDivElement>(null)

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

  useEffect(() => {
    if (!open) return
    const handleClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
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

  function applyMode(modeId: CodingModeId) {
    if (isControlled) {
      onChange?.(modeId)
    } else {
      void requestSetCodingMode(modeId)
      if (activeTabId) setSessionCodingMode(activeTabId, modeId)
    }
  }

  function handleSelect(modeId: CodingModeId) {
    const targetItem = items.find((i) => i.id === modeId)
    const isAutonomousTarget = targetItem
      ? targetItem.isAutonomous
      : FALLBACK_AUTONOMOUS_MODES.has(modeId)
    if (isAutonomousTarget && modeId !== currentMode) {
      setOpen(false)
      setPendingAutonomous(modeId)
      return
    }
    applyMode(modeId)
    setOpen(false)
  }

  const isPlan = currentMode === 'plan'
  const triggerClass = isPlan
    ? 'flex items-center gap-1 rounded-full bg-[var(--color-plan-accent-container)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-on-plan-accent-container)] transition-colors hover:brightness-95'
    : 'flex items-center gap-1 rounded-full bg-[var(--color-surface-container-low)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]'

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen(!open)}
        className={triggerClass}
        title={t('codingMode.selectorTitle')}
      >
        <span className="material-symbols-outlined text-[12px]">
          {MODE_BADGE_GLYPH[currentMode] ?? 'tune'}
        </span>
        <span>{currentLabel}</span>
        <span className="material-symbols-outlined text-[11px]">expand_more</span>
      </button>

      {open && (
        <div className="absolute left-0 bottom-full mb-2 w-[220px] max-h-[480px] overflow-y-auto rounded-xl bg-[var(--color-surface-container-lowest)] border border-[var(--color-border)] shadow-[var(--shadow-dropdown)] z-50 py-2">
          <div className="px-3 py-1.5 text-[10px] font-bold uppercase tracking-widest text-[var(--color-outline)]">
            {t('codingMode.title')}
          </div>
          {items.map((item) => {
            const readOnly = item.isReadOnly
            const autonomous = item.isAutonomous
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
                  className={`material-symbols-outlined text-[18px] mt-0.5 shrink-0 ${
                    autonomous
                      ? 'text-[var(--color-error)]'
                      : readOnly
                      ? 'text-[var(--color-text-tertiary)]'
                      : 'text-[var(--color-text-secondary)]'
                  }`}
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
        </div>
      )}

      {pendingAutonomous &&
        createPortal(
          <div
            className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40 pl-[var(--sidebar-width)]"
            onClick={() => setPendingAutonomous(null)}
          >
            <div
              className="w-[420px] rounded-2xl bg-[var(--color-surface-container-lowest)] border border-[var(--color-border)] shadow-[var(--shadow-dropdown)] overflow-hidden"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="flex items-center gap-3 px-5 py-4 bg-[var(--color-error)]/8 border-b border-[var(--color-error)]/15">
                <div className="flex items-center justify-center w-10 h-10 rounded-xl bg-[var(--color-error)]/12">
                  <span className="material-symbols-outlined text-[22px] text-[var(--color-error)]">
                    warning
                  </span>
                </div>
                <div>
                  <div className="text-sm font-bold text-[var(--color-text-primary)]">
                    {t('codingMode.confirmAutonomousTitle')}
                  </div>
                  <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
                    {t('codingMode.confirmAutonomousSubtitle')}
                  </div>
                </div>
              </div>
              <div className="px-5 py-4 text-xs text-[var(--color-text-secondary)] leading-relaxed">
                {t('codingMode.confirmAutonomousBody')}
              </div>
              <div className="flex items-center justify-end gap-2 px-5 py-3 border-t border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
                <button
                  onClick={() => setPendingAutonomous(null)}
                  className="px-4 py-2 text-xs font-semibold text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] rounded-lg transition-colors"
                >
                  {t('common.cancel')}
                </button>
                <button
                  onClick={() => {
                    if (pendingAutonomous) applyMode(pendingAutonomous)
                    setPendingAutonomous(null)
                  }}
                  className="px-4 py-2 text-xs font-semibold text-white bg-[var(--color-error)] hover:opacity-90 rounded-lg transition-colors"
                >
                  {t('codingMode.confirmAutonomousBtn')}
                </button>
              </div>
            </div>
          </div>,
          document.body,
        )}
    </div>
  )
}
