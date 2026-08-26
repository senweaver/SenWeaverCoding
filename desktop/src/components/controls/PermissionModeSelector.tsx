// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo, useState } from 'react'
import DOMPurify from 'dompurify'
import { createPortal } from 'react-dom'
import { useTranslation } from '../../i18n'
import { useDockSuspend } from '../../hooks/useDockSuspend'
import { useAnchoredDropdown } from '../../hooks/useAnchoredDropdown'
import type { PermissionMode } from '../../types/settings'

const MODE_ICONS: Record<PermissionMode, string> = {
  default: 'rule',
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

const PERMISSION_ROWS: PermissionMode[] = [
  'askEveryTime',
  'acceptEdits',
  'default',
  'plan',
  'dontAsk',
  'bypassPermissions',
]

export function PermissionModeSelector({ value, onChange, workDir }: Props) {
  const t = useTranslation()
  const [open, setOpen] = useState(false)
  const [confirmDialog, setConfirmDialog] = useState(false)
  const [pendingDangerMode, setPendingDangerMode] = useState<PermissionMode | null>(null)
  useDockSuspend(confirmDialog)
  const { triggerRef, menuRef, style, portalTarget } = useAnchoredDropdown<HTMLButtonElement>(
    open,
    () => setOpen(false),
    { estimatedHeight: 420 },
  )

  const rowMeta = useMemo(
    () =>
      ({
        askEveryTime: {
          label: t('settings.agents.autoRun.opt.askEveryTime'),
          description: t('settings.agents.autoRun.opt.askEveryTimeHint'),
          icon: 'help',
        },
        acceptEdits: {
          label: t('settings.agents.autoRun.opt.acceptEdits'),
          description: t('settings.agents.autoRun.opt.acceptEditsHint'),
          icon: 'bolt',
        },
        default: {
          label: t('settings.agents.autoRun.opt.useAllowlist'),
          description: t('settings.agents.autoRun.opt.useAllowlistHint'),
          icon: 'rule',
        },
        plan: {
          label: t('settings.agents.autoRun.opt.plan'),
          description: t('settings.agents.autoRun.opt.planHint'),
          icon: 'architecture',
        },
        dontAsk: {
          label: t('settings.agents.autoRun.opt.dontAsk'),
          description: t('settings.agents.autoRun.opt.dontAskHint'),
          icon: 'gavel',
          color: 'text-[var(--color-error)]',
        },
        bypassPermissions: {
          label: t('settings.agents.autoRun.opt.runEverything'),
          description: t('settings.agents.autoRun.opt.runEverythingHint'),
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
    default: t('settings.agents.autoRun.opt.useAllowlist'),
    acceptEdits: t('settings.agents.autoRun.opt.acceptEdits'),
    plan: t('permMode.label.plan'),
    bypassPermissions: t('settings.agents.autoRun.opt.runEverything'),
    dontAsk: t('permMode.label.dontAsk'),
    askEveryTime: t('settings.agents.autoRun.opt.askEveryTime'),
  }

  const displayWorkDir = workDir?.trim() || '~'

  return (
    <div className="relative">
      <button
        ref={triggerRef}
        type="button"
        onClick={() => setOpen(!open)}
        className="flex w-[220px] shrink-0 items-center gap-1.5 rounded-full bg-[var(--color-surface-container-low)] px-2.5 py-0.5 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)]"
      >
        <span className="material-symbols-outlined shrink-0 text-[12px]">{MODE_ICONS[value]}</span>
        <div className="min-w-0 flex-1 truncate text-xs font-semibold text-[var(--color-text-primary)]">
          {MODE_LABELS[value]}
        </div>
        <span className="material-symbols-outlined shrink-0 text-[12px]">expand_more</span>
      </button>

      {open && style && createPortal(
        <div
          ref={menuRef}
          className="w-[300px] rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] p-1.5 shadow-[var(--shadow-dropdown)]"
          style={style}
        >
          <div className="mb-1 px-1.5 pt-0.5 text-[10px] font-bold uppercase tracking-widest text-[var(--color-outline)]">
            {t('permMode.executionPermissions')}
          </div>
          <div>
            {PERMISSION_ROWS.map((perm) => {
              const meta = rowMeta[perm]
              const isSelected = perm === value
              return (
                <button
                  type="button"
                  key={perm}
                  onClick={() => {
                    if (perm === 'bypassPermissions' || perm === 'dontAsk') {
                      setOpen(false)
                      setPendingDangerMode(perm)
                      setConfirmDialog(true)
                      return
                    }
                    onChange(perm)
                    setOpen(false)
                  }}
                  className={`
                      w-full rounded-lg border px-2 py-1 text-left transition-colors
                      ${isSelected
                        ? 'border-[var(--color-brand)]/20 bg-[var(--color-primary-fixed)]'
                        : 'border-transparent hover:bg-[var(--color-surface-hover)]'}
                    `}
                >
                  <div className="flex items-start gap-2">
                    <div
                      className={`mt-0.5 flex h-3.5 w-3.5 flex-shrink-0 items-center justify-center rounded-full border-2 ${
                        isSelected ? 'border-[var(--color-brand)]' : 'border-[var(--color-outline)]'
                      }`}
                    >
                      {isSelected && (
                        <div className="h-1.5 w-1.5 rounded-full bg-[var(--color-brand)]" />
                      )}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="flex items-start gap-1.5">
                        <span
                          className={`material-symbols-outlined mt-px shrink-0 text-[16px] ${
                            meta.color || 'text-[var(--color-text-secondary)]'
                          }`}
                        >
                          {meta.icon}
                        </span>
                        <div className="min-w-0 flex-1">
                          <div className="text-xs font-semibold leading-tight text-[var(--color-text-primary)]">
                            {meta.label}
                          </div>
                          <div className="mt-px truncate text-xs leading-snug text-[var(--color-text-tertiary)]">
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
        </div>,
        portalTarget,
      )}

      {confirmDialog && createPortal(
        <div
          className="fixed inset-0 z-[40000] flex items-center justify-center bg-black/40 pl-[var(--sidebar-width)]"
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
              <div className="flex h-8 w-8 items-center justify-center rounded-xl bg-[var(--color-error)]/12">
                <span className="material-symbols-outlined text-[16px] text-[var(--color-error)]">warning</span>
              </div>
              <div>
                <div className="text-xs font-semibold text-[var(--color-text-primary)]">{t('permMode.enableBypassTitle')}</div>
                <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">{t('permMode.enableBypassSubtitle')}</div>
              </div>
            </div>

            <div className="px-5 py-4">
              <p
                className="text-xs text-[var(--color-text-tertiary)] leading-relaxed mb-3"
                dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(t('permMode.enableBypassBody')) }}
              />
              <div
                className="flex items-center gap-2 px-3 py-2 rounded-lg bg-[var(--color-surface-container)] border border-[var(--color-border)]"
                title={displayWorkDir}
              >
                <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)] shrink-0">folder</span>
                <code className="text-xs font-[var(--font-mono)] text-[var(--color-text-primary)] truncate">{displayWorkDir}</code>
              </div>
              <ul className="mt-3 space-y-1.5 text-xs text-[var(--color-text-tertiary)]">
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
                className="h-7 px-4 text-xs font-semibold text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] rounded-lg transition-colors"
              >
                {t('common.cancel')}
              </button>
              <button
                type="button"
                onClick={() => {
                  onChange(pendingDangerMode ?? 'bypassPermissions')
                  setPendingDangerMode(null)
                  setConfirmDialog(false)
                }}
                className="h-7 px-4 text-xs font-semibold text-[var(--color-on-error)] bg-[var(--color-error)] hover:opacity-90 rounded-lg transition-colors"
              >
                {t('permMode.enableBypassBtn')}
              </button>
            </div>
          </div>
        </div>,
        portalTarget,
      )}
    </div>
  )
}
