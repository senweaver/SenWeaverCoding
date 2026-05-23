import { useEffect, useState, useCallback, useMemo, useRef } from 'react'
import { useSessionStore } from '../../stores/sessionStore'
import { useSessionRunStateStore } from '../../stores/sessionRunStateStore'
import { useUIStore } from '../../stores/uiStore'
import { useTranslation } from '../../i18n'
import { ProjectFilter } from './ProjectFilter'
import { ConfirmDialog } from '../shared/ConfirmDialog'
import type { SessionListItem } from '../../types/session'
import { useTabStore, SCHEDULED_TAB_ID } from '../../stores/tabStore'
import { useChatStore } from '../../stores/chatStore'
import { focusSession } from '../../lib/focusSession'
import { useBrowserPanelStore } from '../../stores/browserPanelStore'
import { isPlaceholderTitle, resolveSessionTitle } from '../../utils/sessionTitle'
import { Spinner } from '../shared/Spinner'
import { useWorkspaceQueueStore } from '../../stores/workspaceQueueStore'
import { AgentMonitorPanel } from './AgentMonitorPanel'

const isTauri = typeof window !== 'undefined' && ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)

type TimeGroup = 'today' | 'yesterday' | 'last7days' | 'last30days' | 'older'

const TIME_GROUP_ORDER: TimeGroup[] = ['today', 'yesterday', 'last7days', 'last30days', 'older']

const SIDEBAR_GROUP_PAGE_SIZE = 9

const WORKSPACE_UNKNOWN_KEY = '__unknown__'

type WorkspaceGroup = {
  workspaceKey: string
  workspaceLabel: string
  workspacePath: string
  workspaceExists: boolean
  latestModifiedAt: number
  totalCount: number
  timeGroups: Map<TimeGroup, SessionListItem[]>
}

