// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import { useTranslation } from '../../i18n'
import { unitDisplayName, type DesignUnit } from '../../stores/designerCanvasStore'
import { Button } from '../shared/Button'
import { Modal } from '../shared/Modal'

type Props = {
  unit: DesignUnit | null
  onCancel: () => void
  onConfirm: (unit: DesignUnit) => Promise<boolean>
}

export function DesignerDeleteUnitDialog({ unit, onCancel, onConfirm }: Props) {
  const t = useTranslation()
  const [deleting, setDeleting] = useState(false)
  const [failed, setFailed] = useState(false)

  const handleConfirm = async () => {
    if (!unit || deleting) return
    setDeleting(true)
    setFailed(false)
    try {
      const ok = await onConfirm(unit)
      if (!ok) setFailed(true)
    } catch {
      setFailed(true)
    } finally {
      setDeleting(false)
    }
  }

  const open = unit !== null

  return (
    <Modal
      open={open}
      onClose={() => {
        if (!deleting) onCancel()
      }}
      title={t('designer.canvas.deleteDialogTitle')}
      width={460}
      footer={
        <>
          <Button variant="secondary" size="md" onClick={onCancel} disabled={deleting}>
            {t('common.cancel')}
          </Button>
          <Button
            variant="danger"
            size="md"
            loading={deleting}
            onClick={() => {
              void handleConfirm()
            }}
            icon={
              deleting ? undefined : (
                <span className="material-symbols-outlined text-[16px]">delete_forever</span>
              )
            }
          >
            {t('designer.canvas.deleteDialogConfirm')}
          </Button>
        </>
      }
    >
      {unit && (
        <div className="flex flex-col gap-3">
          <div className="flex items-start gap-3">
            <span
              className="material-symbols-outlined text-[28px] text-[var(--color-error)]"
              aria-hidden="true"
            >
              delete_forever
            </span>
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-semibold text-[var(--color-text-primary)]">
                {unitDisplayName(unit)}
              </div>
              <div
                className="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
                title={unit.relPath}
              >
                {unit.relPath}
              </div>
            </div>
          </div>
          <p className="text-sm text-[var(--color-text-secondary)]">
            {t('designer.canvas.deleteDialogDescription')}
          </p>
          <div className="rounded-[var(--radius-md)] border border-[var(--color-error)]/40 bg-[var(--color-error)]/8 px-3 py-2 text-xs text-[var(--color-error)]">
            <span
              className="material-symbols-outlined mr-1 align-middle text-[14px]"
              aria-hidden="true"
            >
              warning
            </span>
            {t('designer.canvas.deleteDialogWarning')}
          </div>
          {failed && (
            <p className="text-xs text-[var(--color-error)]">{t('designer.canvas.deleteFailed')}</p>
          )}
        </div>
      )}
    </Modal>
  )
}
