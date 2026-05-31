// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef } from 'react'
import { useTranslation } from '../../i18n'
import type { FileTreeNode } from '../../types/workspaceFile'

export type ContextMenuTarget =
  | { kind: 'root' }
  | { kind: 'node'; node: FileTreeNode }

type Props = {
  x: number
  y: number
  target: ContextMenuTarget
  canReveal: boolean
  canOpenTerminal: boolean
  onClose: () => void
  onNewFile: (parent: FileTreeNode | null) => void
  onNewFolder: (parent: FileTreeNode | null) => void
  onRename: (node: FileTreeNode) => void
  onDelete: (node: FileTreeNode) => void
  onRefresh: () => void
  onUpload: (parent: FileTreeNode | null) => void
  onCopyAbsolutePath: (node: FileTreeNode) => void
  onCopyRelativePath: (node: FileTreeNode) => void
  onCopyAsMarkdown?: (node: FileTreeNode) => void
  onReveal: (node: FileTreeNode) => void
  onOpenInTerminal: (node: FileTreeNode) => void
}

export function FileTreeContextMenu({
  x,
  y,
  target,
  canReveal,
  canOpenTerminal,
  onClose,
  onNewFile,
  onNewFolder,
  onRename,
  onDelete,
  onRefresh,
  onUpload,
  onCopyAbsolutePath,
  onCopyRelativePath,
  onCopyAsMarkdown,
  onReveal,
  onOpenInTerminal,
}: Props) {
  const t = useTranslation()
  const ref = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    function onDoc(event: MouseEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        onClose()
      }
    }
    function onKey(event: KeyboardEvent) {
      if (event.key === 'Escape') onClose()
    }
    document.addEventListener('mousedown', onDoc)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDoc)
      document.removeEventListener('keydown', onKey)
    }
  }, [onClose])

  const node = target.kind === 'node' ? target.node : null
  const isDir = node?.isDir ?? true
  const parent = target.kind === 'node' ? (node?.isDir ? node : null) : null

  return (
    <div
      ref={ref}
      role="menu"
      style={{ left: x, top: y, boxShadow: 'var(--shadow-dropdown)' }}
      className="fixed z-50 min-w-[180px] rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] py-1 text-xs"
    >
      {isDir && (
        <>
          <MenuItem
            label={t('files.newFile')}
            icon="note_add"
            onClick={() => {
              onNewFile(parent)
              onClose()
            }}
          />
          <MenuItem
            label={t('files.newFolder')}
            icon="create_new_folder"
            onClick={() => {
              onNewFolder(parent)
              onClose()
            }}
          />
          <MenuItem
            label={t('files.upload')}
            icon="upload_file"
            onClick={() => {
              onUpload(parent)
              onClose()
            }}
          />
          {(target.kind === 'node' || target.kind === 'root') && (
            <Separator />
          )}
        </>
      )}
      {node && (
        <>
          <MenuItem
            label={t('files.rename')}
            icon="edit"
            onClick={() => {
              onRename(node)
              onClose()
            }}
          />
          <MenuItem
            label={t('files.delete')}
            icon="delete"
            danger
            onClick={() => {
              onDelete(node)
              onClose()
            }}
          />
          <Separator />
          <MenuItem
            label={t('files.tree.copyPath')}
            icon="content_copy"
            onClick={() => {
              onCopyAbsolutePath(node)
              onClose()
            }}
          />
          <MenuItem
            label={t('files.tree.copyRelativePath')}
            icon="link"
            onClick={() => {
              onCopyRelativePath(node)
              onClose()
            }}
          />
          {!isDir && onCopyAsMarkdown && (
            <MenuItem
              label={t('files.tree.copyMarkdown')}
              icon="code_blocks"
              onClick={() => {
                onCopyAsMarkdown(node)
                onClose()
              }}
            />
          )}
          {canReveal && (
            <MenuItem
              label={t('files.tree.reveal')}
              icon="folder_open"
              onClick={() => {
                onReveal(node)
                onClose()
              }}
            />
          )}
          {canOpenTerminal && (
            <MenuItem
              label={t('files.tree.openInTerminal')}
              icon="terminal"
              onClick={() => {
                onOpenInTerminal(node)
                onClose()
              }}
            />
          )}
          <Separator />
        </>
      )}
      <MenuItem
        label={t('files.refresh')}
        icon="refresh"
        onClick={() => {
          onRefresh()
          onClose()
        }}
      />
    </div>
  )
}

function MenuItem({
  label,
  icon,
  onClick,
  danger,
}: {
  label: string
  icon: string
  onClick: () => void
  danger?: boolean
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className={`flex w-full items-center gap-2 px-3 py-1.5 text-left ${
        danger
          ? 'text-[var(--color-danger)] hover:bg-[var(--color-danger)]/10'
          : 'text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]'
      }`}
    >
      <span className="material-symbols-outlined text-[14px]">{icon}</span>
      <span className="flex-1">{label}</span>
    </button>
  )
}

function Separator() {
  return <div className="my-1 h-px bg-[var(--color-border)]" />
}