export function Sidebar() {
  const sessions = useSessionStore((s) => s.sessions)
  const runningSessions = useSessionRunStateStore((s) => s.running)
  const queueState = useWorkspaceQueueStore((s) => s.queues)
  const queuedCounts = useMemo(() => {
    const counts: Record<string, number> = {}
    for (const list of Object.values(queueState)) {
      for (const item of list) {
        counts[item.sessionId] = (counts[item.sessionId] ?? 0) + 1
      }
    }
    return counts
  }, [queueState])
  const selectedProjects = useSessionStore((s) => s.selectedProjects)
  const error = useSessionStore((s) => s.error)
  const fetchSessions = useSessionStore((s) => s.fetchSessions)
  const deleteSession = useSessionStore((s) => s.deleteSession)
  const renameSession = useSessionStore((s) => s.renameSession)
  const addToast = useUIStore((s) => s.addToast)
  const sidebarOpen = useUIStore((s) => s.sidebarOpen)
  const settingsOverlayOpen = useUIStore((s) => s.settingsOverlayOpen)
  const activeTabId = useTabStore((s) => s.activeTabId)
  const closeTab = useTabStore((s) => s.closeTab)
  const disconnectSession = useChatStore((s) => s.disconnectSession)
  const [searchQuery, setSearchQuery] = useState('')
  const [contextMenu, setContextMenu] = useState<{ id: string; x: number; y: number } | null>(null)
  const [pendingDeleteSessionId, setPendingDeleteSessionId] = useState<string | null>(null)
  const [renamingId, setRenamingId] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState('')

  const [expandedWorkspaces, setExpandedWorkspaces] = useState<Set<string>>(new Set())
  const [expandedTimeGroups, setExpandedTimeGroups] = useState<Set<string>>(new Set())
  const [workspacesShowMore, setWorkspacesShowMore] = useState(false)
  const [timeGroupsShowMore, setTimeGroupsShowMore] = useState<Set<string>>(new Set())

  useEffect(() => {
    fetchSessions()
  }, [fetchSessions])

  useEffect(() => {
    if (!contextMenu || sidebarOpen) return
    setContextMenu(null)
  }, [contextMenu, sidebarOpen])

  useEffect(() => {
    if (!contextMenu) return
    const close = () => setContextMenu(null)
    document.addEventListener('click', close)
    return () => document.removeEventListener('click', close)
  }, [contextMenu])

  const t = useTranslation()

  const filteredSessions = useMemo(() => {
    let result = sessions
    if (selectedProjects.length > 0) {
      result = result.filter((s) => selectedProjects.includes(s.projectPath))
    }
    if (searchQuery) {
      const q = searchQuery.toLowerCase()
      result = result.filter((s) => s.title.toLowerCase().includes(q))
    }
    return result
  }, [sessions, selectedProjects, searchQuery])

  const inSearchMode = searchQuery.trim().length > 0

  const unknownWorkspaceLabel = t('sidebar.other')
  const workspaceGroups = useMemo(
    () => groupByWorkspaceAndTime(filteredSessions, unknownWorkspaceLabel),
    [filteredSessions, unknownWorkspaceLabel],
  )

  const currentWorkspaceKey = useMemo(() => {
    if (!activeTabId) return null
    const s = sessions.find((x) => x.id === activeTabId)
    if (!s) return null
    const workDir = s.workDir?.trim() ?? ''
    return workDir || WORKSPACE_UNKNOWN_KEY
  }, [sessions, activeTabId])

  const initializedWorkspacesRef = useRef<Set<string>>(new Set())
  useEffect(() => {
    if (!currentWorkspaceKey) return
    if (initializedWorkspacesRef.current.has(currentWorkspaceKey)) return
    initializedWorkspacesRef.current.add(currentWorkspaceKey)
    setExpandedWorkspaces((prev) => {
      if (prev.has(currentWorkspaceKey)) return prev
      const next = new Set(prev)
      next.add(currentWorkspaceKey)
      return next
    })
    setExpandedTimeGroups((prev) => {
      const next = new Set(prev)
      let changed = false
      for (const tg of TIME_GROUP_ORDER) {
        const key = `${currentWorkspaceKey}::${tg}`
        if (!next.has(key)) {
          next.add(key)
          changed = true
        }
      }
      return changed ? next : prev
    })
  }, [currentWorkspaceKey])

  const toggleWorkspace = useCallback((workspaceKey: string) => {
    setExpandedWorkspaces((prev) => {
      const next = new Set(prev)
      if (next.has(workspaceKey)) next.delete(workspaceKey)
      else next.add(workspaceKey)
      return next
    })
  }, [])

  const toggleTimeGroup = useCallback((tgKey: string) => {
    setExpandedTimeGroups((prev) => {
      const next = new Set(prev)
      if (next.has(tgKey)) next.delete(tgKey)
      else next.add(tgKey)
      return next
    })
  }, [])

  const revealMoreSessions = useCallback((tgKey: string) => {
    setTimeGroupsShowMore((prev) => {
      if (prev.has(tgKey)) return prev
      const next = new Set(prev)
      next.add(tgKey)
      return next
    })
  }, [])

  const handleContextMenu = useCallback((e: React.MouseEvent, id: string) => {
    e.preventDefault()
    setContextMenu({ id, x: e.clientX, y: e.clientY })
  }, [])

  const handleDelete = useCallback((id: string) => {
    setContextMenu(null)
    setPendingDeleteSessionId(id)
  }, [])

  const confirmDelete = useCallback(async () => {
    if (!pendingDeleteSessionId) return
    await deleteSession(pendingDeleteSessionId)
    disconnectSession(pendingDeleteSessionId)
    closeTab(pendingDeleteSessionId)
    setPendingDeleteSessionId(null)
  }, [closeTab, deleteSession, disconnectSession, pendingDeleteSessionId])

  const handleStartRename = useCallback((id: string, currentTitle: string) => {
    setContextMenu(null)
    setRenamingId(id)
    setRenameValue(currentTitle)
  }, [])

  const handleFinishRename = useCallback(async () => {
    if (renamingId && renameValue.trim()) {
      await renameSession(renamingId, renameValue.trim())
    }
    setRenamingId(null)
    setRenameValue('')
  }, [renamingId, renameValue, renameSession])

  const startDraggingRef = useRef<(() => Promise<void>) | null>(null)

  useEffect(() => {
    if (!isTauri) return
    import(/* @vite-ignore */ '@tauri-apps/api/window')
      .then(({ getCurrentWindow }) => {
        const win = getCurrentWindow()
        startDraggingRef.current = () => win.startDragging()
      })
      .catch(() => {})
  }, [])

  const handleSidebarDrag = useCallback((e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('button, input, textarea, select, a, [role="button"]')) return
    startDraggingRef.current?.()
  }, [])

  const timeGroupLabels: Record<TimeGroup, string> = {
    today: t('sidebar.timeGroup.today'),
    yesterday: t('sidebar.timeGroup.yesterday'),
    last7days: t('sidebar.timeGroup.last7days'),
    last30days: t('sidebar.timeGroup.last30days'),
    older: t('sidebar.timeGroup.older'),
  }

  const renderSessionRow = (session: SessionListItem) => {
    const displayTitle = resolveSessionTitle(session.title, t('sidebar.untitled'))
    const isRunning = runningSessions.has(session.id)
    const queuedCount = queuedCounts[session.id] ?? 0
    return (
      <div key={session.id} className="group/row relative">
        {renamingId === session.id ? (
          <input
            autoFocus
            value={renameValue}
            onChange={(e) => setRenameValue(e.target.value)}
            onBlur={handleFinishRename}
            onKeyDown={(e) => {
              if (e.key === 'Enter') handleFinishRename()
              if (e.key === 'Escape') {
                setRenamingId(null)
                setRenameValue('')
              }
            }}
            className="ml-1 w-full rounded-[var(--radius-md)] border border-[var(--color-border-focus)] bg-[var(--color-surface)] px-3 py-2 text-xs text-[var(--color-text-primary)] outline-none"
          />
        ) : (
          <>
            <button
              onClick={() => {
                useTabStore.getState().openTab(session.id, displayTitle)
                focusSession(session.id)
              }}
              onContextMenu={(e) => handleContextMenu(e, session.id)}
              title={displayTitle}
              className={`
                w-full rounded-[12px] px-3 py-2 pr-9 text-left text-xs transition-colors duration-200
                ${session.id === activeTabId
                  ? 'bg-[var(--color-sidebar-item-active)] text-[var(--color-text-primary)]'
                  : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-sidebar-item-hover)]'
                }
              `}
            >
              <span className="flex items-center gap-2.5">
                {isRunning ? (
                  <span
                    aria-label={t('common.running')}
                    className="inline-flex items-center flex-shrink-0 text-[var(--color-brand)]"
                  >
                    <Spinner size={8} />
                  </span>
                ) : (
                  <span
                    className="h-1.5 w-1.5 flex-shrink-0 rounded-full"
                    style={{
                      backgroundColor: session.id === activeTabId ? 'var(--color-brand)' : 'var(--color-text-tertiary)',
                      opacity: session.id === activeTabId ? 1 : 0.5,
                    }}
                  />
                )}
                <span className="flex-1 truncate font-medium tracking-[-0.01em]">{displayTitle}</span>
                {!isRunning && queuedCount > 0 && (
                  <span
                    aria-label={t('tabs.queuedBadge', { count: queuedCount })}
                    title={t('tabs.queuedBadge', { count: queuedCount })}
                    className="flex-shrink-0 inline-flex items-center text-[10px] tabular-nums text-[var(--color-text-tertiary)]"
                  >
                    ·{queuedCount}
                  </span>
                )}
                {!session.workDirExists && (
                  <span
                    className="flex-shrink-0 text-xs text-[var(--color-warning)]"
                    title={session.workDir ?? ''}
                  >
                    {t('sidebar.missingDir')}
                  </span>
                )}
                <span className="flex-shrink-0 text-xs text-[var(--color-text-tertiary)] opacity-50 transition-opacity duration-150 group-hover/row:opacity-0">
                  {formatRelativeTime(session.modifiedAt)}
                </span>
              </span>
            </button>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                handleDelete(session.id)
              }}
              aria-label={t('sidebar.deleteAria')}
              title={t('sidebar.deleteAria')}
              data-testid={`sidebar-session-delete-${session.id}`}
              className="absolute right-2 top-1/2 flex h-7 w-7 -translate-y-1/2 items-center justify-center rounded-md text-[var(--color-text-tertiary)] opacity-0 transition-opacity duration-150 hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-error)] focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)] group-hover/row:opacity-100"
            >
              <TrashIcon />
            </button>
          </>
        )}
      </div>
    )
  }

  const visibleWorkspaces =
    workspacesShowMore || workspaceGroups.length <= SIDEBAR_GROUP_PAGE_SIZE
      ? workspaceGroups
      : workspaceGroups.slice(0, SIDEBAR_GROUP_PAGE_SIZE)
  const hiddenWorkspaceCount = workspaceGroups.length - visibleWorkspaces.length

  return (
    <aside
      onMouseDown={handleSidebarDrag}
      className="sidebar-panel relative h-full flex flex-col bg-[var(--color-surface-sidebar)] border-r border-[var(--color-border)] select-none"
      data-state={sidebarOpen ? 'open' : 'closed'}
      aria-label="Sidebar"
    >
      <div className="flex justify-end px-3 pb-2 pt-3">
        <a
          href="https://github.com/senweaver/SenWeaverCoding"
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex items-center justify-center rounded-md p-1 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          title="GitHub"
          data-tauri-drag-region
        >
          <GitHubIcon />
        </a>
      </div>

      <div className={`px-3 pb-3 flex flex-col ${sidebarOpen ? 'gap-0.5' : 'items-center gap-2'}`}>
        <NavItem
          active={false}
          collapsed={!sidebarOpen}
          label={t('sidebar.newSession')}
          onClick={async () => {
            try {
              const currentTabId = useTabStore.getState().activeTabId
              const workDir = useSessionStore.getState().resolveWorkDirForNewSessionTab(currentTabId)
              const sessionId = await useSessionStore.getState().createSession(workDir)
              useTabStore.getState().openTab(sessionId, t('sidebar.newSession'))
              focusSession(sessionId)
            } catch (error) {
              addToast({
                type: 'error',
                message: error instanceof Error ? error.message : t('sidebar.sessionListFailed'),
              })
            }
          }}
          icon={<PlusIcon />}
        >
          {t('sidebar.newSession')}
        </NavItem>
        <NavItem
          active={activeTabId === SCHEDULED_TAB_ID}
          collapsed={!sidebarOpen}
          label={t('sidebar.scheduled')}
          onClick={() => useTabStore.getState().openTab(SCHEDULED_TAB_ID, t('sidebar.scheduled'), 'scheduled')}
          icon={<ClockIcon />}
        >
          {t('sidebar.scheduled')}
        </NavItem>
      </div>

      <AgentMonitorPanel />

      {sidebarOpen ? (
        <>
          <div
            data-testid="sidebar-project-filter-section"
            className="sidebar-section sidebar-section--visible relative z-20 flex-none px-3 pb-2"
            style={{ overflow: 'visible' }}
          >
            <div className="flex h-9 items-center rounded-[14px] border border-[var(--color-sidebar-search-border)] bg-[var(--color-sidebar-search-bg)] pl-1.5 pr-3 transition-colors focus-within:border-[var(--color-border-focus)]">
              <ProjectFilter variant="embedded" />
              <span className="mx-2 h-4 w-px bg-[var(--color-border)]/80" aria-hidden="true" />
              <span className="pointer-events-none flex shrink-0 items-center text-[var(--color-text-tertiary)]">
                <SearchIcon />
              </span>
              <input
                id="sidebar-search"
                type="text"
                placeholder={t('sidebar.searchPlaceholder')}
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="min-w-0 flex-1 bg-transparent pl-2 pr-0 text-[13px] text-[var(--color-text-primary)] placeholder:text-[var(--color-text-tertiary)] outline-none"
              />
            </div>
          </div>

          <div
            data-testid="sidebar-session-list-section"
            className="sidebar-section sidebar-section--visible flex flex-1 min-h-0 flex-col"
          >
            <div className="sidebar-scroll-area min-h-0 flex-1 overflow-y-auto px-3">
              {error && (
                <div className="mx-1 mt-2 rounded-[var(--radius-md)] border border-[var(--color-error)]/20 bg-[var(--color-error)]/5 px-3 py-2">
                  <div className="text-xs font-medium text-[var(--color-error)]">{t('sidebar.sessionListFailed')}</div>
                  <div className="mt-1 text-[11px] text-[var(--color-text-secondary)] break-words">{error}</div>
                  <button
                    onClick={() => fetchSessions()}
                    className="mt-2 text-[11px] font-medium text-[var(--color-brand)] hover:underline"
                  >
                    {t('common.retry')}
                  </button>
                </div>
              )}
              {filteredSessions.length === 0 && (
                <div className="px-3 py-4 text-center text-xs text-[var(--color-text-tertiary)]">
                  {searchQuery ? t('sidebar.noMatching') : t('sidebar.noSessions')}
                </div>
              )}
              {inSearchMode ? (

                <div className="mb-1 pt-2">
                  {filteredSessions.map(renderSessionRow)}
                </div>
              ) : (
                <>
                  {visibleWorkspaces.map((ws) => {
                    const wsOpen = expandedWorkspaces.has(ws.workspaceKey)
                    return (
                      <div key={ws.workspaceKey} className="mb-1 mt-2">
                        <button
                          type="button"
                          onClick={() => toggleWorkspace(ws.workspaceKey)}
                          aria-expanded={wsOpen}
                          aria-label={wsOpen ? t('sidebar.collapseWorkspace') : t('sidebar.expandWorkspace')}
                          title={ws.workspacePath || ws.workspaceLabel}
                          className="flex w-full items-center gap-1.5 rounded-[10px] px-2 py-1 text-left text-xs font-semibold tracking-wide text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-sidebar-item-hover)]"
                        >
                          <ChevronRightIcon open={wsOpen} />
                          <FolderIcon existing={ws.workspaceExists} />
                          <span className="flex-1 truncate">{ws.workspaceLabel}</span>
                          {!ws.workspaceExists && ws.workspaceKey !== WORKSPACE_UNKNOWN_KEY && (
                            <span
                              className="flex-shrink-0 text-xs font-normal text-[var(--color-warning)]"
                              title={ws.workspacePath}
                            >
                              {t('sidebar.missingDir')}
                            </span>
                          )}
                          <span className="flex-shrink-0 text-xs font-normal tabular-nums text-[var(--color-text-tertiary)]">
                            {ws.totalCount}
                          </span>
                        </button>
                        {wsOpen && (
                          <div className="ml-3 mt-0.5">
                            {TIME_GROUP_ORDER.map((tg) => {
                              const items = ws.timeGroups.get(tg)
                              if (!items || items.length === 0) return null
                              const tgKey = `${ws.workspaceKey}::${tg}`
                              const tgOpen = expandedTimeGroups.has(tgKey)
                              const tgShowAll = timeGroupsShowMore.has(tgKey)
                              const visibleItems =
                                tgShowAll || items.length <= SIDEBAR_GROUP_PAGE_SIZE
                                  ? items
                                  : items.slice(0, SIDEBAR_GROUP_PAGE_SIZE)
                              const hiddenCount = items.length - visibleItems.length
                              return (
                                <div key={tg} className="mb-1">
                                  <button
                                    type="button"
                                    onClick={() => toggleTimeGroup(tgKey)}
                                    aria-expanded={tgOpen}
                                    aria-label={tgOpen ? t('sidebar.collapseTimeGroup') : t('sidebar.expandTimeGroup')}
                                    className="flex w-full items-center gap-1 rounded-[8px] px-2 py-0.5 text-left text-xs font-semibold tracking-wide text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-sidebar-item-hover)]"
                                  >
                                    <ChevronRightIcon open={tgOpen} size="sm" />
                                    <span className="flex-1 truncate">{timeGroupLabels[tg]}</span>
                                    <span className="flex-shrink-0 text-xs font-normal tabular-nums opacity-70">
                                      {items.length}
                                    </span>
                                  </button>
                                  {tgOpen && (
                                    <>
                                      {visibleItems.map(renderSessionRow)}
                                      {hiddenCount > 0 && (
                                        <button
                                          type="button"
                                          onClick={() => revealMoreSessions(tgKey)}
                                          className="ml-4 mt-0.5 flex items-center gap-1 rounded-md px-2 py-1 text-[11px] font-medium text-[var(--color-brand)] transition-colors hover:bg-[var(--color-sidebar-item-hover)]"
                                        >
                                          <span className="material-symbols-outlined text-[14px]" aria-hidden="true">expand_more</span>
                                          {t('sidebar.showMore', { count: hiddenCount })}
                                        </button>
                                      )}
                                    </>
                                  )}
                                </div>
                              )
                            })}
                          </div>
                        )}
                      </div>
                    )
                  })}
                  {!workspacesShowMore && hiddenWorkspaceCount > 0 && (
                    <button
                      type="button"
                      onClick={() => setWorkspacesShowMore(true)}
                      className="mt-2 flex w-full items-center justify-center gap-1 rounded-md px-2 py-1.5 text-[11px] font-medium text-[var(--color-brand)] transition-colors hover:bg-[var(--color-sidebar-item-hover)]"
                    >
                      <span className="material-symbols-outlined text-[14px]" aria-hidden="true">expand_more</span>
                      {t('sidebar.showMoreWorkspaces', { count: hiddenWorkspaceCount })}
                    </button>
                  )}
                </>
              )}
            </div>
          </div>
        </>
      ) : (
        <div className="flex-1" aria-hidden="true" />
      )}

      <div className="flex items-center justify-end gap-1 border-t border-[var(--color-border)] p-2">
        {isTauri && (
          <button
            type="button"
            onClick={() => {
              const sessionId = useTabStore.getState().activeTabId
              if (!sessionId) return
              const store = useBrowserPanelStore.getState()
              const panel = store.panels[sessionId]
              if (panel?.visible) {
                void store.closeForSession(sessionId)
              } else {
                void store.toggle(sessionId, { source: 'manual' })
              }
            }}
            title={t('sidebar.browserPanel')}
            aria-label={t('sidebar.browserPanel')}
            className="inline-flex items-center justify-center rounded-md p-1.5 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[16px]">public</span>
          </button>
        )}
        <button
          type="button"
          onClick={() => useUIStore.getState().toggleSettingsOverlay()}
          title={t('sidebar.settings')}
          aria-label={t('sidebar.settings')}
          aria-pressed={settingsOverlayOpen}
          className={
            settingsOverlayOpen
              ? 'inline-flex items-center justify-center rounded-md p-1.5 bg-[var(--color-surface-selected)] text-[var(--color-text-primary)] transition-colors'
              : 'inline-flex items-center justify-center rounded-md p-1.5 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
          }
        >
          <span className="material-symbols-outlined text-[16px]">settings</span>
        </button>
      </div>

      {contextMenu && sidebarOpen && (
        <div
          className="fixed z-50 min-w-[140px] rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] py-1"
          style={{ left: contextMenu.x, top: contextMenu.y, boxShadow: 'var(--shadow-dropdown)' }}
        >
          <button
            onClick={() => {
              const session = sessions.find((s) => s.id === contextMenu.id)
              const seed = isPlaceholderTitle(session?.title) ? '' : (session?.title ?? '')
              handleStartRename(contextMenu.id, seed)
            }}
            className="w-full px-3 py-1.5 text-left text-xs text-[var(--color-text-primary)] transition-colors hover:bg-[var(--color-surface-hover)]"
          >
            {t('common.rename')}
          </button>
          <button
            onClick={() => handleDelete(contextMenu.id)}
            className="w-full px-3 py-1.5 text-left text-xs text-[var(--color-error)] transition-colors hover:bg-[var(--color-surface-hover)]"
          >
            {t('common.delete')}
          </button>
        </div>
      )}

      <ConfirmDialog
        open={pendingDeleteSessionId !== null}
        onClose={() => setPendingDeleteSessionId(null)}
        onConfirm={confirmDelete}
        title={t('common.delete')}
        body={pendingDeleteSessionId ? t('sidebar.confirmDelete') : ''}
        confirmLabel={t('common.delete')}
        cancelLabel={t('common.cancel')}
        confirmVariant="danger"
      />
    </aside>
  )
}

