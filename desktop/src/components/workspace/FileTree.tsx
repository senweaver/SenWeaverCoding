// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import { useTerminalPanelStore } from '../../stores/terminalPanelStore'
import { useLanStore } from '../../stores/lanStore'
import { useLanGroupStore } from '../../stores/lanGroupStore'
import { useLanShareStore } from '../../stores/lanShareStore'
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
import { useFileDragStore } from '../../stores/fileDragStore'
import type { FileRefDragPayload } from '../chat/composerRefs'
import { InlineNamePrompt } from './InlineNamePrompt'
import { DeleteConfirmModal } from './DeleteConfirmModal'

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
  const root = useWorkspaceFilesStore((s) => s.root)
  const rootEntries = useWorkspaceFilesStore((s) => s.rootEntries)
  const rootLoaded = useWorkspaceFilesStore((s) => s.rootLoaded)
  const rootLoading = useWorkspaceFilesStore((s) => s.rootLoading)
  const rootError = useWorkspaceFilesStore((s) => s.rootError)
  const refreshAll = useWorkspaceFilesStore((s) => s.refreshAll)
  const loadDirectory = useWorkspaceFilesStore((s) => s.loadDirectory)
  const selectedRelPath = useWorkspaceFilesStore((s) => s.selectedRelPath)
  const renameAction = useWorkspaceFilesStore((s) => s.rename)
  const removeAction = useWorkspaceFilesStore((s) => s.remove)
  const createFile = useWorkspaceFilesStore((s) => s.createFile)
  const createDir = useWorkspaceFilesStore((s) => s.createDir)
  const uploadFiles = useWorkspaceFilesStore((s) => s.uploadFiles)
  const clipboard = useWorkspaceFilesStore((s) => s.clipboard)
  const copyJob = useWorkspaceFilesStore((s) => s.copyJob)
  const copyToClipboard = useWorkspaceFilesStore((s) => s.copyToClipboard)
  const cutToClipboard = useWorkspaceFilesStore((s) => s.cutToClipboard)
  const pasteInto = useWorkspaceFilesStore((s) => s.pasteInto)
  const cancelCopy = useWorkspaceFilesStore((s) => s.cancelCopy)
  const addToast = useUIStore((s) => s.addToast)
  const lanPeers = useLanStore((s) => s.peers)
  const lanEnabled = useLanStore((s) => s.identity?.running ?? false)
  const lanSendFile = useLanStore((s) => s.sendFile)

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
  const [deleteTarget, setDeleteTarget] = useState<FileTreeNode | null>(null)
  const [isDraggingExternal, setIsDraggingExternal] = useState(false)
  const [filterText, setFilterText] = useState('')
  const [focusedRelPath, setFocusedRelPath] = useState<string | null>(null)
  const fileInputRef = useRef<HTMLInputElement | null>(null)
  const uploadParentRef = useRef<string>('')
  const treeRef = useRef<HTMLDivElement | null>(null)

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
    if (!filterNeedle || filterNeedle.length < 2) return
    if (!root) return
    const dirs = useWorkspaceFilesStore.getState().dirs
    const candidates: string[] = []
    const collect = (entries: FileTreeNode[], depth: number) => {
      if (depth > 3) return
      for (const entry of entries) {
        if (!entry.isDir) continue
        const key = `${root}::${entry.relPath}`
        const dir = dirs[key]
        if (!dir || (!dir.loaded && !dir.loading)) {
          candidates.push(entry.relPath)
        } else if (dir.children?.length) {
          collect(dir.children, depth + 1)
        }
      }
    }
    collect(rootEntries, 0)
    if (candidates.length === 0) return

    let cancelled = false
    let inFlight = 0
    const queue = [...candidates]
    const handle = window.setTimeout(() => {
      const pump = () => {
        if (cancelled) return
        while (inFlight < 4 && queue.length > 0) {
          const target = queue.shift()
          if (!target) continue
          inFlight += 1
          loadDirectory(target)
            .catch(() => {
              /* ignore */
            })
            .finally(() => {
              inFlight -= 1
              if (!cancelled) pump()
            })
        }
      }
      pump()
    }, 200)
    return () => {
      cancelled = true
      window.clearTimeout(handle)
    }
  }, [filterNeedle, root, rootEntries, loadDirectory])

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
      event.stopPropagation()
      setContextMenu({ x: event.clientX, y: event.clientY, target: { kind: 'node', node } })
    },
    [],
  )

  const handleRootContextMenu = useCallback((event: React.MouseEvent) => {
    event.preventDefault()
    setContextMenu({ x: event.clientX, y: event.clientY, target: { kind: 'root' } })
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

  const handleTreeMoveDrop = useCallback(
    (payload: FileRefDragPayload, x: number, y: number) => {
      const targetEl = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest(
        '[data-tree-relpath]',
      ) as HTMLElement | null
      const targetRel = targetEl?.getAttribute('data-tree-relpath') ?? ''
      const targetIsDir = targetEl?.getAttribute('data-tree-isdir') === '1'
      const targetParentRel = targetEl ? (targetIsDir ? targetRel : parentOf(targetRel)) : ''
      const fromRel = payload.relPath
      if (!fromRel || fromRel === targetParentRel) return
      if (targetParentRel === fromRel || targetParentRel.startsWith(`${fromRel}/`)) {
        addToast({ type: 'warning', message: t('files.dropTargetInvalid') })
        return
      }
      const name = nameOf(fromRel)
      const next = joinPath(targetParentRel, name)
      if (next === fromRel) return
      renameAction(fromRel, next).catch((err) => {
        addToast({ type: 'error', message: err instanceof Error ? err.message : String(err) })
      })
    },
    [addToast, renameAction, t],
  )

  useEffect(() => {
    const store = useFileDragStore.getState()
    store.registerZone({
      id: 'file-tree',
      getRect: () => treeRef.current?.getBoundingClientRect() ?? null,
      onDrop: handleTreeMoveDrop,
    })
    return () => store.unregisterZone('file-tree')
  }, [handleTreeMoveDrop])

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

  const handleDelete = useCallback((node: FileTreeNode) => {
    setDeleteTarget(node)
  }, [])

  const confirmDelete = useCallback(async () => {
    if (!deleteTarget) return
    const node = deleteTarget
    try {
      await removeAction(node.relPath, node.isDir)
      setDeleteTarget(null)
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    }
  }, [addToast, deleteTarget, removeAction])

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

  const handleSendToPeer = useCallback(
    async (node: FileTreeNode, peerId: string) => {
      try {
        const abs = joinWorkspaceAbsPath(workDir, node.relPath)
        await lanSendFile(peerId, abs)
        addToast({
          type: 'success',
          message: t('files.tree.lanSendStarted', { name: node.name }),
        })
      } catch (err) {
        addToast({
          type: 'error',
          message: t('files.tree.lanSendFailed', {
            message: err instanceof Error ? err.message : String(err),
          }),
        })
      }
    },
    [addToast, lanSendFile, t, workDir],
  )

  const handleUploadToGroup = useCallback(
    (node: FileTreeNode) => {
      const abs = joinWorkspaceAbsPath(workDir, node.relPath)
      useLanShareStore.getState().closePanel()
      useUIStore.getState().closeTemplateLibrary()
      useLanGroupStore.getState().stageUpload(abs)
    },
    [workDir],
  )

  const handleShareToLan = useCallback(
    (node: FileTreeNode) => {
      const abs = joinWorkspaceAbsPath(workDir, node.relPath)
      void useLanShareStore.getState().addShare(abs)
      useLanGroupStore.getState().closePanel()
      useUIStore.getState().closeTemplateLibrary()
      useLanShareStore.getState().openPanel()
      addToast({ type: 'success', message: t('lanShare.sharedToast', { name: node.name }) })
    },
    [addToast, t, workDir],
  )

  const handleOpenInTerminal = useCallback(
    (node: FileTreeNode) => {
      try {
        const abs = joinWorkspaceAbsPath(workDir, node.relPath)
        const store = useTerminalPanelStore.getState()
        store.setOpen(true)
        store.openNewTab({ cwd: abs })
      } catch (err) {
        addToast({
          type: 'error',
          message: t('files.tree.openTerminalFailed', {
            message: err instanceof Error ? err.message : String(err),
          }),
        })
      }
    },
    [addToast, t, workDir],
  )

  const canReveal = useMemo(() => isTauriRuntime(), [])
  const canOpenTerminal = useMemo(() => isTauriRuntime(), [])

  const dirsForFlat = useWorkspaceFilesStore((s) => s.dirs)
  const visibleNodes = useMemo(() => {
    if (!root) return [] as FileTreeNode[]
    const out: FileTreeNode[] = []
    const visit = (entries: FileTreeNode[]) => {
      for (const entry of entries) {
        if (filterState?.active) {
          const isMatch = filterState.matches.has(entry.relPath)
          const isAncestor = filterState.ancestors.has(entry.relPath)
          if (!isMatch && !isAncestor) continue
        }
        out.push(entry)
        if (entry.isDir) {
          const key = `${root}::${entry.relPath}`
          const dir = dirsForFlat[key]
          if (dir?.expanded) {
            if (dir.children?.length) {
              visit(dir.children)
            } else if (entry.children?.length) {
              visit(entry.children)
            }
          }
        }
      }
    }
    visit(rootEntries)
    return out
  }, [dirsForFlat, filterState, root, rootEntries])

  const visibleByPath = useMemo(() => {
    const map = new Map<string, FileTreeNode>()
    for (const node of visibleNodes) map.set(node.relPath, node)
    return map
  }, [visibleNodes])

  const setExpanded = useWorkspaceFilesStore((s) => s.setExpanded)

  const handleCopyNode = useCallback(
    (node: FileTreeNode) => {
      if (!node || node.relPath === '') return
      copyToClipboard(node.relPath, node.isDir)
    },
    [copyToClipboard],
  )

  const handleCutNode = useCallback(
    (node: FileTreeNode) => {
      if (!node || node.relPath === '') return
      cutToClipboard(node.relPath, node.isDir)
    },
    [cutToClipboard],
  )

  const handlePasteInto = useCallback(
    (targetDir: string) => {
      if (!clipboard) return
      void pasteInto(targetDir)
    },
    [clipboard, pasteInto],
  )

  const handleTreeKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLDivElement>) => {
      if (renameTarget || createTarget) return
      const tag = (event.target as HTMLElement | null)?.tagName?.toLowerCase()
      if (tag === 'input' || tag === 'textarea') return

      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'c') {
        const activeRel = focusedRelPath ?? selectedRelPath
        const activeNode = activeRel ? visibleByPath.get(activeRel) : undefined
        if (activeNode && activeNode.relPath !== '') {
          event.preventDefault()
          handleCopyNode(activeNode)
        }
        return
      }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'x') {
        const activeRel = focusedRelPath ?? selectedRelPath
        const activeNode = activeRel ? visibleByPath.get(activeRel) : undefined
        if (activeNode && activeNode.relPath !== '') {
          event.preventDefault()
          handleCutNode(activeNode)
        }
        return
      }
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'v') {
        if (!clipboard) return
        event.preventDefault()
        const activeRel = focusedRelPath ?? selectedRelPath
        const activeNode = activeRel ? visibleByPath.get(activeRel) : undefined
        const targetDir = activeNode
          ? activeNode.isDir
            ? activeNode.relPath
            : parentOf(activeNode.relPath)
          : ''
        handlePasteInto(targetDir)
        return
      }

      if (visibleNodes.length === 0) return

      const currentRel = focusedRelPath ?? selectedRelPath ?? visibleNodes[0]?.relPath ?? null
      const currentIdx = currentRel
        ? visibleNodes.findIndex((n) => n.relPath === currentRel)
        : -1
      const idx = currentIdx < 0 ? 0 : currentIdx
      const node = visibleNodes[idx]

      if (event.key === 'ArrowDown') {
        event.preventDefault()
        const next = Math.min(visibleNodes.length - 1, idx + 1)
        const nextNode = visibleNodes[next]
        if (nextNode) setFocusedRelPath(nextNode.relPath)
        return
      }
      if (event.key === 'ArrowUp') {
        event.preventDefault()
        const prev = Math.max(0, idx - 1)
        const prevNode = visibleNodes[prev]
        if (prevNode) setFocusedRelPath(prevNode.relPath)
        return
      }
      if (event.key === 'ArrowRight') {
        if (!node) return
        event.preventDefault()
        if (node.isDir) {
          const key = `${root}::${node.relPath}`
          const dir = dirsForFlat[key]
          if (!dir?.expanded) {
            setExpanded(node.relPath, true)
          } else {
            const nextIdx = Math.min(visibleNodes.length - 1, idx + 1)
            const nextNode = visibleNodes[nextIdx]
            if (nextNode) setFocusedRelPath(nextNode.relPath)
          }
        }
        return
      }
      if (event.key === 'ArrowLeft') {
        if (!node) return
        event.preventDefault()
        const key = `${root}::${node.relPath}`
        const dir = dirsForFlat[key]
        if (node.isDir && dir?.expanded) {
          setExpanded(node.relPath, false)
        } else {
          const parent = parentOf(node.relPath)
          if (parent) setFocusedRelPath(parent)
        }
        return
      }
      if (event.key === 'Enter') {
        if (!node) return
        event.preventDefault()
        if (node.isDir) {
          const key = `${root}::${node.relPath}`
          const dir = dirsForFlat[key]
          setExpanded(node.relPath, !dir?.expanded)
        } else {
          handleSelect(node)
        }
        return
      }
      if (event.key === 'F2') {
        if (!node) return
        event.preventDefault()
        handleRename(node)
        return
      }
      if (event.key === 'Delete' || event.key === 'Backspace') {
        if (!node) return
        if (event.key === 'Backspace' && !(event.metaKey || event.ctrlKey)) return
        event.preventDefault()
        void handleDelete(node)
      }
    },
    [
      clipboard,
      createTarget,
      dirsForFlat,
      focusedRelPath,
      handleCopyNode,
      handleCutNode,
      handleDelete,
      handlePasteInto,
      handleRename,
      handleSelect,
      renameTarget,
      root,
      selectedRelPath,
      setExpanded,
      visibleByPath,
      visibleNodes,
    ],
  )

  useEffect(() => {
    if (!focusedRelPath) return
    if (!visibleByPath.has(focusedRelPath)) {
      setFocusedRelPath(null)
    }
  }, [focusedRelPath, visibleByPath])

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
      ref={treeRef}
      role="tree"
      tabIndex={0}
      onKeyDown={handleTreeKeyDown}
      onContextMenu={handleRootContextMenu}
      className="relative flex-1 overflow-y-auto outline-none"
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
              void refreshAll()
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

      {copyJob && (
        <div className="sticky top-14 z-[8] border-b border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1.5">
          <div className="flex items-center gap-2">
            <div className="min-w-0 flex-1">
              <p className="truncate text-[11px] text-[var(--color-text-secondary)]">
                {t('workspace.copying', {
                  name: copyJob.fromName,
                  dir: copyJob.toDir,
                  percent:
                    copyJob.bytesTotal > 0
                      ? Math.min(
                          100,
                          Math.round((copyJob.bytesDone / copyJob.bytesTotal) * 100),
                        )
                      : 0,
                })}
              </p>
            </div>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                cancelCopy()
              }}
              aria-label={t('workspace.copyCancel')}
              title={t('workspace.copyCancel')}
              className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-danger)]"
            >
              <span className="material-symbols-outlined text-[14px]">close</span>
            </button>
          </div>
          <div className="mt-1 h-1.5 overflow-hidden rounded-full bg-[var(--color-surface)]">
            {copyJob.bytesTotal > 0 ? (
              <div
                className="h-full bg-[var(--color-text-accent)] transition-all duration-200"
                style={{
                  width: `${Math.min(
                    100,
                    Math.round((copyJob.bytesDone / copyJob.bytesTotal) * 100),
                  )}%`,
                }}
              />
            ) : (
              <div className="h-full w-1/3 rounded-full bg-[var(--color-text-accent)]/75 animate-pulse" />
            )}
          </div>
        </div>
      )}

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
          focusedRelPath={focusedRelPath}
          renameTarget={renameTarget}
          createTarget={createTarget}
          filter={filterState}
          onSelect={handleSelect}
          onFocus={setFocusedRelPath}
          onContextMenu={handleContextMenu}
          onDrop={handleDrop}
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
          canOpenTerminal={canOpenTerminal}
          onClose={() => setContextMenu(null)}
          onNewFile={handleNewFile}
          onNewFolder={handleNewFolder}
          onRename={handleRename}
          onDelete={handleDelete}
          onRefresh={refreshAll}
          onUpload={handleUpload}
          onCopyAbsolutePath={handleCopyAbsolutePath}
          onCopyRelativePath={handleCopyRelativePath}
          onCopyAsMarkdown={handleCopyAsMarkdown}
          onReveal={handleReveal}
          onOpenInTerminal={handleOpenInTerminal}
          onCopyEntry={handleCopyNode}
          onCutEntry={handleCutNode}
          onPasteInto={handlePasteInto}
          canPaste={clipboard !== null}
          lanEnabled={lanEnabled}
          lanPeers={lanPeers}
          onSendToPeer={handleSendToPeer}
          onUploadToGroup={handleUploadToGroup}
          onShareToLan={handleShareToLan}
        />
      )}

      {deleteTarget && (
        <DeleteConfirmModal
          node={deleteTarget}
          onCancel={() => setDeleteTarget(null)}
          onConfirm={confirmDelete}
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

