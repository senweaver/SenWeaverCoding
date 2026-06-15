// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { lazy, Suspense, useCallback, useEffect } from 'react'
import { useTranslation } from '../../i18n'
import { useUIStore } from '../../stores/uiStore'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import type { FileSearchHit, FileTreeNode } from '../../types/workspaceFile'
import { EditorTabs } from './EditorTabs'
import { MonacoEditorBoundary } from './MonacoEditorBoundary'
import { FileTree } from './FileTree'
import { WorkspaceSearchBar } from './WorkspaceSearchBar'
import { WorkspaceSplit } from './WorkspaceSplit'
import { OutlinePanel } from './OutlinePanel'
import { ProblemsPanel } from './ProblemsPanel'

const MonacoFileEditor = lazy(() =>
  import('./MonacoFileEditor').then((m) => ({ default: m.MonacoFileEditor })),
)

type Props = {
  sessionId: string | null
  workDir: string | null
  onClose: () => void
  emptyHint?: string
}

export function RightSidebarShell({
  sessionId: _sessionId,
  workDir,
  onClose,
  emptyHint,
}: Props) {
  void _sessionId
  const t = useTranslation()
  const setRoot = useWorkspaceFilesStore((s) => s.setRoot)
  const refreshAll = useWorkspaceFilesStore((s) => s.refreshAll)
  const selectFile = useWorkspaceFilesStore((s) => s.selectFile)
  const activeTab = useWorkspaceFilesStore((s) => s.activeTab)
  const requestNavigation = useWorkspaceFilesStore((s) => s.requestNavigation)
  const addToast = useUIStore((s) => s.addToast)

  useEffect(() => {
    setRoot(workDir)
  }, [setRoot, workDir])

  const handleTreeSelect = useCallback(
    async (node: FileTreeNode) => {
      if (node.isDir) return
      try {
        await selectFile(node.relPath)
      } catch (err) {
        addToast({
          type: 'error',
          message: err instanceof Error ? err.message : String(err),
        })
      }
    },
    [addToast, selectFile],
  )

  const handleSearchSelect = useCallback(
    async (hit: FileSearchHit) => {
      if (hit.isDir) return
      try {
        await selectFile(hit.relPath)
      } catch (err) {
        addToast({
          type: 'error',
          message: err instanceof Error ? err.message : String(err),
        })
      }
    },
    [addToast, selectFile],
  )

  const handleNavigate = useCallback(
    async (relPath: string, position: { line: number; character: number }) => {
      try {
        await requestNavigation(relPath, position.line, position.character)
      } catch (err) {
        addToast({
          type: 'error',
          message: err instanceof Error ? err.message : String(err),
        })
      }
    },
    [addToast, requestNavigation],
  )

  if (!workDir) {
    return (
      <div className="flex h-full min-h-0 flex-col">
        <Header workDir={null} onClose={onClose} onRefresh={() => {}} />
        <div className="flex flex-1 min-h-0 items-center justify-center px-4 text-center text-xs text-[var(--color-text-tertiary)]">
          {emptyHint ?? t('rightSidebar.empty')}
        </div>
      </div>
    )
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <Header workDir={workDir} onClose={onClose} onRefresh={() => void refreshAll()} />
      <WorkspaceSearchBar workDir={workDir} onSelect={handleSearchSelect} />
      <WorkspaceSplit
        left={
          <div className="flex h-full min-h-0 flex-col">
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
              <FileTree workDir={workDir} onSelect={handleTreeSelect} />
            </div>
            <OutlinePanel workDir={workDir} onJump={handleNavigate} />
            <ProblemsPanel workDir={workDir} onJump={handleNavigate} />
          </div>
        }
        right={
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
            <EditorTabs />
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
              {activeTab ? (
                <MonacoEditorBoundary>
                  <Suspense
                    fallback={
                      <div className="flex flex-1 items-center justify-center px-4 text-center text-xs text-[var(--color-text-tertiary)]">
                        {t('files.editorLoading')}
                      </div>
                    }
                  >
                    <MonacoFileEditor workDir={workDir} />
                  </Suspense>
                </MonacoEditorBoundary>
              ) : (
                <div className="flex flex-1 items-center justify-center px-4 text-center text-xs text-[var(--color-text-tertiary)]">
                  {t('files.noFileSelected')}
                </div>
              )}
            </div>
          </div>
        }
      />
    </div>
  )
}

function Header({
  workDir,
  onClose,
  onRefresh,
}: {
  workDir: string | null
  onClose: () => void
  onRefresh: () => void
}) {
  const t = useTranslation()
  return (
    <header className="flex h-9 flex-shrink-0 items-center justify-between border-b border-[var(--color-border)] px-2">
      <div className="flex min-w-0 items-center gap-1.5">
        <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)]">
          folder_open
        </span>
        <span
          className="truncate text-xs font-medium text-[var(--color-text-secondary)]"
          title={workDir ?? ''}
        >
          {workDir ?? t('rightSidebar.empty')}
        </span>
      </div>
      <div className="flex items-center gap-0.5">
        <button
          type="button"
          onClick={onRefresh}
          aria-label={t('rightSidebar.refresh')}
          title={t('rightSidebar.refresh')}
          className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
        >
          <span className="material-symbols-outlined text-[16px]">refresh</span>
        </button>
        <button
          type="button"
          onClick={onClose}
          aria-label={t('rightSidebar.toggleClose')}
          title={t('rightSidebar.toggleClose')}
          className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
        >
          <span className="material-symbols-outlined text-[16px]">close</span>
        </button>
      </div>
    </header>
  )
}
