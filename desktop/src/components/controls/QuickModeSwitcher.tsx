import { useEffect } from 'react'
import { createPortal } from 'react-dom'
import { useShallow } from 'zustand/react/shallow'
import { useUIStore } from '../../stores/uiStore'
import { useSettingsStore } from '../../stores/settingsStore'
import { useChatStore } from '../../stores/chatStore'
import { useTabStore } from '../../stores/tabStore'
import { useTranslation } from '../../i18n'
import type { CodingModeId } from '../../types/codingMode'
import type { TranslationKey } from '../../i18n'

const QUICK_MODE_GLYPH: Record<CodingModeId, string> = {
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

const QUICK_MODE_ORDER: CodingModeId[] = [
  'agent',
  'spec',
  'plan',
  'ask',
  'debug',
  'harness',
]

const QUICK_MODE_HOTKEY: Partial<Record<CodingModeId, string>> = {
  agent: '1',
  spec: '2',
  plan: '3',
  ask: '4',
  debug: '5',
  harness: '6',
}

const QUICK_MODE_AUTONOMOUS = new Set<CodingModeId>(['agent', 'harness'])
const QUICK_MODE_READONLY = new Set<CodingModeId>(['ask'])

export function QuickModeSwitcher() {
  const t = useTranslation()
  const { activeModal, closeModal } = useUIStore(
    useShallow((s) => ({ activeModal: s.activeModal, closeModal: s.closeModal })),
  )
  const { currentMode, codingModes, requestSetCodingMode } = useSettingsStore(
    useShallow((s) => ({
      currentMode: s.codingMode,
      codingModes: s.codingModes,
      requestSetCodingMode: s.requestSetCodingMode,
    })),
  )
  const setSessionCodingMode = useChatStore((s) => s.setSessionCodingMode)
  const activeTabId = useTabStore((s) => s.activeTabId)

  const open = activeModal === 'quick-mode-switcher'

  const items = QUICK_MODE_ORDER.map((id) => {
    const backendLabel = codingModes.find((m) => m.id === id)?.label
    return {
      id,
      label: t(`codingMode.${id}.label` as TranslationKey) || backendLabel || id,
      description: t(`codingMode.${id}.description` as TranslationKey),
      hotkey: QUICK_MODE_HOTKEY[id],
      autonomous: QUICK_MODE_AUTONOMOUS.has(id),
      readOnly: QUICK_MODE_READONLY.has(id),
    }
  })

  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => {
      const key = e.key.length === 1 ? e.key.toUpperCase() : e.key
      const target = items.find((it) => it.hotkey === key)
      if (target) {
        e.preventDefault()
        e.stopPropagation()
        void requestSetCodingMode(target.id)
        if (activeTabId) setSessionCodingMode(activeTabId, target.id)
        closeModal()
      }
    }
    document.addEventListener('keydown', handler, { capture: true })
    return () =>
      document.removeEventListener('keydown', handler, {
        capture: true,
      } as EventListenerOptions)
  }, [open, items, requestSetCodingMode, setSessionCodingMode, activeTabId, closeModal])

  if (!open) return null

  return createPortal(
    <div
      className="fixed inset-0 z-[120] flex items-start justify-center bg-black/40 pt-[12vh]"
      onClick={closeModal}
    >
      <div
        className="w-[480px] max-h-[70vh] overflow-hidden rounded-2xl bg-[var(--color-surface-container-lowest)] border border-[var(--color-border)] shadow-[var(--shadow-dropdown)]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 px-4 py-3 border-b border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
          <span className="material-symbols-outlined text-[18px] text-[var(--color-text-secondary)]">
            tune
          </span>
          <span className="text-sm font-bold text-[var(--color-text-primary)]">
            {t('quickMode.title')}
          </span>
          <span className="ml-auto text-[10px] uppercase tracking-wider text-[var(--color-text-tertiary)]">
            {t('quickMode.escHint')}
          </span>
        </div>
        <div className="overflow-y-auto max-h-[calc(70vh-44px)] py-2">
          {items.map((item) => {
            const selected = item.id === currentMode
            return (
              <button
                key={item.id}
                onClick={() => {
                  void requestSetCodingMode(item.id)
                  if (activeTabId) setSessionCodingMode(activeTabId, item.id)
                  closeModal()
                }}
                className={`w-full flex items-start gap-3 px-4 py-2.5 text-left transition-colors hover:bg-[var(--color-surface-hover)] ${
                  selected ? 'bg-[var(--color-surface-selected)]' : ''
                }`}
              >
                <span className="shrink-0 w-7 h-7 inline-flex items-center justify-center rounded-md border border-[var(--color-outline-variant)]/40 bg-[var(--color-surface-container)] text-[11px] font-bold text-[var(--color-text-secondary)] tabular-nums">
                  {item.hotkey}
                </span>
                <span
                  className={`material-symbols-outlined text-[20px] mt-0.5 shrink-0 ${
                    item.autonomous
                      ? 'text-[var(--color-error)]'
                      : item.readOnly
                      ? 'text-[var(--color-text-tertiary)]'
                      : 'text-[var(--color-text-secondary)]'
                  }`}
                >
                  {QUICK_MODE_GLYPH[item.id]}
                </span>
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-semibold text-[var(--color-text-primary)] flex items-center gap-1.5">
                    <span className="truncate">
                      {item.label}
                    </span>
                    {item.autonomous && (
                      <span className="shrink-0 text-[9px] uppercase tracking-wider px-1.5 py-0.5 rounded-full bg-[var(--color-error)]/12 text-[var(--color-error)]">
                        {t('codingMode.tag.autonomous')}
                      </span>
                    )}
                    {item.readOnly && (
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
                {selected && (
                  <span
                    className="material-symbols-outlined text-[18px] text-[var(--color-brand)] mt-0.5 shrink-0"
                    style={{ fontVariationSettings: "'FILL' 1" }}
                  >
                    check_circle
                  </span>
                )}
              </button>
            )
          })}
        </div>
        <div className="flex items-center justify-between gap-2 px-4 py-2 border-t border-[var(--color-border)] bg-[var(--color-surface-container-low)] text-[10px] text-[var(--color-text-tertiary)]">
          <span>{t('quickMode.footerKeys')}</span>
          <span>{t('quickMode.footerHint')}</span>
        </div>
      </div>
    </div>,
    document.body,
  )
}
