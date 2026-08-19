// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { parentOf } from '../../stores/workspaceFilesStore'
import type { FileTreeNode } from '../../types/workspaceFile'
import type { LanPeer } from '../../types/lan'

export type ContextMenuTarget =
  | { kind: 'root' }
  | { kind: 'node'; node: FileTreeNode }

type Props = {
  x: number
  y: number
  target: ContextMenuTarget
  canReveal: boolean
  canOpenTerminal: boolean
  multiSelectionCount?: number
  onClose: () => void
  onNewFile: (parentRelPath: string) => void
  onNewFolder: (parentRelPath: string) => void
  onRename: (node: FileTreeNode) => void
  onDelete: (node: FileTreeNode) => void
  onRefresh: () => void
  onUpload: (parentRelPath: string) => void
  onAddToChat: (node: FileTreeNode) => void
  onFindInFolder: (node: FileTreeNode) => void
  onShowHistory: (node: FileTreeNode) => void
  onCopyAbsolutePath: (node: FileTreeNode) => void
  onCopyRelativePath: (node: FileTreeNode) => void
  onCopyAsMarkdown?: (node: FileTreeNode) => void
  onReveal: (node: FileTreeNode) => void
  onOpenInTerminal: (node: FileTreeNode) => void
  onCopyEntry: (node: FileTreeNode) => void
  onCutEntry: (node: FileTreeNode) => void
  onPasteInto: (targetDir: string) => void
  canPaste: boolean
  lanEnabled: boolean
  lanPeers: LanPeer[]
  onSendToPeer: (node: FileTreeNode, peerId: string) => void
  onShareToLan: (node: FileTreeNode) => void
}

