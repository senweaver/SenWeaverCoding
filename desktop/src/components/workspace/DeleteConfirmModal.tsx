// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import { useTranslation } from '../../i18n'
import type { FileTreeNode } from '../../types/workspaceFile'
import { Button } from '../shared/Button'
import { Modal } from '../shared/Modal'

type Props = {
  node: FileTreeNode
  onCancel: () => void
  onConfirm: () => void | Promise<void>
}

export function DeleteConfirmModal({ node, onCancel, onConfirm }: Props) {
  const t = useTranslation()
  const [isDeleting, setIsDeleting] = useState(false)

  const handleConfirm = async () => {
    if (isDeleting) return
    setIsDeleting(true)
    try {
      await onConfirm()
    } finally {
      setIsDeleting(false)
    }
  }

  return (
    <Modal
      open
      onClose={() => {
        if (!isDeleting) onCancel()
      }}
      title={t('files.deleteTitle')}
      width={460}
      footer={
        <>
          <Button
            variant="secondary"
            size="md"
            onClick={onCancel}
            disabled={isDeleting}
          >
            {t('common.cancel')}
          </Button>
          <Button
            variant="danger"
            size="md"
            loading={isDeleting}
            onClick={() => {
              void handleConfirm()
            }}
            icon={
              isDeleting ? undefined : (
                <span className="material-symbols-outlined text-[16px]">delete</span>
              )
            }
          >
            {t('files.delete')}
          </Button>
        </>
      }
    >
      <div className="flex flex-col gap-3">
        <div className="flex items-start gap-3">
          <span
            className="material-symbols-outlined text-[28px] text-[var(--color-error)]"
            aria-hidden="true"
          >
            {node.isDir ? 'folder_delete' : 'delete'}
          </span>
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold text-[var(--color-text-primary)]">
              {node.name}
            </div>
            <div
              className="truncate font-[var(--font-mono)] text-[11px] text-[var(--color-text-tertiary)]"
              title={node.relPath || '/'}
            >
              {node.relPath || '/'}
            </div>
          </div>
        </div>
        <p className="text-sm text-[var(--color-text-secondary)]">
          {node.isDir
            ? t('files.deleteDescriptionDir')
            : t('files.deleteDescriptionFile')}
        </p>
        <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-2 text-xs text-[var(--color-text-tertiary)]">
          <span
            className="material-symbols-outlined mr-1 align-middle text-[14px]"
            aria-hidden="true"
          >
            delete_sweep
          </span>
          {t('files.deleteToTrashHint')}
        </div>
      </div>
    </Modal>
  )
}
