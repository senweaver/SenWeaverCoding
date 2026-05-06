import { memo, useCallback, useEffect, useMemo, useState } from 'react'
import { Icon } from '@iconify/react/dist/offline'
import { useTranslation } from '../../i18n'
import type { FileTreeNode } from '../../types/workspaceFile'
import {
  AI_FRESH_WINDOW_MS,
  useWorkspaceFilesStore,
} from '../../stores/workspaceFilesStore'
import { ensureVscodeIcons, getFileIconId, isVscodeIconsReady } from '../../lib/fileIcons'
import { InlineNamePrompt } from './InlineNamePrompt'

export type RenameTargetState = {
  relPath: string
  initial: string
}

export type CreateTargetState = {
  parentRelPath: string
  kind: 'file' | 'folder'
}

type Props = {
  node: FileTreeNode
  depth: number
  selectedRelPath: string | null

  renameTarget: RenameTargetState | null

  createTarget: CreateTargetState | null
  onSelect: (node: FileTreeNode) => void
  onContextMenu: (event: React.MouseEvent, node: FileTreeNode) => void
  onDrop: (event: React.DragEvent, target: FileTreeNode) => void
  onDragStart: (event: React.DragEvent, node: FileTreeNode) => void
  onRenameSubmit: (value: string) => void
  onRenameCancel: () => void
  onCreateSubmit: (value: string) => void
  onCreateCancel: () => void
}

function useEnsureVscodeIcons(): boolean {
  const [ready, setReady] = useState(() => isVscodeIconsReady())
  useEffect(() => {
    if (ready) return
    let cancelled = false
    void ensureVscodeIcons().then(() => {
      if (!cancelled) setReady(true)
    })
    return () => {
      cancelled = true
    }
  }, [ready])
  return ready
}

