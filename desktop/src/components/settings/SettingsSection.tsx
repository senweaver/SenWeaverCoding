// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ReactNode } from 'react'

export type SettingsSectionStatusValue = { kind: 'ok' | 'error'; text: string } | null

export function SettingsSection({
  title,
  description,
  children,
  footer,
}: {
  title: string
  description?: string
  children: ReactNode
  footer?: ReactNode
}) {
  return (
    <section className="shrink-0 overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)]">
      <div className="px-3 py-2 border-b border-[var(--color-border)] bg-[var(--color-surface-container-low)]">
        <h2 className="text-xs font-semibold text-[var(--color-text-primary)]">{title}</h2>
        {description && (
          <p className="text-xs text-[var(--color-text-tertiary)] mt-0.5">{description}</p>
        )}
      </div>
      <div className="p-3 space-y-3">{children}</div>
      {footer && (
        <div className="px-3 py-2 border-t border-[var(--color-border)] bg-[var(--color-surface-container-low)] flex items-center justify-end gap-3">
          {footer}
        </div>
      )}
    </section>
  )
}

export function SettingsSectionStatus({ status }: { status: SettingsSectionStatusValue }) {
  if (!status) return null
  return (
    <span
      className={`flex-1 min-w-0 truncate text-left text-xs ${
        status.kind === 'ok' ? 'text-[var(--color-success)]' : 'text-[var(--color-error)]'
      }`}
      title={status.text}
    >
      {status.text}
    </span>
  )
}