function groupByTime(sessions: SessionListItem[]): Map<TimeGroup, SessionListItem[]> {
  const groups = new Map<TimeGroup, SessionListItem[]>()
  const now = new Date()
  const startOfToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
  const startOfYesterday = startOfToday - 86400000
  const sevenDaysAgo = startOfToday - 7 * 86400000
  const thirtyDaysAgo = startOfToday - 30 * 86400000

  for (const session of sessions) {
    const ts = new Date(session.modifiedAt).getTime()
    let group: TimeGroup
    if (ts >= startOfToday) group = 'today'
    else if (ts >= startOfYesterday) group = 'yesterday'
    else if (ts >= sevenDaysAgo) group = 'last7days'
    else if (ts >= thirtyDaysAgo) group = 'last30days'
    else group = 'older'

    if (!groups.has(group)) groups.set(group, [])
    groups.get(group)!.push(session)
  }

  return groups
}

function workspaceBasename(p: string): string {
  const trimmed = p.trim().replace(/[\\/]+$/, '')
  if (!trimmed) return ''
  const parts = trimmed.split(/[\\/]/)
  return parts[parts.length - 1] || trimmed
}

function groupByWorkspaceAndTime(
  sessions: SessionListItem[],
  unknownLabel: string,
): WorkspaceGroup[] {
  const buckets = new Map<string, SessionListItem[]>()
  for (const s of sessions) {
    const workDir = s.workDir?.trim() ?? ''
    const key = workDir || WORKSPACE_UNKNOWN_KEY
    const list = buckets.get(key)
    if (list) {
      list.push(s)
    } else {
      buckets.set(key, [s])
    }
  }

  const out: WorkspaceGroup[] = []
  for (const [key, items] of buckets) {
    const first = items[0]!
    const isUnknown = key === WORKSPACE_UNKNOWN_KEY
    const workspacePath = isUnknown ? '' : key
    const basename = isUnknown ? '' : workspaceBasename(key)
    const workspaceLabel = isUnknown ? unknownLabel : basename || key

    const workspaceExists = isUnknown ? true : first.workDirExists
    let latest = 0
    for (const s of items) {
      const ts = new Date(s.modifiedAt).getTime()
      if (ts > latest) latest = ts
    }
    out.push({
      workspaceKey: key,
      workspaceLabel,
      workspacePath,
      workspaceExists,
      latestModifiedAt: latest,
      totalCount: items.length,
      timeGroups: groupByTime(items),
    })
  }

  out.sort((a, b) => b.latestModifiedAt - a.latestModifiedAt)
  return out
}

