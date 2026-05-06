import { useMemo, useState, useRef, useEffect } from 'react'
import DOMPurify from 'dompurify'
import { createPortal } from 'react-dom'
import { useTranslation } from '../../i18n'
import type { PermissionMode } from '../../types/settings'

const MODE_ICONS: Record<PermissionMode, string> = {
  default: 'verified_user',
  acceptEdits: 'bolt',
  plan: 'architecture',
  bypassPermissions: 'gavel',
  dontAsk: 'gavel',
  askEveryTime: 'help',
}

type Props = {

  value: PermissionMode

  onChange: (mode: PermissionMode) => void

  workDir?: string
}

const PERMISSION_ROWS: PermissionMode[] = ['default', 'acceptEdits', 'plan', 'bypassPermissions']

export function PermissionModeSelector({ value, onChange, workDir }: Props) {
  const t = useTranslation()
  const [open, setOpen] = useState(false)
  const [confirmDialog, setConfirmDialog] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  const rowMeta = useMemo(
    () =>
      ({
        default: {
          label: t('permMode.askPermissions'),
          description: t('permMode.askPermDesc'),
          icon: 'verified_user',
        },
        acceptEdits: {
          label: t('permMode.autoAccept'),
          description: t('permMode.autoAcceptDesc'),
          icon: 'bolt',
        },
        plan: {
          label: t('permMode.planMode'),
          description: t('permMode.planModeDesc'),
          icon: 'architecture',
          color: 'text-[var(--color-text-tertiary)]',
        },
        bypassPermissions: {
          label: t('permMode.bypass'),
          description: t('permMode.bypassDesc'),
          icon: 'gavel',
          color: 'text-[var(--color-error)]',
        },
      }) as Record<
        (typeof PERMISSION_ROWS)[number],
        { label: string; description: string; icon: string; color?: string }
      >,
    [t],
  )

  const MODE_LABELS: Record<PermissionMode, string> = {
    default: t('permMode.label.default'),
    acceptEdits: t('permMode.label.acceptEdits'),
    plan: t('permMode.label.plan'),
    bypassPermissions: t('permMode.label.bypassPermissions'),
    dontAsk: t('permMode.label.dontAsk'),
    askEveryTime: t('settings.agents.autoRun.opt.askEveryTime'),
  }

  const displayWorkDir = workDir?.trim() || '~'

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

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen(!open)}
        className="flex w-[220px] shrink-0 items-center gap-1.5 rounded-full bg-[var(--color-surface-container-low)] px-2.5 py-0.5 text-[11px] font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]"
      >
        <span className="material-symbols-outlined shrink-0 text-[12px]">{MODE_ICONS[value]}</span>
        <div className="min-w-0 flex-1 truncate text-[12px] font-semibold text-[var(--color-text-primary)]">
          {MODE_LABELS[value]}
        </div>
        <span className="material-symbols-outlined shrink-0 text-[11px]">expand_more</span>
      </button>

      {open && (
        <div className="absolute left-0 bottom-full z-50 mb-2 w-[220px] rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]">
          <div className="max-h-[420px] overflow-y-auto p-2">
            <div className="mb-1.5 px-1 text-[10px] font-bold uppercase tracking-widest text-[var(--color-outline)]">
              {t('permMode.executionPermissions')}
            </div>
            <div className="space-y-0.5">
              {PERMISSION_ROWS.map((perm) => {
                const meta = rowMeta[perm]
                const isSelected = perm === value
                return (
                  <button
                    type="button"
                    key={perm}
                    onClick={() => {
                      if (perm === 'bypassPermissions') {
                        setOpen(false)
                        setConfirmDialog(true)
                        return
                      }
                      onChange(perm)
                      setOpen(false)
                    }}
                    className={`
                      w-full rounded-lg border px-2.5 py-2 text-left transition-colors
                      ${isSelected
                        ? 'border-[var(--color-brand)]/20 bg-[var(--color-primary-fixed)]'
                        : 'border-transparent hover:bg-[var(--color-surface-hover)]'}
                    `}
                  >
                    <div className="flex items-start gap-2">
                      <div
                        className={`mt-0.5 flex h-4 w-4 flex-shrink-0 items-center justify-center rounded-full border-2 ${
                          isSelected ? 'border-[var(--color-brand)]' : 'border-[var(--color-outline)]'
                        }`}
                      >
                        {isSelected && (
                          <div className="h-2 w-2 rounded-full bg-[var(--color-brand)]" />
                        )}
                      </div>
                      <div className="min-w-0 flex-1">
                        <div className="flex items-start gap-2">
                          <span
                            className={`material-symbols-outlined mt-0.5 shrink-0 text-[18px] ${
                              meta.color || 'text-[var(--color-text-secondary)]'
                            }`}
                          >
                            {meta.icon}
                          </span>
                          <div className="min-w-0 flex-1">
                            <div className="text-[13px] font-semibold text-[var(--color-text-primary)]">
                              {meta.label}
                            </div>
                            <div className="mt-0.5 text-[10px] leading-snug text-[var(--color-text-tertiary)]">
                              {meta.description}
                            </div>
                          </div>
                        </div>
                      </div>
                    </div>
                  </button>
                )
              })}
            </div>
          </div>
        </div>
      )}

      {confirmDialog && createPortal(
        <div
          className="fixed inset-0 z-[100] flex items-center justify-center bg-black/40 pl-[var(--sidebar-width)]"
          role="presentation"
          onClick={() => setConfirmDialog(false)}
        >
          <div
            className="w-[420px] rounded-2xl bg-[var(--color-surface-container-lowest)] border border-[var(--color-border)] shadow-[var(--shadow-dropdown)] overflow-hidden"
            role="dialog"
            aria-modal="true"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center gap-3 px-5 py-4 bg-[var(--color-error)]/8 border-b border-[var(--color-error)]/15">
              <div className="flex items-center justify-center w-10 h-10 rounded-xl bg-[var(--color-error)]/12">
                <span className="material-symbols-outlined text-[22px] text-[var(--color-error)]">warning</span>
              </div>
              <div>
                <div className="text-sm font-bold text-[var(--color-text-primary)]">{t('permMode.enableBypassTitle')}</div>
                <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">{t('permMode.enableBypassSubtitle')}</div>
              </div>
            </div>

            <div className="px-5 py-4">
              <p
                className="text-xs text-[var(--color-text-secondary)] leading-relaxed mb-3"
                dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(t('permMode.enableBypassBody')) }}
              />
              <div
                className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--color-surface-container)] border border-[var(--color-border)]"
                title={displayWorkDir}
              >
                <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)] shrink-0">folder</span>
                <code className="text-xs font-[var(--font-mono)] text-[var(--color-text-primary)] truncate">{displayWorkDir}</code>
              </div>
              <ul className="mt-3 space-y-1.5 text-xs text-[var(--color-text-secondary)]">
                <li className="flex items-start gap-2">
                  <span className="material-symbols-outlined text-[14px] text-[var(--color-error)] mt-0.5">check</span>
                  {t('permMode.permReadWrite')}
                </li>
                <li className="flex items-start gap-2">
                  <span className="material-symbols-outlined text-[14px] text-[var(--color-error)] mt-0.5">check</span>
                  {t('permMode.permShell')}
                </li>
                <li className="flex items-start gap-2">
                  <span className="material-symbols-outlined text-[14px] text-[var(--color-error)] mt-0.5">check</span>
                  {t('permMode.permPackages')}
                </li>
              </ul>
            </div>

            <div className="flex items-center justify-end gap-2 px-5 py-3 border-t border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
              <button
                type="button"
                onClick={() => setConfirmDialog(false)}
                className="px-4 py-2 text-xs font-semibold text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] rounded-lg transition-colors"
              >
                {t('common.cancel')}
              </button>
              <button
                type="button"
                onClick={() => {
                  onChange('bypassPermissions')
                  setConfirmDialog(false)
                }}
                className="px-4 py-2 text-xs font-semibold text-white bg-[var(--color-error)] hover:opacity-90 rounded-lg transition-colors"
              >
                {t('permMode.enableBypassBtn')}
              </button>
            </div>
          </div>
        </div>,
        document.body,
      )}
    </div>
  )
}
