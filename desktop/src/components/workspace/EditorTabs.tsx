import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Icon } from '@iconify/react/dist/offline'
import { useShallow } from 'zustand/react/shallow'
import { useTranslation } from '../../i18n'
import {
  AI_FRESH_WINDOW_MS,
  nameOf,
  useWorkspaceFilesStore,
} from '../../stores/workspaceFilesStore'
import { ensureVscodeIcons, getFileIconId, isVscodeIconsReady } from '../../lib/fileIcons'

const DRAG_MIME = 'application/x-sen-editor-tab'

type ContextMenu = {
  x: number
  y: number
  relPath: string
}

export function EditorTabs() {
  const t = useTranslation()
  const openTabs = useWorkspaceFilesStore((s) => s.openTabs)
  const activeTab = useWorkspaceFilesStore((s) => s.activeTab)
  const aiModifiedAt = useWorkspaceFilesStore((s) => s.aiModifiedAt)
  const externalChanged = useWorkspaceFilesStore((s) => s.externalChanged)
  const closeTab = useWorkspaceFilesStore((s) => s.closeTab)
  const closeAllTabs = useWorkspaceFilesStore((s) => s.closeAllTabs)
  const closeOtherTabs = useWorkspaceFilesStore((s) => s.closeOtherTabs)
  const reorderTab = useWorkspaceFilesStore((s) => s.reorderTab)
  const setActiveTab = useWorkspaceFilesStore((s) => s.setActiveTab)
  const root = useWorkspaceFilesStore((s) => s.root)

  const dirtyByPath = useWorkspaceFilesStore(
    useShallow((s) => {
      const out: Record<string, boolean> = {}
      if (!s.root) return out
      for (const tab of s.openTabs) {
        const buf = s.files[`${s.root}::${tab}`]
        if (buf?.isDirty) out[tab] = true
      }
      return out
    }),
  )

  const [iconsReady, setIconsReady] = useState(() => isVscodeIconsReady())
  useEffect(() => {
    if (iconsReady) return
    let cancelled = false
    void ensureVscodeIcons().then(() => {
      if (!cancelled) setIconsReady(true)
    })
    return () => {
      cancelled = true
    }
  }, [iconsReady])

  const [now, setNow] = useState(() => Date.now())

  const hasFreshAi = useMemo(() => {
    return Object.values(aiModifiedAt).some(
      (ts) => now - ts < AI_FRESH_WINDOW_MS,
    )
  }, [aiModifiedAt, now])

  useEffect(() => {
    if (!hasFreshAi) return
    const interval = window.setInterval(() => {
      setNow(Date.now())
    }, 1_000)
    return () => window.clearInterval(interval)
  }, [hasFreshAi])

  const [menu, setMenu] = useState<ContextMenu | null>(null)
  const dragOverIdx = useRef<number | null>(null)

  const handleDragStart = useCallback(
    (event: React.DragEvent, relPath: string) => {
      event.dataTransfer.setData(DRAG_MIME, relPath)
      event.dataTransfer.effectAllowed = 'move'
    },
    [],
  )

  const handleDragOver = useCallback((event: React.DragEvent, idx: number) => {
    if (!event.dataTransfer.types.includes(DRAG_MIME)) return
    event.preventDefault()
    event.dataTransfer.dropEffect = 'move'
    dragOverIdx.current = idx
  }, [])

  const handleDrop = useCallback(
    (event: React.DragEvent, idx: number) => {
      const fromRelPath = event.dataTransfer.getData(DRAG_MIME)
      dragOverIdx.current = null
      if (!fromRelPath) return
      reorderTab(fromRelPath, idx)
    },
    [reorderTab],
  )

  const handleClose = useCallback(
    (event: React.MouseEvent | React.KeyboardEvent, relPath: string) => {
      event.stopPropagation()
      closeTab(relPath)
    },
    [closeTab],
  )

  const handleAuxClick = useCallback(
    (event: React.MouseEvent, relPath: string) => {

      if (event.button === 1) {
        event.preventDefault()
        closeTab(relPath)
      }
    },
    [closeTab],
  )

  const handleContextMenu = useCallback(
    (event: React.MouseEvent, relPath: string) => {
      event.preventDefault()
      setMenu({ x: event.clientX, y: event.clientY, relPath })
    },
    [],
  )

  useEffect(() => {
    if (!menu) return
    const onClick = () => setMenu(null)
    window.addEventListener('mousedown', onClick)
    return () => window.removeEventListener('mousedown', onClick)
  }, [menu])

  if (!root || openTabs.length === 0) {
    return null
  }

  return (
    <div className="relative flex h-9 flex-shrink-0 items-end overflow-x-auto overflow-y-hidden border-b border-[var(--color-border)] bg-[var(--color-surface-elevated)] scroll-smooth">
      {openTabs.map((relPath, idx) => {
        const isActive = relPath === activeTab
        const aiTs = aiModifiedAt[relPath]
        const aiAge = aiTs !== undefined ? now - aiTs : Number.POSITIVE_INFINITY
        const aiFresh = aiAge < AI_FRESH_WINDOW_MS

        const aiOpacity = aiFresh
          ? Math.max(0, Math.min(1, 1 - (aiAge - AI_FRESH_WINDOW_MS / 2) / (AI_FRESH_WINDOW_MS / 2)))
          : 0
        const isExternal = externalChanged[relPath] !== undefined
        const isDirty = !!dirtyByPath[relPath]
        return (
          <button
            key={relPath}
            type="button"
            draggable
            onDragStart={(e) => handleDragStart(e, relPath)}
            onDragOver={(e) => handleDragOver(e, idx)}
            onDrop={(e) => handleDrop(e, idx)}
            onClick={() => setActiveTab(relPath)}
            onAuxClick={(e) => handleAuxClick(e, relPath)}
            onContextMenu={(e) => handleContextMenu(e, relPath)}
            title={relPath}
            className={`group relative flex h-full max-w-[200px] flex-shrink-0 items-center gap-1.5 border-r border-[var(--color-border)] px-2.5 text-xs transition-colors ${
              isActive
                ? 'bg-[var(--color-surface)] text-[var(--color-text-primary)]'
                : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
            }`}
          >
            {isActive && (
              <span
                aria-hidden="true"
                className="absolute inset-x-0 top-0 h-[2px] bg-[var(--color-accent)]"
              />
            )}
            {iconsReady && (
              <Icon
                aria-hidden="true"
                icon={getFileIconId(nameOf(relPath), false, false)}
                width={14}
                height={14}
                className="flex-shrink-0"
              />
            )}
            <span className="truncate">{nameOf(relPath)}</span>
            {aiFresh && (
              <span
                aria-label={t('files.tab.aiBadge')}
                title={t('files.tab.aiBadge')}
                style={{ opacity: aiOpacity, transition: 'opacity 250ms linear' }}
                className="flex h-4 min-w-[16px] items-center justify-center rounded-sm bg-[var(--color-warning)]/85 px-1 text-[10px] font-bold leading-none text-white"
              >
                M
              </span>
            )}
            {isExternal && !aiFresh && (
              <span
                aria-label={t('files.tab.externalBadge')}
                title={t('files.tab.externalBadge')}
                className="material-symbols-outlined text-[14px] text-[var(--color-warning)]"
              >
                error
              </span>
            )}
            {isDirty && !aiFresh && (
              <span
                aria-label={t('files.unsavedChanges')}
                className="h-1.5 w-1.5 flex-shrink-0 rounded-full bg-[var(--color-text-secondary)]"
              />
            )}
            <span
              role="button"
              tabIndex={-1}
              aria-label={t('files.tab.close')}
              onClick={(e) => handleClose(e, relPath)}
              onKeyDown={(e) => {
                if (e.key === 'Enter' || e.key === ' ') handleClose(e, relPath)
              }}
              className={`ml-0.5 flex h-4 w-4 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] ${
                isActive ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
              }`}
            >
              <span className="material-symbols-outlined text-[12px]">close</span>
            </span>
          </button>
        )
      })}

      {menu && (
        <div
          role="menu"
          onMouseDown={(e) => e.stopPropagation()}
          className="fixed z-50 min-w-[160px] overflow-hidden rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] py-1 text-xs shadow-lg"
          style={{ left: `${menu.x}px`, top: `${menu.y}px` }}
        >
          <MenuItem
            label={t('files.tab.close')}
            onClick={() => {
              closeTab(menu.relPath)
              setMenu(null)
            }}
          />
          <MenuItem
            label={t('files.tab.closeOthers')}
            disabled={openTabs.length <= 1}
            onClick={() => {
              closeOtherTabs(menu.relPath)
              setMenu(null)
            }}
          />
          <MenuItem
            label={t('files.tab.closeAll')}
            onClick={() => {
              closeAllTabs()
              setMenu(null)
            }}
          />
        </div>
      )}
    </div>
  )
}

function MenuItem({
  label,
  onClick,
  disabled,
}: {
  label: string
  onClick: () => void
  disabled?: boolean
}) {
  return (
    <button
      type="button"
      role="menuitem"
      disabled={disabled}
      onClick={onClick}
      className={`block w-full px-3 py-1.5 text-left ${
        disabled
          ? 'cursor-not-allowed text-[var(--color-text-tertiary)]'
          : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
      }`}
    >
      {label}
    </button>
  )
}
