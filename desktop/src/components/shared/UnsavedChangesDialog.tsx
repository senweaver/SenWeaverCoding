// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { Modal } from './Modal'
import { Button } from './Button'

type UnsavedChangesDialogProps = {
  open: boolean
  title: string
  body: string
  saveLabel: string
  discardLabel: string
  cancelLabel: string
  onSave: () => void | Promise<void>
  onDiscard: () => void | Promise<void>
  onCancel: () => void
  busy?: boolean
}

export function UnsavedChangesDialog({
  open,
  title,
  body,
  saveLabel,
  discardLabel,
  cancelLabel,
  onSave,
  onDiscard,
  onCancel,
  busy = false,
}: UnsavedChangesDialogProps) {
  return (
    <Modal
      open={open}
      onClose={busy ? () => {} : onCancel}
      title={title}
      width={460}
      footer={(
        <>
          <Button variant="secondary" onClick={onCancel} disabled={busy}>
            {cancelLabel}
          </Button>
          <Button variant="danger" onClick={() => void onDiscard()} disabled={busy}>
            {discardLabel}
          </Button>
          <Button variant="primary" onClick={() => void onSave()} loading={busy}>
            {saveLabel}
          </Button>
        </>
      )}
    >
      <p className="text-sm leading-6 text-[var(--color-text-secondary)]">
        {body}
      </p>
    </Modal>
  )
}