function ChevronRightIcon({ open, size = 'md' }: { open: boolean; size?: 'sm' | 'md' }) {
  const px = size === 'sm' ? 'text-[14px]' : 'text-[15px]'
  return (
    <span
      className={`material-symbols-outlined ${px} flex-shrink-0 transition-transform duration-150`}
      style={{ transform: open ? 'rotate(90deg)' : 'rotate(0deg)' }}
      aria-hidden="true"
    >
      chevron_right
    </span>
  )
}

function FolderIcon({ existing }: { existing: boolean }) {
  return (
    <span
      className="material-symbols-outlined text-[15px] flex-shrink-0"
      style={{
        color: existing ? 'var(--color-text-tertiary)' : 'var(--color-warning)',
        fontVariationSettings: "'FILL' 1",
      }}
      aria-hidden="true"
    >
      folder
    </span>
  )
}

function NavItem({
  active,
  collapsed,
  label,
  onClick,
  icon,
  children,
}: {
  active: boolean
  collapsed: boolean
  label: string
  onClick: () => void
  icon: React.ReactNode
  children: React.ReactNode
}) {
  return (
    <button
      onClick={onClick}
      aria-label={label}
      title={collapsed ? label : undefined}
      className={`
        flex items-center transition-colors duration-200
        ${collapsed ? 'h-10 w-10 justify-center rounded-[var(--radius-md)] px-0 py-0' : 'w-full gap-2.5 rounded-[12px] px-3 py-2.5 text-xs'}
        ${active
          ? 'bg-[var(--color-sidebar-item-active)] font-medium text-[var(--color-text-primary)]'
          : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-sidebar-item-hover)] hover:text-[var(--color-text-primary)]'
        }
      `}
    >
      <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center">
        {icon}
      </span>
      <span className={`sidebar-copy ${collapsed ? 'sidebar-copy--hidden' : 'sidebar-copy--visible'}`}>
        {children}
      </span>
    </button>
  )
}

