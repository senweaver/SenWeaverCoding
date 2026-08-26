// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useLanStore } from '../../stores/lanStore'
import { useLanShareStore } from '../../stores/lanShareStore'
import type { LanPeerShare } from '../../types/lanShare'

function isTauriRuntime() {
  return (
    typeof window !== 'undefined' &&
    ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)
  )
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)))
  return `${(bytes / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${units[i]}`
}

export function SharePanel() {
  const t = useTranslation()
  const closePanel = useLanShareStore((s) => s.closePanel)
  const myShares = useLanShareStore((s) => s.myShares)
  const peerShares = useLanShareStore((s) => s.peerShares)
  const downloads = useLanShareStore((s) => s.downloads)
  const addShare = useLanShareStore((s) => s.addShare)
  const removeShare = useLanShareStore((s) => s.removeShare)
  const download = useLanShareStore((s) => s.download)

  const lanRunning = useLanStore((s) => s.identity?.running ?? false)
  const setDiscovery = useLanStore((s) => s.setDiscovery)
  const saveReceivedFile = useLanStore((s) => s.saveReceivedFile)

  const [view, setView] = useState<'mine' | 'network'>('network')
  const [busy, setBusy] = useState(false)
  const [downloading, setDownloading] = useState<Record<string, boolean>>({})
  const [saveState, setSaveState] = useState<Record<string, 'saving' | 'saved' | 'error'>>({})

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closePanel()
    }
    document.addEventListener('keydown', handleKeyDown)
    return () => document.removeEventListener('keydown', handleKeyDown)
  }, [closePanel])

  const grouped = useMemo(() => {
    const map = new Map<string, { nickname: string; items: LanPeerShare[] }>()
    for (const item of peerShares) {
      const entry = map.get(item.ownerId) ?? { nickname: item.ownerNickname, items: [] }
      entry.items.push(item)
      map.set(item.ownerId, entry)
    }
    return Array.from(map.entries()).map(([ownerId, value]) => ({ ownerId, ...value }))
  }, [peerShares])

  async function pickAndShare(directory: boolean) {
    let path: string | null = null
    if (isTauriRuntime()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog')
        const selected = await open({ directory, multiple: false })
        path = Array.isArray(selected) ? selected[0] : selected
      } catch (err) {
        console.error('[SharePanel] dialog failed', err)
        return
      }
    } else {
      path = window.prompt(t('lan.pathPrompt'))
    }
    if (!path) return
    setBusy(true)
    try {
      await addShare(path, '')
    } finally {
      setBusy(false)
    }
  }

  async function handleDownload(item: LanPeerShare) {
    setDownloading((prev) => ({ ...prev, [item.id]: true }))
    try {
      await download(item.ownerId, item.id)
    } finally {
      setTimeout(() => {
        setDownloading((prev) => {
          const next = { ...prev }
          delete next[item.id]
          return next
        })
      }, 1500)
    }
  }

  async function handleSave(key: string, path: string) {
    let destDir: string | null = null
    if (isTauriRuntime()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog')
        const selected = await open({ directory: true, multiple: false })
        destDir = Array.isArray(selected) ? selected[0] : selected
      } catch (err) {
        console.error('[SharePanel] save dialog failed', err)
        return
      }
    } else {
      destDir = window.prompt(t('lan.saveDestPrompt'))
    }
    if (!destDir) return
    setSaveState((prev) => ({ ...prev, [key]: 'saving' }))
    try {
      await saveReceivedFile(path, destDir)
      setSaveState((prev) => ({ ...prev, [key]: 'saved' }))
    } catch (err) {
      console.error('[SharePanel] save failed', err)
      setSaveState((prev) => ({ ...prev, [key]: 'error' }))
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col bg-[var(--color-surface)]">
      <div className="flex flex-shrink-0 items-center gap-2 border-b border-[var(--color-border)] px-4 py-3">
        <span className="material-symbols-outlined text-[20px] text-[var(--color-brand)]">
          share
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[15px] font-semibold text-[var(--color-text-primary)]">
            {t('lanShare.title')}
          </div>
          <div className="truncate text-[11px] text-[var(--color-text-tertiary)]">
            {t('lanShare.subtitle')}
          </div>
        </div>
        <button
          type="button"
          title={t('common.close')}
          onClick={closePanel}
          className="flex h-8 w-8 items-center justify-center rounded-full text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
        >
          <span className="material-symbols-outlined text-[18px]">close</span>
        </button>
      </div>

      <div className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col">
      {!lanRunning && (
        <div className="flex items-center justify-between gap-2 border-b border-[var(--color-border)] bg-[var(--color-surface-hover)] px-3 py-2">
          <span className="text-xs text-[var(--color-text-secondary)]">
            {t('lan.discoveryOff')}
          </span>
          <button
            type="button"
            onClick={() => void setDiscovery(true)}
            className="rounded-md bg-[var(--color-brand)] px-2.5 py-1 text-xs font-semibold text-[var(--color-on-primary)] hover:opacity-90"
          >
            {t('lan.enableDiscovery')}
          </button>
        </div>
      )}

      <div className="flex items-center gap-1 border-b border-[var(--color-border)] px-2 py-1.5">
        <button
          type="button"
          onClick={() => setView('network')}
          className={`flex-1 rounded-md px-2 py-1 text-xs font-medium transition-colors ${
            view === 'network'
              ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
              : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)]'
          }`}
        >
          {t('lanShare.network')}
        </button>
        <button
          type="button"
          onClick={() => setView('mine')}
          className={`flex-1 rounded-md px-2 py-1 text-xs font-medium transition-colors ${
            view === 'mine'
              ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
              : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)]'
          }`}
        >
          {t('lanShare.mine')} ({myShares.length})
        </button>
      </div>

      {view === 'mine' ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <div className="flex items-center gap-1.5 border-b border-[var(--color-border)] px-3 py-2">
            <button
              type="button"
              disabled={busy}
              onClick={() => void pickAndShare(false)}
              className="inline-flex items-center gap-1 rounded-md bg-[var(--color-brand)] px-2.5 py-1 text-xs font-semibold text-[var(--color-on-primary)] hover:opacity-90 disabled:opacity-40"
            >
              <span className="material-symbols-outlined text-[15px]">attach_file</span>
              {t('lanShare.shareFile')}
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => void pickAndShare(true)}
              className="inline-flex items-center gap-1 rounded-md border border-[var(--color-border)] px-2.5 py-1 text-xs font-medium text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-40"
            >
              <span className="material-symbols-outlined text-[15px]">folder</span>
              {t('lanShare.shareFolder')}
            </button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto p-2">
            {myShares.length === 0 ? (
              <div className="px-2 py-8 text-center text-xs text-[var(--color-text-tertiary)]">
                {t('lanShare.emptyMine')}
              </div>
            ) : (
              myShares.map((item) => (
                <div
                  key={item.id}
                  className="mb-1 flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-[var(--color-surface-hover)]"
                >
                  <span className="material-symbols-outlined text-[18px] text-[var(--color-text-tertiary)]">
                    {item.isDir ? 'folder' : 'description'}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-xs font-medium text-[var(--color-text-primary)]">
                      {item.name}
                    </div>
                    <div className="truncate text-[10px] text-[var(--color-text-tertiary)]">
                      {formatBytes(item.size)}
                      {item.note ? ` · ${item.note}` : ''}
                    </div>
                  </div>
                  <button
                    type="button"
                    title={t('lanShare.unshare')}
                    onClick={() => void removeShare(item.id)}
                    className="inline-flex items-center justify-center rounded p-1 text-[var(--color-text-tertiary)] hover:bg-[var(--color-error)]/10 hover:text-[var(--color-error)]"
                  >
                    <span className="material-symbols-outlined text-[16px]">delete</span>
                  </button>
                </div>
              ))
            )}
          </div>
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {grouped.length === 0 ? (
            <div className="px-2 py-8 text-center text-xs text-[var(--color-text-tertiary)]">
              {t('lanShare.emptyNetwork')}
            </div>
          ) : (
            grouped.map((group) => (
              <div key={group.ownerId} className="mb-2">
                <div className="flex items-center gap-1.5 px-1 py-1 text-[10px] font-bold uppercase tracking-widest text-[var(--color-text-tertiary)]">
                  <span className="material-symbols-outlined text-[13px]">person</span>
                  {group.nickname}
                </div>
                {group.items.map((item) => {
                  const isDownloading = downloading[item.id]
                  return (
                    <div
                      key={item.id}
                      className="mb-1 flex items-center gap-2 rounded-md px-2 py-1.5 hover:bg-[var(--color-surface-hover)]"
                    >
                      <span className="material-symbols-outlined text-[18px] text-[var(--color-text-tertiary)]">
                        {item.isDir ? 'folder' : 'description'}
                      </span>
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-xs font-medium text-[var(--color-text-primary)]">
                          {item.name}
                        </div>
                        <div className="truncate text-[10px] text-[var(--color-text-tertiary)]">
                          {formatBytes(item.size)}
                          {item.note ? ` · ${item.note}` : ''}
                        </div>
                      </div>
                      <button
                        type="button"
                        disabled={isDownloading}
                        title={t('lanShare.download')}
                        onClick={() => void handleDownload(item)}
                        className="inline-flex items-center gap-1 rounded-md bg-[var(--color-brand)] px-2 py-1 text-[11px] font-semibold text-[var(--color-on-primary)] hover:opacity-90 disabled:opacity-50"
                      >
                        <span className="material-symbols-outlined text-[14px]">
                          {isDownloading ? 'hourglass_top' : 'download'}
                        </span>
                        {isDownloading ? t('lanShare.requesting') : t('lanShare.download')}
                      </button>
                    </div>
                  )
                })}
              </div>
            ))
          )}

          {downloads.length > 0 && (
            <>
              <div className="px-1 py-1 text-[10px] font-bold uppercase tracking-widest text-[var(--color-text-tertiary)]">
                {t('lanShare.received')}
              </div>
              {downloads.map((d, idx) => {
                const key = `${d.shareId}-${idx}`
                const state = saveState[key]
                return (
                  <div
                    key={key}
                    className="mb-1 flex items-center gap-2 rounded-md border border-[var(--color-border)] px-2 py-1.5"
                  >
                    <span className="material-symbols-outlined text-[16px] text-[var(--color-success,#16a34a)]">
                      check_circle
                    </span>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-xs text-[var(--color-text-primary)]">
                        {d.name}
                      </div>
                      <div className="truncate text-[10px] text-[var(--color-text-tertiary)]">
                        {d.path}
                      </div>
                    </div>
                    <button
                      type="button"
                      disabled={state === 'saving'}
                      onClick={() => void handleSave(key, d.path)}
                      className="inline-flex items-center gap-1 rounded-md px-2 py-0.5 text-[11px] font-medium text-[var(--color-brand)] hover:bg-[var(--color-surface-hover)] disabled:opacity-60"
                    >
                      <span className="material-symbols-outlined text-[14px]">
                        {state === 'saved' ? 'check' : 'save'}
                      </span>
                      {state === 'saving'
                        ? t('lan.saving')
                        : state === 'saved'
                          ? t('lan.saved')
                          : state === 'error'
                            ? t('lan.saveFailed')
                            : t('lan.saveToLocal')}
                    </button>
                  </div>
                )
              })}
            </>
          )}
        </div>
      )}
      </div>
    </div>
  )
}