export const FileTreeNodeView = memo(function FileTreeNodeView({
  node,
  depth,
  selectedRelPath,
  renameTarget,
  createTarget,
  onSelect,
  onContextMenu,
  onDrop,
  onDragStart,
  onRenameSubmit,
  onRenameCancel,
  onCreateSubmit,
  onCreateCancel,
}: Props) {
  const t = useTranslation()
  const iconsReady = useEnsureVscodeIcons()
  const setExpanded = useWorkspaceFilesStore((s) => s.setExpanded)
  const toggleExpanded = useWorkspaceFilesStore((s) => s.toggleExpanded)
  const dirState = useWorkspaceFilesStore((s) => {
    const root = s.root
    return root ? s.dirs[`${root}::${node.relPath}`] : undefined
  })
  const aiModifiedTs = useWorkspaceFilesStore(
    (s) => s.aiModifiedAt[node.relPath],
  )

  const [now, setNow] = useState(() => Date.now())
  const aiAge = aiModifiedTs !== undefined ? now - aiModifiedTs : Number.POSITIVE_INFINITY
  const aiFresh = aiAge < AI_FRESH_WINDOW_MS
  const aiOpacity = aiFresh
    ? Math.max(0, Math.min(1, 1 - (aiAge - AI_FRESH_WINDOW_MS / 2) / (AI_FRESH_WINDOW_MS / 2)))
    : 0

  useEffect(() => {
    if (!aiFresh) return
    const interval = window.setInterval(() => setNow(Date.now()), 1_000)
    return () => window.clearInterval(interval)
  }, [aiFresh])

  const expanded = node.isDir && (dirState?.expanded ?? false)
  const loading = node.isDir && (dirState?.loading ?? false)
  const error = node.isDir ? dirState?.error : undefined
  const children = useMemo<FileTreeNode[]>(() => {
    if (!node.isDir) return []
    if (dirState?.loaded) return dirState.children ?? []
    return node.children ?? []
  }, [dirState, node.children, node.isDir, node.loaded])

  const isSelected = !node.isDir && selectedRelPath === node.relPath

  const handleClick = useCallback(() => {
    if (node.isDir) {
      void toggleExpanded(node.relPath)
    } else {
      onSelect(node)
    }
  }, [node, onSelect, toggleExpanded])

  const handleChevron = useCallback(
    (event: React.MouseEvent) => {
      event.stopPropagation()
      if (node.isDir) {
        void toggleExpanded(node.relPath)
      }
    },
    [node, toggleExpanded],
  )

  const handleDragOver = useCallback(
    (event: React.DragEvent) => {
      if (!node.isDir) return
      event.preventDefault()
      event.dataTransfer.dropEffect = 'move'
      if (!expanded) {
        setExpanded(node.relPath, true)
        if (!dirState?.loaded && !dirState?.loading) {
          void useWorkspaceFilesStore.getState().loadDirectory(node.relPath)
        }
      }
    },
    [dirState?.loaded, dirState?.loading, expanded, node.isDir, node.relPath, setExpanded],
  )

  if (renameTarget && renameTarget.relPath === node.relPath) {
    return (
      <div className="px-1 py-0.5" style={{ paddingLeft: `${depth * 12 + 4}px` }}>
        <InlineNamePrompt
          initial={renameTarget.initial}
          onCancel={onRenameCancel}
          onSubmit={onRenameSubmit}
        />
      </div>
    )
  }

  return (
    <>
      <div
        role="treeitem"
        aria-selected={isSelected}
        aria-expanded={node.isDir ? expanded : undefined}
        draggable
        onDragStart={(e) => onDragStart(e, node)}
        onDragOver={handleDragOver}
        onDrop={(e) => onDrop(e, node)}
        onClick={handleClick}
        onContextMenu={(e) => onContextMenu(e, node)}
        className={`group relative flex items-center gap-1 h-6 px-1 cursor-pointer text-xs select-none ${
          isSelected
            ? 'bg-[var(--color-accent)]/15 text-[var(--color-text-primary)]'
            : 'hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]'
        }`}
        style={{ paddingLeft: `${depth * 12 + 4}px` }}
        title={node.relPath}
      >
        {}
        {depth > 0 &&
          Array.from({ length: depth }).map((_, lvl) => (
            <span
              key={lvl}
              aria-hidden="true"
              className="pointer-events-none absolute top-0 h-full w-px bg-[var(--color-border)]/50"
              style={{ left: `${lvl * 12 + 10}px` }}
            />
          ))}
        <span
          aria-hidden="true"
          onClick={handleChevron}
          className="material-symbols-outlined text-[14px] w-4 text-[var(--color-text-tertiary)] flex-shrink-0"
        >
          {node.isDir ? (expanded ? 'expand_more' : 'chevron_right') : ''}
        </span>
        {}
        {iconsReady ? (
          <Icon
            aria-hidden="true"
            icon={getFileIconId(node.name, node.isDir, expanded)}
            width={16}
            height={16}
            className="flex-shrink-0"
          />
        ) : (
          <span
            aria-hidden="true"
            className={`material-symbols-outlined text-[14px] flex-shrink-0 ${
              node.isDir
                ? 'text-[var(--color-accent)]'
                : 'text-[var(--color-text-tertiary)]'
            }`}
          >
            {node.isDir ? (expanded ? 'folder_open' : 'folder') : 'description'}
          </span>
        )}
        <span className="truncate">{node.name}</span>
        {aiFresh && (
          <span
            aria-hidden="true"
            style={{ opacity: aiOpacity, transition: 'opacity 250ms linear' }}
            className="ml-auto flex h-3.5 min-w-[14px] items-center justify-center rounded-sm bg-[var(--color-warning)]/85 px-1 text-[9px] font-bold leading-none text-white"
          >
            M
          </span>
        )}
        {loading && (
          <span
            aria-hidden="true"
            className="material-symbols-outlined ml-auto text-[12px] animate-spin text-[var(--color-text-tertiary)]"
          >
            progress_activity
          </span>
        )}
      </div>
      {node.isDir && expanded && (
        <>
          {createTarget && createTarget.parentRelPath === node.relPath && (
            <div
              className="px-1 py-0.5"
              style={{ paddingLeft: `${(depth + 1) * 12 + 4}px` }}
            >
              <InlineNamePrompt
                placeholder={
                  createTarget.kind === 'file'
                    ? t('files.newFile')
                    : t('files.newFolder')
                }
                onCancel={onCreateCancel}
                onSubmit={onCreateSubmit}
              />
            </div>
          )}
          {error ? (
            <div
              className="px-2 py-1 text-[11px] text-[var(--color-text-tertiary)]"
              style={{ paddingLeft: `${(depth + 1) * 12 + 4}px` }}
            >
              {error}
            </div>
          ) : children.length === 0 && dirState?.loaded ? (
            <div
              className="px-2 py-1 text-[11px] text-[var(--color-text-tertiary)] italic"
              style={{ paddingLeft: `${(depth + 1) * 12 + 4}px` }}
            >
              {}
              ·
            </div>
          ) : (
            children.map((child) => (
              <FileTreeNodeView
                key={child.relPath}
                node={child}
                depth={depth + 1}
                selectedRelPath={selectedRelPath}
                renameTarget={renameTarget}
                createTarget={createTarget}
                onSelect={onSelect}
                onContextMenu={onContextMenu}
                onDrop={onDrop}
                onDragStart={onDragStart}
                onRenameSubmit={onRenameSubmit}
                onRenameCancel={onRenameCancel}
                onCreateSubmit={onCreateSubmit}
                onCreateCancel={onCreateCancel}
              />
            ))
          )}
        </>
      )}
    </>
  )
})
