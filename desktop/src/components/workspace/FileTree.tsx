import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import {
  joinPath,
  nameOf,
  parentOf,
  useWorkspaceFilesStore,
} from '../../stores/workspaceFilesStore'
import type { FileTreeNode } from '../../types/workspaceFile'
import { copyTextToClipboard } from '../chat/clipboard'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import { revealInExplorer } from '../../lib/revealInExplorer'
import { joinWorkspaceAbsPath } from '../../lib/workspacePath'
import { inferLanguageFromPath, languageToMarkdownLang } from '../../lib/extLanguage'
import { workspaceFilesApi } from '../../api/workspaceFiles'
import { FileTreeContextMenu, type ContextMenuTarget } from './FileTreeContextMenu'
import { FileTreeNodeView, type FilterState } from './FileTreeNodeView'
import { InlineNamePrompt } from './InlineNamePrompt'

type Props = {
  workDir: string
  onSelect: (node: FileTreeNode) => void
}

type RenameTarget = {
  relPath: string
  initial: string
}

type CreateTarget = {
  parentRelPath: string
  kind: 'file' | 'folder'
}

const DRAG_MIME = 'application/x-sen-workspace-rel-path'

export function FileTree({ workDir, onSelect }: Props) {
  const t = useTranslation()
  const setRoot = useWorkspaceFilesStore((s) => s.setRoot)
  const root = useWorkspaceFilesStore((s) => s.root)
  const rootEntries = useWorkspaceFilesStore((s) => s.rootEntries)
  const rootLoaded = useWorkspaceFilesStore((s) => s.rootLoaded)
  const rootLoading = useWorkspaceFilesStore((s) => s.rootLoading)
  const rootError = useWorkspaceFilesStore((s) => s.rootError)
  const refreshRoot = useWorkspaceFilesStore((s) => s.refreshRoot)
  const selectedRelPath = useWorkspaceFilesStore((s) => s.selectedRelPath)
  const renameAction = useWorkspaceFilesStore((s) => s.rename)
  const removeAction = useWorkspaceFilesStore((s) => s.remove)
  const createFile = useWorkspaceFilesStore((s) => s.createFile)
  const createDir = useWorkspaceFilesStore((s) => s.createDir)
  const uploadFiles = useWorkspaceFilesStore((s) => s.uploadFiles)
  const addToast = useUIStore((s) => s.addToast)

  const collapseAll = useCallback(() => {
    const dirs = useWorkspaceFilesStore.getState().dirs
    const setExpanded = useWorkspaceFilesStore.getState().setExpanded
    for (const key of Object.keys(dirs)) {
      const dir = dirs[key]
      if (!dir?.expanded) continue

      const sep = key.indexOf('::')
      if (sep < 0) continue
      const relPath = key.slice(sep + 2)
      setExpanded(relPath, false)
    }
  }, [])

  const workspaceName = useMemo(() => {
    if (!workDir) return ''
    const norm = workDir.replace(/\\/g, '/').replace(/\/$/, '')
    const idx = norm.lastIndexOf('/')
    return idx === -1 ? norm : norm.slice(idx + 1)
  }, [workDir])

  const [contextMenu, setContextMenu] = useState<{
    x: number
    y: number
    target: ContextMenuTarget
  } | null>(null)
  const [renameTarget, setRenameTarget] = useState<RenameTarget | null>(null)
  const [createTarget, setCreateTarget] = useState<CreateTarget | null>(null)
  const [isDraggingExternal, setIsDraggingExternal] = useState(false)
  const [filterText, setFilterText] = useState('')
  const fileInputRef = useRef<HTMLInputElement | null>(null)
  const uploadParentRef = useRef<string>('')

  const filterNeedle = filterText.trim()
  const dirsForFilter = useWorkspaceFilesStore((s) =>
    filterNeedle ? s.dirs : undefined,
  )
  const filterState: FilterState | undefined = useMemo(() => {
    if (!filterNeedle) return undefined
    const matches = new Set<string>()
    const ancestors = new Set<string>()
    const target = filterNeedle.toLowerCase()
    if (!root) {
      return { active: true, needle: filterNeedle, matches, ancestors }
    }
    const visit = (entries: FileTreeNode[]) => {
      for (const entry of entries) {
        const lowerName = entry.name.toLowerCase()
        const lowerRel = entry.relPath.toLowerCase()
        if (lowerName.includes(target) || lowerRel.includes(target)) {
          matches.add(entry.relPath)
          let parent = parentOf(entry.relPath)
          while (parent && !ancestors.has(parent)) {
            ancestors.add(parent)
            parent = parentOf(parent)
          }
        }
        if (entry.isDir) {
          const childKey = `${root}::${entry.relPath}`
          const dir = dirsForFilter?.[childKey]
          if (dir?.children?.length) {
            visit(dir.children)
          } else if (entry.children?.length) {
            visit(entry.children)
          }
        }
      }
    }
    visit(rootEntries)
    return { active: true, needle: filterNeedle, matches, ancestors }
  }, [filterNeedle, root, rootEntries, dirsForFilter])

  useEffect(() => {
    setRoot(workDir)
  }, [workDir, setRoot])

  const handleSelect = useCallback(
    (node: FileTreeNode) => {
      if (node.isDir) return
      onSelect(node)
    },
    [onSelect],
  )

  const handleContextMenu = useCallback(
    (event: React.MouseEvent, node: FileTreeNode) => {
      event.preventDefault()
      setContextMenu({ x: event.clientX, y: event.clientY, target: { kind: 'node', node } })
    },
    [],
  )

  const handleRootContextMenu = useCallback((event: React.MouseEvent) => {
    event.preventDefault()
    setContextMenu({ x: event.clientX, y: event.clientY, target: { kind: 'root' } })
  }, [])

  const handleDragStart = useCallback((event: React.DragEvent, node: FileTreeNode) => {
    event.dataTransfer.setData(DRAG_MIME, node.relPath)
    event.dataTransfer.effectAllowed = 'move'
  }, [])

  const handleDrop = useCallback(
    (event: React.DragEvent, targetNode: FileTreeNode | null) => {
      event.preventDefault()
      event.stopPropagation()
      setIsDraggingExternal(false)
      const fromRel = event.dataTransfer.getData(DRAG_MIME)
      const targetParentRel = targetNode
        ? targetNode.isDir
          ? targetNode.relPath
          : parentOf(targetNode.relPath)
        : ''
      if (fromRel) {
        if (fromRel === '' || fromRel === targetParentRel) return
        if (targetParentRel === fromRel || targetParentRel.startsWith(`${fromRel}/`)) {
          addToast({ type: 'warning', message: t('files.dropTargetInvalid') })
          return
        }
        const name = nameOf(fromRel)
        const next = joinPath(targetParentRel, name)
        renameAction(fromRel, next).catch((err) => {
          addToast({ type: 'error', message: err instanceof Error ? err.message : String(err) })
        })
        return
      }

      const files = Array.from(event.dataTransfer.files ?? [])
      if (files.length > 0) {
        uploadFiles(targetParentRel, files)
          .then((count) => {
            if (count > 0) {
              addToast({ type: 'success', message: t('files.uploadSuccess', { count }) })
            }
          })
          .catch((err) => {
            addToast({
              type: 'error',
              message: t('files.uploadError', {
                message: err instanceof Error ? err.message : String(err),
              }),
            })
          })
      }
    },
    [addToast, renameAction, t, uploadFiles],
  )

  const handleNewFile = useCallback((parent: FileTreeNode | null) => {
    setCreateTarget({ parentRelPath: parent?.relPath ?? '', kind: 'file' })
  }, [])

  const handleNewFolder = useCallback((parent: FileTreeNode | null) => {
    setCreateTarget({ parentRelPath: parent?.relPath ?? '', kind: 'folder' })
  }, [])

  const handleUpload = useCallback((parent: FileTreeNode | null) => {
    uploadParentRef.current = parent?.relPath ?? ''
    fileInputRef.current?.click()
  }, [])

  const handleRename = useCallback((node: FileTreeNode) => {
    setRenameTarget({ relPath: node.relPath, initial: node.name })
  }, [])

  const handleDelete = useCallback(
    async (node: FileTreeNode) => {
      const ok = window.confirm(t('files.deleteConfirm', { name: node.name }))
      if (!ok) return
      try {
        await removeAction(node.relPath, node.isDir)
      } catch (err) {
        addToast({
          type: 'error',
          message: err instanceof Error ? err.message : String(err),
        })
      }
    },
    [addToast, removeAction, t],
  )

  const handleCreateSubmit = useCallback(
    async (value: string) => {
      if (!createTarget) return
      const target = createTarget
      setCreateTarget(null)
      try {
        if (target.kind === 'file') {
          await createFile(target.parentRelPath, value)
        } else {
          await createDir(target.parentRelPath, value)
        }
      } catch (err) {
        addToast({
          type: 'error',
          message: err instanceof Error ? err.message : String(err),
        })
      }
    },
    [addToast, createDir, createFile, createTarget],
  )

  const handleRenameSubmit = useCallback(
    async (value: string) => {
      if (!renameTarget) return
      const target = renameTarget
      setRenameTarget(null)
      const next = joinPath(parentOf(target.relPath), value)
      try {
        await renameAction(target.relPath, next)
      } catch (err) {
        addToast({
          type: 'error',
          message: err instanceof Error ? err.message : String(err),
        })
      }
    },
    [addToast, renameAction, renameTarget],
  )

  const handleCopyAbsolutePath = useCallback(
    async (node: FileTreeNode) => {
      const abs = joinWorkspaceAbsPath(workDir, node.relPath)
      const ok = await copyTextToClipboard(abs)
      addToast({
        type: ok ? 'success' : 'error',
        message: ok ? t('files.preview.copied') : t('files.preview.copyFailed'),
      })
    },
    [addToast, t, workDir],
  )

  const handleCopyRelativePath = useCallback(
    async (node: FileTreeNode) => {
      const ok = await copyTextToClipboard(node.relPath)
      addToast({
        type: ok ? 'success' : 'error',
        message: ok ? t('files.preview.copied') : t('files.preview.copyFailed'),
      })
    },
    [addToast, t],
  )

  const handleCopyAsMarkdown = useCallback(
    async (node: FileTreeNode) => {
      if (node.isDir) return
      const currentRoot = root
      if (!currentRoot) return
      try {
        const res = await workspaceFilesApi.readFile({
          root: currentRoot,
          path: node.relPath,
        })
        let content = ''
        if (res.encoding === 'base64') {
          addToast({
            type: 'error',
            message: t('files.tree.copyMarkdownFailed'),
          })
          return
        }
        content = res.content ?? ''
        const lang = languageToMarkdownLang(inferLanguageFromPath(node.relPath))
        const fenceHeader = lang
          ? `\`\`\`${lang} ${node.relPath}`
          : `\`\`\`${node.relPath}`
        const body =
          content.length === 0 || content.endsWith('\n') ? content : `${content}\n`
        const markdown = `${fenceHeader}\n${body}\`\`\`\n`
        const ok = await copyTextToClipboard(markdown)
        addToast({
          type: ok ? 'success' : 'error',
          message: ok
            ? t('files.tree.copyMarkdownDone')
            : t('files.tree.copyMarkdownFailed'),
        })
      } catch {
        addToast({
          type: 'error',
          message: t('files.tree.copyMarkdownFailed'),
        })
      }
    },
    [addToast, root, t],
  )

  const handleReveal = useCallback(
    async (node: FileTreeNode) => {
      try {
        const abs = joinWorkspaceAbsPath(workDir, node.relPath)
        await revealInExplorer(abs)
      } catch (err) {
        addToast({
          type: 'error',
          message: t('files.preview.revealFailed', {
            message: err instanceof Error ? err.message : String(err),
          }),
        })
      }
    },
    [addToast, t, workDir],
  )

  const canReveal = useMemo(() => isTauriRuntime(), [])

  const containerProps = useMemo(
    () => ({
      onDragOver: (event: React.DragEvent) => {
        event.preventDefault()
        if (event.dataTransfer.types.includes('Files')) {
          setIsDraggingExternal(true)
          event.dataTransfer.dropEffect = 'copy'
        }
      },
      onDragLeave: () => setIsDraggingExternal(false),
      onDrop: (event: React.DragEvent) => handleDrop(event, null),
    }),
    [handleDrop],
  )

  if (!root) {
    return null
  }

  return (
    <div
      role="tree"
      onContextMenu={handleRootContextMenu}
      className="relative flex-1 overflow-y-auto"
      {...containerProps}
    >
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={(event) => {
          const files = Array.from(event.target.files ?? [])
          if (files.length > 0) {
            uploadFiles(uploadParentRef.current, files)
              .then((count) => {
                if (count > 0) {
                  addToast({
                    type: 'success',
                    message: t('files.uploadSuccess', { count }),
                  })
                }
              })
              .catch((err) => {
                addToast({
                  type: 'error',
                  message: t('files.uploadError', {
                    message: err instanceof Error ? err.message : String(err),
                  }),
                })
              })
          }
          event.target.value = ''
        }}
      />

      <div className="group sticky top-0 z-10 flex h-7 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-2">
        <span
          className="truncate text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]"
          title={workDir}
        >
          {workspaceName}
        </span>
        <div className="flex items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
          <ToolbarButton
            icon="note_add"
            label={t('files.newFile')}
            onClick={() => handleNewFile(null)}
          />
          <ToolbarButton
            icon="create_new_folder"
            label={t('files.newFolder')}
            onClick={() => handleNewFolder(null)}
          />
          <ToolbarButton
            icon="upload"
            label={t('files.upload')}
            onClick={() => handleUpload(null)}
          />
          <ToolbarButton
            icon="unfold_less"
            label={t('files.collapseAll')}
            onClick={collapseAll}
          />
          <ToolbarButton
            icon="refresh"
            label={t('files.refresh')}
            onClick={() => {
              void refreshRoot()
            }}
          />
        </div>
      </div>

      <div className="sticky top-7 z-[9] flex h-7 items-center gap-1 border-b border-[var(--color-border)] bg-[var(--color-surface)] px-2">
        <span
          aria-hidden="true"
          className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]"
        >
          filter_alt
        </span>
        <input
          type="text"
          value={filterText}
          onChange={(e) => setFilterText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              e.stopPropagation()
              setFilterText('')
            }
          }}
          placeholder={t('files.filterPlaceholder')}
          spellCheck={false}
          className="h-5 flex-1 bg-transparent text-[11px] text-[var(--color-text-primary)] placeholder-[var(--color-text-tertiary)] outline-none"
        />
        {filterText && (
          <button
            type="button"
            onClick={() => setFilterText('')}
            aria-label={t('files.filterClear')}
            title={t('files.filterClear')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[14px]">close</span>
          </button>
        )}
      </div>

      {rootError && (
        <div className="px-3 py-2 text-[11px] text-[var(--color-danger)]">
          {t('files.errorLoadingTree', { message: rootError })}
        </div>
      )}
      {rootLoading && !rootLoaded && (
        <div className="px-3 py-2 text-[11px] text-[var(--color-text-tertiary)]">
          {t('rightSidebar.loading')}
        </div>
      )}
      {createTarget && createTarget.parentRelPath === '' && (
        <div className="px-2 py-0.5">
          <InlineNamePrompt
            placeholder={
              createTarget.kind === 'file'
                ? t('files.newFile')
                : t('files.newFolder')
            }
            onCancel={() => setCreateTarget(null)}
            onSubmit={handleCreateSubmit}
          />
        </div>
      )}

      {rootEntries.map((entry) => (
        <FileTreeNodeView
          key={entry.relPath}
          node={entry}
          depth={0}
          selectedRelPath={selectedRelPath}
          renameTarget={renameTarget}
          createTarget={createTarget}
          filter={filterState}
          onSelect={handleSelect}
          onContextMenu={handleContextMenu}
          onDrop={handleDrop}
          onDragStart={handleDragStart}
          onRenameSubmit={handleRenameSubmit}
          onRenameCancel={() => setRenameTarget(null)}
          onCreateSubmit={handleCreateSubmit}
          onCreateCancel={() => setCreateTarget(null)}
        />
      ))}

      {!rootLoading && rootEntries.length === 0 && rootLoaded && (
        <div className="px-3 py-3 text-center text-[11px] text-[var(--color-text-tertiary)] italic">
          {t('files.empty')}
        </div>
      )}

      {filterState && filterState.matches.size === 0 && rootEntries.length > 0 && (
        <div className="px-3 py-3 text-center text-[11px] text-[var(--color-text-tertiary)] italic">
          {t('files.filterEmpty')}
        </div>
      )}

      {isDraggingExternal && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-[var(--color-accent)]/10 text-[12px] text-[var(--color-accent)]">
          {t('files.dropToUpload')}
        </div>
      )}

      {contextMenu && (
        <FileTreeContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          target={contextMenu.target}
          canReveal={canReveal}
          onClose={() => setContextMenu(null)}
          onNewFile={handleNewFile}
          onNewFolder={handleNewFolder}
          onRename={handleRename}
          onDelete={handleDelete}
          onRefresh={refreshRoot}
          onUpload={handleUpload}
          onCopyAbsolutePath={handleCopyAbsolutePath}
          onCopyRelativePath={handleCopyRelativePath}
          onCopyAsMarkdown={handleCopyAsMarkdown}
          onReveal={handleReveal}
        />
      )}
    </div>
  )
}

function ToolbarButton({
  icon,
  label,
  onClick,
}: {
  icon: string
  label: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation()
        onClick()
      }}
      aria-label={label}
      title={label}
      className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
    >
      <span className="material-symbols-outlined text-[14px]">{icon}</span>
    </button>
  )
}

