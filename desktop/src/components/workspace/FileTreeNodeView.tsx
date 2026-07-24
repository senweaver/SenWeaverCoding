// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Icon } from '@iconify/react/dist/offline'
import { useTranslation } from '../../i18n'
import type { FileTreeNode } from '../../types/workspaceFile'
import { useFileDragStore } from '../../stores/fileDragStore'
import {
  AI_FRESH_WINDOW_MS,
  useWorkspaceFilesStore,
} from '../../stores/workspaceFilesStore'
import {
  classifyEntry,
  statusBadgeChar,
  useGitStatusStore,
  type GitStatusSeverity,
} from '../../stores/gitStatusStore'
import { ensureVscodeIcons, getFileIconId, isVscodeIconsReady } from '../../lib/fileIcons'
import { formatBytes } from '../../lib/formatBytes'
import { formatAbsoluteTime, formatRelativeTime } from '../../lib/formatRelativeTime'
import { InlineNamePrompt } from './InlineNamePrompt'

const GIT_BADGE_COLORS: Record<GitStatusSeverity, string> = {
  modified: 'bg-amber-500',
  typeChanged: 'bg-amber-500',
  added: 'bg-emerald-500',
  deleted: 'bg-rose-500',
  untracked: 'bg-zinc-400',
  renamed: 'bg-sky-500',
  copied: 'bg-sky-500',
  conflicted: 'bg-orange-500',
  ignored: 'bg-red-600',
  unmodified: '',
}

export type RenameTargetState = {
  relPath: string
  initial: string
}

export type FilterState = {
  active: boolean
  needle: string
  matches: Set<string>
  ancestors: Set<string>
}

type Props = {
  node: FileTreeNode
  depth: number
  selectedRelPath: string | null
  focusedRelPath?: string | null

  renameTarget: RenameTargetState | null

  filter?: FilterState
  onSelect: (node: FileTreeNode) => void
  onFocus?: (relPath: string) => void
  onContextMenu: (event: React.MouseEvent, node: FileTreeNode) => void
  onDrop: (event: React.DragEvent, target: FileTreeNode) => void
  onRenameSubmit: (value: string) => void
  onRenameCancel: () => void
}