export function FileTreeContextMenu({
  x,
  y,
  target,
  canReveal,
  canOpenTerminal,
  multiSelectionCount = 0,
  onClose,
  onNewFile,
  onNewFolder,
  onRename,
  onDelete,
  onRefresh,
  onUpload,
  onAddToChat,
  onFindInFolder,
  onShowHistory,
  onCopyAbsolutePath,
  onCopyRelativePath,
  onCopyAsMarkdown,
  onReveal,
  onOpenInTerminal,
  onCopyEntry,
  onCutEntry,
  onPasteInto,
  canPaste,
  lanEnabled,
  lanPeers,
  onSendToPeer,
  onShareToLan,
}: Props) {
  const t = useTranslation()
  const ref = useRef<HTMLDivElement | null>(null)
  const [pos, setPos] = useState({ x, y })

  useLayoutEffect(() => {
    const el = ref.current
    if (!el) {
      setPos({ x, y })
      return
    }
    const rect = el.getBoundingClientRect()
    const margin = 6
    let nextX = x
    let nextY = y
    if (nextX + rect.width + margin > window.innerWidth) {
      nextX = Math.max(margin, window.innerWidth - rect.width - margin)
    }
    if (nextY + rect.height + margin > window.innerHeight) {
      nextY = Math.max(margin, window.innerHeight - rect.height - margin)
    }
    setPos({ x: nextX, y: nextY })
  }, [x, y, target])

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
  const newParentRel = node
    ? node.isDir
      ? node.relPath
      : parentOf(node.relPath)
    : ''
  const deleteLabel =
    node && multiSelectionCount > 1
      ? t('files.deleteSelected', { count: multiSelectionCount })
      : t('files.delete')
  const addToChatLabel =
    node && multiSelectionCount > 1
      ? t('files.tree.addToChatSelected', { count: multiSelectionCount })
      : t('files.tree.addToChat')

  return (
    <div
      ref={ref}
      role="menu"
      style={{ left: pos.x, top: pos.y, boxShadow: 'var(--shadow-dropdown)' }}
      className="fixed z-50 min-w-[180px] rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] py-1 text-xs"
    >
      <MenuItem
        label={t('files.newFile')}
        icon="note_add"
        onClick={() => {
          onNewFile(newParentRel)
          onClose()
        }}
      />
      <MenuItem
        label={t('files.newFolder')}
        icon="create_new_folder"
        onClick={() => {
          onNewFolder(newParentRel)
          onClose()
        }}
      />
      <MenuItem
        label={t('files.upload')}
        icon="upload_file"
        onClick={() => {
          onUpload(newParentRel)
          onClose()
        }}
      />
      {canPaste && (
        <MenuItem
          label={t('workspace.paste')}
          icon="content_paste"
          onClick={() => {
            onPasteInto(newParentRel)
            onClose()
          }}
        />
      )}
      <Separator />
      {node && (
        <>
          <MenuItem
            label={addToChatLabel}
            icon="forum"
            onClick={() => {
              onAddToChat(node)
              onClose()
            }}
          />
          <MenuItem
            label={t('files.tree.findInFolder')}
            icon="manage_search"
            onClick={() => {
              onFindInFolder(node)
              onClose()
            }}
          />
          {!isDir && (
            <MenuItem
              label={t('files.history.menu')}
              icon="history"
              onClick={() => {
                onShowHistory(node)
                onClose()
              }}
            />
          )}
          <Separator />
          <MenuItem
            label={t('files.rename')}
            icon="edit"
            onClick={() => {
              onRename(node)
              onClose()
            }}
          />
          <MenuItem
            label={deleteLabel}
            icon="delete"
            danger
            onClick={() => {
              onDelete(node)
              onClose()
            }}
          />
          <Separator />
          <MenuItem
            label={t('workspace.copy')}
            icon="file_copy"
            onClick={() => {
              onCopyEntry(node)
              onClose()
            }}
          />
          <MenuItem
            label={t('workspace.cut')}
            icon="content_cut"
            onClick={() => {
              onCutEntry(node)
              onClose()
            }}
          />
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
          <LanSendMenuItem
            label={t('files.tree.sendToLan')}
            emptyLabel={lanEnabled ? t('files.tree.lanNoPeers') : t('files.tree.lanDiscoveryOff')}
            peers={lanEnabled ? lanPeers.filter((p) => p.online) : []}
            onSelectPeer={(peerId) => {
              onSendToPeer(node, peerId)
              onClose()
            }}
          />
          <MenuItem
            label={t('files.tree.shareToLan')}
            icon="share"
            onClick={() => {
              onShareToLan(node)
              onClose()
            }}
          />
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

function LanSendMenuItem({
  label,
  emptyLabel,
  peers,
  onSelectPeer,
}: {
  label: string
  emptyLabel: string
  peers: LanPeer[]
  onSelectPeer: (peerId: string) => void
}) {
  const rowRef = useRef<HTMLDivElement | null>(null)
  const [open, setOpen] = useState(false)
  const [placeRight, setPlaceRight] = useState(true)
  const [top, setTop] = useState(true)

  const openSub = () => {
    const el = rowRef.current
    if (el) {
      const rect = el.getBoundingClientRect()
      const subWidth = 220
      const subHeightEstimate = Math.min(320, 36 + peers.length * 30)
      setPlaceRight(rect.right + subWidth + 8 <= window.innerWidth)
      setTop(rect.top + subHeightEstimate <= window.innerHeight)
    }
    setOpen(true)
  }

  return (
    <div
      ref={rowRef}
      className="relative"
      onMouseEnter={openSub}
      onMouseLeave={() => setOpen(false)}
    >
      <button
        type="button"
        role="menuitem"
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
      >
        <span className="material-symbols-outlined text-[14px]">send</span>
        <span className="flex-1">{label}</span>
        <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
          chevron_right
        </span>
      </button>
      {open && (
        <div
          role="menu"
          style={{ boxShadow: 'var(--shadow-dropdown)' }}
          className={`absolute z-[60] max-h-[320px] min-w-[200px] overflow-y-auto rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] py-1 ${
            placeRight ? 'left-full ml-0.5' : 'right-full mr-0.5'
          } ${top ? 'top-0' : 'bottom-0'}`}
        >
          {peers.length === 0 ? (
            <div className="px-3 py-1.5 text-[var(--color-text-tertiary)]">{emptyLabel}</div>
          ) : (
            peers.map((peer) => (
              <button
                key={peer.userId}
                type="button"
                role="menuitem"
                onClick={() => onSelectPeer(peer.userId)}
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
              >
                <span className="h-1.5 w-1.5 flex-shrink-0 rounded-full bg-[var(--color-success,#22c55e)]" />
                <span className="flex min-w-0 flex-col">
                  <span className="truncate">{peer.nickname || peer.userId}</span>
                  <span className="truncate text-[10px] text-[var(--color-text-tertiary)]">
                    {peer.ip}
                  </span>
                </span>
              </button>
            ))
          )}
        </div>
      )}
    </div>
  )
}

function Separator() {
  return <div className="my-1 h-px bg-[var(--color-border)]" />
}