function formatRelativeTime(dateStr: string): string {
  const diff = Date.now() - new Date(dateStr).getTime()
  const min = Math.floor(diff / 60000)
  if (min < 1) return 'now'
  if (min < 60) return `${min}m`
  const hr = Math.floor(min / 60)
  if (hr < 24) return `${hr}h`
  const day = Math.floor(hr / 24)
  if (day < 30) return `${day}d`
  return `${Math.floor(day / 30)}mo`
}

function PlusIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  )
}

function ClockIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="10" />
      <polyline points="12 6 12 12 16 14" />
    </svg>
  )
}

function SearchIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="11" cy="11" r="7" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  )
}

function TrashIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polyline points="3 6 5 6 21 6" />
      <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
      <path d="M9 6V4a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v2" />
    </svg>
  )
}

function GitHubIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path
        fillRule="evenodd"
        clipRule="evenodd"
        d="M12 2C6.477 2 2 6.477 2 12c0 4.42 2.865 8.17 6.839 9.49.5.092.682-.217.682-.482 0-.237-.008-.866-.013-1.7-2.782.603-3.369-1.34-3.369-1.34-.454-1.156-1.11-1.464-1.11-1.464-.908-.62.069-.608.069-.608 1.003.07 1.531 1.03 1.531 1.03.892 1.529 2.341 1.087 2.91.831.092-.646.35-1.086.636-1.336-2.22-.253-4.555-1.11-4.555-4.943 0-1.091.39-1.984 1.029-2.683-.103-.253-.446-1.27.098-2.647 0 0 .84-.269 2.75 1.025A9.578 9.578 0 0112 6.836c.85.004 1.705.114 2.504.336 1.909-1.294 2.747-1.025 2.747-1.025.546 1.377.203 2.394.1 2.647.64.699 1.028 1.592 1.028 2.683 0 3.842-2.339 4.687-4.566 4.935.359.309.678.919.678 1.852 0 1.336-.012 2.415-.012 2.743 0 .267.18.578.688.48C19.138 20.167 22 16.418 22 12c0-5.523-4.477-10-10-10z"
      />
    </svg>
  )
}