function renderHighlight(text: string, needle: string) {
  if (!needle) return text
  const lower = text.toLowerCase()
  const target = needle.toLowerCase()
  if (!target) return text
  const segments: Array<{ text: string; match: boolean }> = []
  let cursor = 0
  while (cursor < text.length) {
    const idx = lower.indexOf(target, cursor)
    if (idx === -1) {
      segments.push({ text: text.slice(cursor), match: false })
      break
    }
    if (idx > cursor) {
      segments.push({ text: text.slice(cursor, idx), match: false })
    }
    segments.push({ text: text.slice(idx, idx + target.length), match: true })
    cursor = idx + target.length
  }
  return (
    <>
      {segments.map((seg, i) =>
        seg.match ? (
          <mark
            key={i}
            className="bg-[var(--color-accent)]/30 text-[var(--color-text-primary)] rounded-sm px-[1px]"
          >
            {seg.text}
          </mark>
        ) : (
          <span key={i}>{seg.text}</span>
        ),
      )}
    </>
  )
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
  focusedRelPath,
  renameTarget,
  filter,
  onSelect,
  onFocus,
  onContextMenu,
  onDrop,
  onRenameSubmit,
  onRenameCancel,
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
  const workspaceRoot = useWorkspaceFilesStore((s) => s.root)
  const isCut = useWorkspaceFilesStore(
    (s) => s.clipboard?.mode === 'cut' && s.clipboard.relPath === node.relPath,
  )
  const gitSeverity = useGitStatusStore((s): GitStatusSeverity => {
    if (!workspaceRoot) return 'unmodified'
    const bucket = s.byRoot[workspaceRoot]
    if (!bucket) return 'unmodified'
    if (node.isDir) {
      return bucket.dirAggregate[node.relPath] ?? 'unmodified'
    }
    const entry = bucket.entries[node.relPath]
    if (!entry) return 'unmodified'
    return classifyEntry(entry)
  })
  const gitBadgeChar = useMemo(() => statusBadgeChar(gitSeverity), [gitSeverity])

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

  const filterActive = filter?.active ?? false
  const isAncestorMatch = filterActive ? (filter?.ancestors.has(node.relPath) ?? false) : false
  const expanded = node.isDir && (
    filterActive && isAncestorMatch
      ? true
      : (dirState?.expanded ?? false)
  )
  const loading = node.isDir && (dirState?.loading ?? false)

  const isSelected = !node.isDir && selectedRelPath === node.relPath
  const isFocused = focusedRelPath === node.relPath

  const sizeLabel = useMemo(() => {
    if (node.isDir) return ''
    if (typeof node.sizeBytes !== 'number' || node.sizeBytes <= 0) return ''
    return formatBytes(node.sizeBytes)
  }, [node.isDir, node.sizeBytes])

  const relativeTimeLabel = useMemo(
    () => formatRelativeTime(node.modifiedAt),
    [node.modifiedAt],
  )

  const tooltip = useMemo(() => {
    const lines: string[] = [
      `${t('files.tree.pathLabel')}: ${node.relPath}`,
    ]
    if (!node.isDir && sizeLabel) {
      lines.push(`${t('files.tree.sizeLabel')}: ${sizeLabel}`)
    }
    const absTime = formatAbsoluteTime(node.modifiedAt)
    if (absTime) {
      lines.push(`${t('files.tree.modifiedLabel')}: ${absTime}`)
    }
    return lines.join('\n')
  }, [node.isDir, node.modifiedAt, node.relPath, sizeLabel, t])

  const suppressClickRef = useRef(false)

  const handleClick = useCallback(() => {
    if (suppressClickRef.current) {
      suppressClickRef.current = false
      return
    }
    onFocus?.(node.relPath)
    if (node.isDir) {
      void toggleExpanded(node.relPath)
    } else {
      onSelect(node)
    }
  }, [node, onFocus, onSelect, toggleExpanded])

  const handlePointerDown = useCallback(
    (event: React.PointerEvent) => {
      if (event.button !== 0) return
      if ((event.target as HTMLElement).closest('[data-tree-chevron]')) return
      suppressClickRef.current = false
      const startX = event.clientX
      const startY = event.clientY
      const payload = { relPath: node.relPath, name: node.name, isDir: node.isDir }
      const store = useFileDragStore.getState()
      let started = false
      const onMove = (ev: PointerEvent) => {
        if (!started) {
          if (Math.hypot(ev.clientX - startX, ev.clientY - startY) < 6) return
          started = true
          suppressClickRef.current = true
          document.body.classList.add('sen-file-dragging')
          store.begin(payload, ev.clientX, ev.clientY)
        } else {
          useFileDragStore.getState().move(ev.clientX, ev.clientY)
        }
      }
      const cleanup = (commit: boolean) => {
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        window.removeEventListener('pointercancel', onCancel)
        document.body.classList.remove('sen-file-dragging')
        if (started) {
          if (commit) useFileDragStore.getState().finish()
          else useFileDragStore.getState().cancel()
        }
      }
      const onUp = () => cleanup(true)
      const onCancel = () => cleanup(false)
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
      window.addEventListener('pointercancel', onCancel)
    },
    [node.relPath, node.name, node.isDir],
  )

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
        data-tree-relpath={node.relPath}
        data-tree-isdir={node.isDir ? '1' : '0'}
        onPointerDown={handlePointerDown}
        onDragOver={handleDragOver}
        onDrop={(e) => onDrop(e, node)}
        onClick={handleClick}
        onContextMenu={(e) => onContextMenu(e, node)}
        className={`group relative flex items-center gap-1 h-6 px-1 cursor-pointer text-xs select-none ${
          isSelected
            ? 'bg-[var(--color-accent)]/15 text-[var(--color-text-primary)]'
            : 'hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]'
        }${isFocused ? ' ring-1 ring-inset ring-[var(--color-accent)]/60' : ''}${
          isCut ? ' opacity-50' : ''
        }`}
        style={{ paddingLeft: `${depth * 12 + 4}px` }}
        title={tooltip}
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
          data-tree-chevron="true"
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
        <span className="truncate">
          {filterActive && filter?.needle
            ? renderHighlight(node.name, filter.needle)
            : node.name}
        </span>
        <div className="ml-auto flex flex-shrink-0 items-center gap-1.5">
          {!aiFresh && !loading && (sizeLabel || relativeTimeLabel) && (
            <span
              aria-hidden="true"
              className="hidden items-center gap-1.5 text-[10px] font-normal tabular-nums text-[var(--color-text-tertiary)]/70 group-hover:flex"
            >
              {relativeTimeLabel && <span>{relativeTimeLabel}</span>}
              {sizeLabel && <span>{sizeLabel}</span>}
            </span>
          )}
          {aiFresh && (
            <span
              aria-hidden="true"
              style={{ opacity: aiOpacity, transition: 'opacity 250ms linear' }}
              className="flex h-3.5 min-w-[14px] items-center justify-center rounded-sm bg-[var(--color-warning)]/85 px-1 text-[9px] font-bold leading-none text-white"
            >
              M
            </span>
          )}
          {!aiFresh && gitBadgeChar && (
            <span
              aria-hidden="true"
              title={`git: ${gitSeverity}`}
              className={`flex h-3.5 min-w-[14px] items-center justify-center rounded-sm px-1 text-[9px] font-bold leading-none text-white ${GIT_BADGE_COLORS[gitSeverity]}`}
            >
              {gitBadgeChar}
            </span>
          )}
          {loading && (
            <span
              aria-hidden="true"
              className="material-symbols-outlined text-[12px] animate-spin text-[var(--color-text-tertiary)]"
            >
              progress_activity
            </span>
          )}
        </div>
      </div>
    </>
  )
})
