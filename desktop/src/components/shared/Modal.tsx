// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, type ReactNode } from 'react'
import { createPortal } from 'react-dom'

import { useDockSuspend } from '../../hooks/useDockSuspend'

type ModalProps = {
  open: boolean
  onClose: () => void
  title?: string
  children: ReactNode
  width?: number
  footer?: ReactNode
  bodyClassName?: string
  titleClassName?: string
  compact?: boolean
}

export function Modal({ open, onClose, title, children, width = 560, footer, bodyClassName, titleClassName, compact }: ModalProps) {
  useDockSuspend(open)

  useEffect(() => {
    if (!open) return
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        onClose()
      }
    }
    document.addEventListener('keydown', handleEsc)
    return () => document.removeEventListener('keydown', handleEsc)
  }, [open, onClose])

  if (!open) return null

  return createPortal(
    <div className="fixed inset-0 z-[10000] flex items-center justify-center">
      <div
        className="absolute inset-0 bg-[var(--color-overlay-scrim)] transition-opacity duration-200"
        onClick={onClose}
      />

      <div
        className="glass-panel relative flex max-h-[90vh] min-h-0 flex-col overflow-hidden rounded-[var(--radius-xl)]"
        style={{ width, maxWidth: 'calc(100vw - 48px)' }}
        role="dialog"
        aria-modal="true"
      >
        {title && (
          <div className={`flex shrink-0 justify-between gap-4 px-6 pb-0 ${compact ? 'items-center pt-4' : 'items-start pt-6'}`}>
            <h2 className={titleClassName ?? 'text-xl font-bold text-[var(--color-text-primary)]'}>{title}</h2>
            <button
              type="button"
              onClick={onClose}
              aria-label="Close dialog"
              className={`flex shrink-0 items-center justify-center rounded-full text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] ${compact ? 'h-7 w-7' : 'h-9 w-9'}`}
            >
              <span className={`material-symbols-outlined ${compact ? 'text-[16px]' : 'text-[18px]'}`}>close</span>
            </button>
          </div>
        )}

        <div className={`min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-6 py-4 ${bodyClassName ?? ''}`}>
          {children}
        </div>

        {footer && (
          <div className={`flex shrink-0 justify-end gap-2 px-6 pt-0 ${compact ? 'pb-4' : 'pb-6'}`}>
            {footer}
          </div>
        )}
      </div>
    </div>,
    document.body,
  )
}
