// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from '../../i18n'
import { useLanStore } from '../../stores/lanStore'
import { useLanGroupStore, type GroupTab } from '../../stores/lanGroupStore'
import { GroupBoard } from './GroupBoard'
import { GroupChat } from './GroupChat'
import { GroupDocuments } from './GroupDocuments'
import { GroupMembers } from './GroupMembers'
import { GroupTimeline } from './GroupTimeline'
import { phaseLabel } from './shared'
import type { TranslationKey } from '../../i18n'

const TABS: { id: GroupTab; label: TranslationKey; icon: string }[] = [
  { id: 'chat', label: 'lanGroup.tabChat', icon: 'forum' },
  { id: 'documents', label: 'lanGroup.tabDocuments', icon: 'folder' },
  { id: 'board', label: 'lanGroup.tabBoard', icon: 'view_kanban' },
  { id: 'timeline', label: 'lanGroup.tabTimeline', icon: 'timeline' },
  { id: 'members', label: 'lanGroup.tabMembers', icon: 'group' },
]

export function GroupsPanel() {
  const t = useTranslation()
  const panelOpen = useLanGroupStore((s) => s.panelOpen)
  const closePanel = useLanGroupStore((s) => s.closePanel)
  const groups = useLanGroupStore((s) => s.groups)
  const activeGroupId = useLanGroupStore((s) => s.activeGroupId)
  const activeTab = useLanGroupStore((s) => s.activeTab)
  const setActiveTab = useLanGroupStore((s) => s.setActiveTab)
  const snapshots = useLanGroupStore((s) => s.snapshots)
  const selectGroup = useLanGroupStore((s) => s.selectGroup)
  const createGroup = useLanGroupStore((s) => s.createGroup)
  const pendingUploadPath = useLanGroupStore((s) => s.pendingUploadPath)
  const lanRunning = useLanStore((s) => s.identity?.running ?? false)
  const setDiscovery = useLanStore((s) => s.setDiscovery)

  const panelRef = useRef<HTMLDivElement>(null)
  const [creating, setCreating] = useState(false)
  const [newName, setNewName] = useState('')
  const [newDesc, setNewDesc] = useState('')

  useEffect(() => {
    if (!panelOpen) return
    const handlePointerDown = (event: PointerEvent) => {
        const target = event.target as HTMLElement | null
        if (!target) return
        if (panelRef.current?.contains(target)) return
        if (target.closest('[data-lan-group-toggle]')) return
        if (target.closest('[data-app-titlebar]')) return
        closePanel()
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closePanel()
    }
    document.addEventListener('pointerdown', handlePointerDown, true)
    document.addEventListener('keydown', handleKeyDown)
    return () => {
      document.removeEventListener('pointerdown', handlePointerDown, true)
      document.removeEventListener('keydown', handleKeyDown)
    }
  }, [panelOpen, closePanel])

  if (!panelOpen) return null

  const snapshot = activeGroupId ? snapshots[activeGroupId] : undefined

  async function handleCreate() {
    const name = newName.trim()
    if (!name) return
    const group = await createGroup(name, newDesc)
    setCreating(false)
    setNewName('')
    setNewDesc('')
    if (group) await selectGroup(group.id)
  }

  return createPortal(
      <div
        ref={panelRef}
        onMouseDown={(e) => e.stopPropagation()}
        className="fixed left-3 z-50 flex w-[760px] max-w-[calc(100vw-24px)] flex-col overflow-hidden rounded-[var(--radius-xl)] border border-[var(--color-border)] bg-[var(--color-surface)] shadow-[var(--shadow-dropdown)]"
        style={{
          top: 'calc(var(--titlebar-height) + 52px)',
          height: '80vh',
          maxHeight: 'calc(100vh - var(--titlebar-height) - 64px)',
        }}
      >
      <div className="flex items-center gap-2 border-b border-[var(--color-border)] p-3">
        <span className="material-symbols-outlined text-[20px] text-[var(--color-brand)]">
          groups
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-semibold text-[var(--color-text-primary)]">
            {t('lanGroup.title')}
          </div>
          <div className="truncate text-[11px] text-[var(--color-text-tertiary)]">
            {t('lanGroup.subtitle')}
          </div>
        </div>
        <button
          type="button"
          title={t('common.close')}
          onClick={closePanel}
          className="inline-flex items-center justify-center rounded-md p-1.5 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
        >
          <span className="material-symbols-outlined text-[18px]">close</span>
        </button>
      </div>

      {!lanRunning && (
        <div className="flex items-center justify-between gap-2 border-b border-[var(--color-border)] bg-[var(--color-surface-hover)] px-3 py-2">
          <span className="text-xs text-[var(--color-text-secondary)]">
            {t('lan.discoveryOff')}
          </span>
          <button
            type="button"
            onClick={() => void setDiscovery(true)}
            className="rounded-md bg-[var(--color-brand)] px-2.5 py-1 text-xs font-semibold text-white hover:opacity-90"
          >
            {t('lan.enableDiscovery')}
          </button>
        </div>
      )}

      <div className="flex min-h-0 flex-1">
        <div className="flex w-52 shrink-0 flex-col border-r border-[var(--color-border)]">
          <div className="flex items-center justify-between px-3 py-2">
            <span className="text-[10px] font-bold uppercase tracking-widest text-[var(--color-text-tertiary)]">
              {t('lanGroup.title')}
            </span>
            <button
              type="button"
              title={t('lanGroup.create')}
              onClick={() => setCreating(true)}
              className="inline-flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
            >
              <span className="material-symbols-outlined text-[16px]">add</span>
            </button>
          </div>
          <div className="flex-1 overflow-y-auto px-2 pb-2">
            {groups.length === 0 ? (
              <div className="px-1 py-4 text-center text-[11px] text-[var(--color-text-tertiary)]">
                {t('lanGroup.empty')}
              </div>
            ) : (
              groups.map((group) => (
                <button
                  type="button"
                  key={group.id}
                  onClick={() => void selectGroup(group.id)}
                  className={`mb-1 flex w-full flex-col gap-1 rounded-md px-2 py-1.5 text-left transition-colors ${
                    activeGroupId === group.id
                      ? 'bg-[var(--color-surface-selected)]'
                      : 'hover:bg-[var(--color-surface-hover)]'
                  }`}
                >
                  <div className="flex items-center gap-1">
                    <span className="min-w-0 flex-1 truncate text-xs font-medium text-[var(--color-text-primary)]">
                      {group.name}
                    </span>
                    {group.unread > 0 && (
                      <span className="inline-flex h-4 min-w-4 items-center justify-center rounded-full bg-[var(--color-error)] px-1 text-[9px] font-semibold text-white">
                        {group.unread > 99 ? '99+' : group.unread}
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-1 text-[9px] text-[var(--color-text-tertiary)]">
                    <span>
                      {group.memberCount} {t('lanGroup.members')}
                    </span>
                    <span className="ml-auto">{Math.round(group.progress)}%</span>
                  </div>
                  <div className="h-1 w-full overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
                    <div
                      className="h-full rounded-full bg-[var(--color-brand)]"
                      style={{ width: `${Math.min(100, group.progress)}%` }}
                    />
                  </div>
                </button>
              ))
            )}
          </div>
        </div>

        <div className="flex min-w-0 flex-1 flex-col">
          {!activeGroupId || !snapshot ? (
            <div className="flex flex-1 items-center justify-center px-4 text-center text-xs text-[var(--color-text-tertiary)]">
              {activeGroupId ? t('common.loading') : t('lanGroup.empty')}
            </div>
          ) : (
            <>
              <div className="border-b border-[var(--color-border)] px-3 py-2">
                <div className="flex items-center gap-2">
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-semibold text-[var(--color-text-primary)]">
                      {snapshot.group.name}
                    </span>
                    {snapshot.group.description && (
                      <span className="block truncate text-[11px] text-[var(--color-text-tertiary)]">
                        {snapshot.group.description}
                      </span>
                    )}
                  </span>
                  <span className="shrink-0 text-[11px] font-medium text-[var(--color-brand)]">
                    {t('lanGroup.progress')} {Math.round(snapshot.group.progress)}%
                  </span>
                </div>
                <div className="mt-2 flex items-center gap-1">
                  {TABS.map((tab) => (
                    <button
                      type="button"
                      key={tab.id}
                      onClick={() => setActiveTab(tab.id)}
                      className={`inline-flex items-center gap-1 rounded-md px-2 py-1 text-xs transition-colors ${
                        activeTab === tab.id
                          ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
                          : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
                      }`}
                    >
                      <span className="material-symbols-outlined text-[15px]">{tab.icon}</span>
                      {t(tab.label)}
                    </button>
                  ))}
                </div>
              </div>

              <div className="relative min-h-0 flex-1">
                {activeTab === 'chat' && (
                  <GroupChat groupId={activeGroupId} snapshot={snapshot} />
                )}
                {activeTab === 'documents' && (
                  <GroupDocuments groupId={activeGroupId} snapshot={snapshot} />
                )}
                {activeTab === 'board' && (
                  <GroupBoard groupId={activeGroupId} snapshot={snapshot} />
                )}
                {activeTab === 'timeline' && <GroupTimeline snapshot={snapshot} />}
                {activeTab === 'members' && (
                  <GroupMembers groupId={activeGroupId} snapshot={snapshot} />
                )}
              </div>
            </>
          )}
        </div>
      </div>

      {creating && (
        <div className="absolute inset-0 z-10 flex items-center justify-center bg-black/30 p-4">
          <div className="flex w-full max-w-sm flex-col gap-2.5 rounded-[var(--radius-xl)] border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-[var(--shadow-dropdown)]">
            <span className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t('lanGroup.createTitle')}
            </span>
            <input
              autoFocus
              value={newName}
              onChange={(e) => setNewName(e.target.value)}
              placeholder={t('lanGroup.namePlaceholder')}
              className="h-9 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
            />
            <textarea
              value={newDesc}
              onChange={(e) => setNewDesc(e.target.value)}
              placeholder={t('lanGroup.descPlaceholder')}
              rows={2}
              className="resize-none rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
            />
            <div className="flex items-center justify-end gap-2">
              <button
                type="button"
                onClick={() => setCreating(false)}
                className="rounded-md px-2.5 py-1 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
              >
                {t('lanGroup.cancel')}
              </button>
              <button
                type="button"
                onClick={() => void handleCreate()}
                disabled={!newName.trim()}
                className="rounded-md bg-[var(--color-brand)] px-3 py-1 text-xs font-semibold text-white hover:opacity-90 disabled:opacity-40"
              >
                {t('lanGroup.confirm')}
              </button>
            </div>
          </div>
        </div>
      )}

      {pendingUploadPath && <PendingUploadDialog />}
    </div>,
    document.body,
  )
}

function PendingUploadDialog() {
  const t = useTranslation()
  const pendingUploadPath = useLanGroupStore((s) => s.pendingUploadPath)
  const clearPendingUpload = useLanGroupStore((s) => s.clearPendingUpload)
  const groups = useLanGroupStore((s) => s.groups)
  const snapshots = useLanGroupStore((s) => s.snapshots)
  const refreshSnapshot = useLanGroupStore((s) => s.refreshSnapshot)
  const uploadDocument = useLanGroupStore((s) => s.uploadDocument)

  const [groupId, setGroupId] = useState(groups[0]?.id ?? '')
  const [phaseId, setPhaseId] = useState('')
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    if (groupId) void refreshSnapshot(groupId)
  }, [groupId, refreshSnapshot])

  const phases = groupId ? snapshots[groupId]?.phases ?? [] : []

  async function confirm() {
    if (!groupId || !pendingUploadPath) return
    setBusy(true)
    try {
      await uploadDocument(groupId, pendingUploadPath, phaseId, '')
      clearPendingUpload()
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="absolute inset-0 z-20 flex items-center justify-center bg-black/30 p-4">
      <div className="flex w-full max-w-sm flex-col gap-2.5 rounded-[var(--radius-xl)] border border-[var(--color-border)] bg-[var(--color-surface)] p-4 shadow-[var(--shadow-dropdown)]">
        <span className="text-sm font-semibold text-[var(--color-text-primary)]">
          {t('lanGroup.pendingUploadTitle')}
        </span>
        <span className="truncate text-[11px] text-[var(--color-text-tertiary)]">
          {pendingUploadPath}
        </span>
        <label className="flex flex-col gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
          {t('lanGroup.title')}
          <select
            value={groupId}
            onChange={(e) => setGroupId(e.target.value)}
            className="h-8 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)]"
          >
            {groups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-0.5 text-[10px] text-[var(--color-text-tertiary)]">
          {t('lanGroup.selectPhase')}
          <select
            value={phaseId}
            onChange={(e) => setPhaseId(e.target.value)}
            className="h-8 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-xs text-[var(--color-text-primary)]"
          >
            <option value="">{t('lanGroup.uncategorized')}</option>
            {phases.map((p) => (
              <option key={p.id} value={p.id}>
                {phaseLabel(p, t)}
              </option>
            ))}
          </select>
        </label>
        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={clearPendingUpload}
            className="rounded-md px-2.5 py-1 text-xs text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            {t('lanGroup.cancel')}
          </button>
          <button
            type="button"
            onClick={() => void confirm()}
            disabled={!groupId || busy}
            className="rounded-md bg-[var(--color-brand)] px-3 py-1 text-xs font-semibold text-white hover:opacity-90 disabled:opacity-40"
          >
            {t('lanGroup.confirm')}
          </button>
        </div>
      </div>
    </div>
  )
}
