// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useLanStore } from '../../stores/lanStore'
import { useUIStore } from '../../stores/uiStore'
import { useTranslation } from '../../i18n'
import type { LanMessage, LanPeer } from '../../types/lan'
import { lanApi } from '../../api/lan'
import {
  clipboardHasImage,
  extractClipboardImage,
  isImageFileName,
} from '../../lib/clipboardImage'
import { LanImage } from './LanImage'

function isTauriRuntime() {
  return (
    typeof window !== 'undefined' &&
    ('__TAURI_INTERNALS__' in window || '__TAURI__' in window)
  )
}

function initials(name: string): string {
  const trimmed = name.trim()
  if (!trimmed) return '?'
  return trimmed.slice(0, 1).toUpperCase()
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.min(units.length - 1, Math.floor(Math.log(bytes) / Math.log(1024)))
  return `${(bytes / 1024 ** i).toFixed(i === 0 ? 0 : 1)} ${units[i]}`
}

export function UserPanel() {
  const t = useTranslation()
  const panelOpen = useLanStore((s) => s.panelOpen)
  const closePanel = useLanStore((s) => s.closePanel)
  const identity = useLanStore((s) => s.identity)
  const peers = useLanStore((s) => s.peers)
  const conversations = useLanStore((s) => s.conversations)
  const messagesByPeer = useLanStore((s) => s.messagesByPeer)
  const transfers = useLanStore((s) => s.transfers)
  const activePeerId = useLanStore((s) => s.activePeerId)
  const selectPeer = useLanStore((s) => s.selectPeer)
  const sendMessage = useLanStore((s) => s.sendMessage)
  const sendFile = useLanStore((s) => s.sendFile)
  const sendImage = useLanStore((s) => s.sendImage)
  const saveReceivedFile = useLanStore((s) => s.saveReceivedFile)
  const setDiscovery = useLanStore((s) => s.setDiscovery)
  const init = useLanStore((s) => s.init)

  const [view, setView] = useState<'list' | 'chat'>('list')
  const [draft, setDraft] = useState('')
  const [lightbox, setLightbox] = useState<string | null>(null)
  const [saveState, setSaveState] = useState<Record<string, 'saving' | 'saved' | 'error'>>({})
  const scrollRef = useRef<HTMLDivElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (panelOpen) void init()
  }, [panelOpen, init])

  useEffect(() => {
    if (!panelOpen) return
    const handlePointerDown = (event: PointerEvent) => {
      const target = event.target as HTMLElement | null
      if (!target) return
      if (panelRef.current?.contains(target)) return
      if (target.closest('[data-lan-panel-toggle]')) return
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

  const activeMessages = activePeerId ? messagesByPeer[activePeerId] ?? [] : []

  useEffect(() => {
    if (view === 'chat' && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [view, activeMessages.length])

  const activePeer: LanPeer | undefined = useMemo(
    () => peers.find((p) => p.userId === activePeerId),
    [peers, activePeerId],
  )

  const activeNickname =
    activePeer?.nickname ??
    conversations.find((c) => c.peerId === activePeerId)?.nickname ??
    activePeerId ??
    ''

  const offlineChats = useMemo(() => {
    const onlineIds = new Set(peers.map((p) => p.userId))
    return conversations.filter((c) => !onlineIds.has(c.peerId))
  }, [peers, conversations])

  const activeTransfers = transfers.filter((tr) => tr.status === 'active')
  const chatActiveTransfers = activePeerId
    ? transfers.filter((tr) => tr.peerId === activePeerId && tr.status === 'active')
    : []

  async function openChat(peerId: string) {
    await selectPeer(peerId)
    setView('chat')
  }

  async function handleSaveFile(message: LanMessage) {
    const filePath = message.filePath
    if (!filePath) return
    let destDir: string | null = null
    if (isTauriRuntime()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog')
        const selected = await open({ directory: true, multiple: false })
        destDir = Array.isArray(selected) ? selected[0] : selected
      } catch (err) {
        console.error('[UserPanel] save dialog failed', err)
        return
      }
    } else {
      destDir = window.prompt(t('lan.saveDestPrompt'))
    }
    if (!destDir) return
    setSaveState((prev) => ({ ...prev, [message.id]: 'saving' }))
    try {
      await saveReceivedFile(filePath, destDir)
      setSaveState((prev) => ({ ...prev, [message.id]: 'saved' }))
    } catch (err) {
      console.error('[UserPanel] save file failed', err)
      setSaveState((prev) => ({ ...prev, [message.id]: 'error' }))
    }
  }

  async function handleSend() {
    if (!activePeerId) return
    const body = draft.trim()
    if (!body) return
    setDraft('')
    await sendMessage(activePeerId, body)
  }

  async function handlePaste(event: React.ClipboardEvent<HTMLInputElement>) {
    if (!activePeer || !activePeerId) return
    if (!clipboardHasImage(event.clipboardData)) return
    event.preventDefault()
    try {
      const image = await extractClipboardImage(event.clipboardData)
      if (!image) return
      await sendImage(activePeerId, image.fileName, image.dataBase64)
    } catch (err) {
      useUIStore.getState().addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    }
  }

  async function pickAndSend(directory: boolean) {
    if (!activePeerId) return
    if (isTauriRuntime()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog')
        const selected = await open({ directory, multiple: false })
        const path = Array.isArray(selected) ? selected[0] : selected
        if (path && typeof path === 'string') {
          await sendFile(activePeerId, path)
        }
      } catch (err) {
        console.error('[UserPanel] file dialog failed', err)
      }
    } else {
      const path = window.prompt(t('lan.pathPrompt'))
      if (path) await sendFile(activePeerId, path)
    }
  }

  if (!panelOpen) return null

  const unreadByPeer = (peerId: string) =>
    conversations.find((c) => c.peerId === peerId)?.unread ?? 0

  return createPortal(
    <div
      ref={panelRef}
      onMouseDown={(e) => e.stopPropagation()}
      className="fixed bottom-14 left-3 z-50 flex w-[400px] max-w-[calc(100vw-24px)] flex-col rounded-[var(--radius-xl)] border border-[var(--color-border)] bg-[var(--color-surface)] shadow-[var(--shadow-dropdown)]"
      style={{ height: '78vh', maxHeight: '640px' }}
    >
        <div className="flex items-center gap-3 border-b border-[var(--color-border)] p-3">
          <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-[var(--color-brand)] text-sm font-semibold text-white">
            {initials(identity?.nickname ?? identity?.userId ?? '?')}
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-1.5">
              <span className="truncate text-sm font-semibold text-[var(--color-text-primary)]">
                {identity?.nickname ?? identity?.userId ?? '—'}
              </span>
              <span
                className={`inline-block h-2 w-2 rounded-full ${
                  identity?.running ? 'bg-[var(--color-success,#16a34a)]' : 'bg-[var(--color-text-tertiary)]'
                }`}
              />
            </div>
            <div className="truncate text-[11px] text-[var(--color-text-tertiary)]">
              {identity?.userId} · {identity?.localIp ?? '—'}
            </div>
          </div>
          <button
            type="button"
            title={t('lan.settings')}
            onClick={() => useUIStore.getState().openSettingsOverlay('general')}
            className="inline-flex items-center justify-center rounded-md p-1.5 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[18px]">settings</span>
          </button>
          <button
            type="button"
            title={t('common.close')}
            onClick={closePanel}
            className="inline-flex items-center justify-center rounded-md p-1.5 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[18px]">close</span>
          </button>
        </div>

        {!identity?.running && (
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

        {view === 'list' ? (
          <div className="flex-1 overflow-y-auto">
            <div className="px-3 py-2 text-[10px] font-bold uppercase tracking-widest text-[var(--color-text-tertiary)]">
              {t('lan.online')}
            </div>
            {peers.length === 0 ? (
              <div className="px-3 py-4 text-center text-xs text-[var(--color-text-tertiary)]">
                {t('lan.noPeers')}
              </div>
            ) : (
              peers.map((peer) => (
                <button
                  key={peer.userId}
                  onClick={() => void openChat(peer.userId)}
                  className="flex w-full items-center gap-3 px-3 py-2.5 text-left transition-colors hover:bg-[var(--color-surface-hover)]"
                >
                  <div className="relative flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-[var(--color-surface-selected)] text-xs font-semibold text-[var(--color-text-primary)]">
                    {initials(peer.nickname)}
                    <span className="absolute -bottom-0.5 -right-0.5 h-2.5 w-2.5 rounded-full border-2 border-[var(--color-surface)] bg-[var(--color-success,#16a34a)]" />
                  </div>
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">
                      {peer.nickname}
                    </div>
                    <div className="truncate text-[11px] text-[var(--color-text-tertiary)]">
                      {peer.ip}
                    </div>
                  </div>
                  {unreadByPeer(peer.userId) > 0 && (
                    <span className="ml-auto inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-[var(--color-brand)] px-1.5 text-[10px] font-semibold text-white">
                      {unreadByPeer(peer.userId)}
                    </span>
                  )}
                </button>
              ))
            )}

            {offlineChats.length > 0 && (
              <>
                <div className="px-3 py-2 text-[10px] font-bold uppercase tracking-widest text-[var(--color-text-tertiary)]">
                  {t('lan.recentChats')}
                </div>
                {offlineChats.map((chat) => (
                  <button
                    key={chat.peerId}
                    onClick={() => void openChat(chat.peerId)}
                    className="flex w-full items-center gap-3 px-3 py-2.5 text-left opacity-70 transition-colors hover:bg-[var(--color-surface-hover)]"
                  >
                    <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full bg-[var(--color-surface-selected)] text-xs font-semibold text-[var(--color-text-primary)]">
                      {initials(chat.nickname)}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">
                        {chat.nickname}
                      </div>
                      <div className="truncate text-[11px] text-[var(--color-text-tertiary)]">
                        {chat.lastMessage}
                      </div>
                    </div>
                    {chat.unread > 0 && (
                      <span className="ml-auto inline-flex h-5 min-w-5 items-center justify-center rounded-full bg-[var(--color-brand)] px-1.5 text-[10px] font-semibold text-white">
                        {chat.unread}
                      </span>
                    )}
                  </button>
                ))}
              </>
            )}

            {activeTransfers.length > 0 && (
              <>
                <div className="px-3 py-2 text-[10px] font-bold uppercase tracking-widest text-[var(--color-text-tertiary)]">
                  {t('lan.transfers')}
                </div>
                <div className="px-3 pb-3 space-y-2">
                  {activeTransfers.map((tr) => {
                    const pct =
                      tr.size > 0 ? Math.min(100, Math.round((tr.transferred / tr.size) * 100)) : 0
                    return (
                      <div key={tr.id} className="rounded-lg border border-[var(--color-border)] p-2">
                        <div className="flex items-center justify-between gap-2">
                          <span className="flex items-center gap-1 truncate text-xs text-[var(--color-text-primary)]">
                            <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
                              {tr.direction === 'out' ? 'upload' : 'download'}
                            </span>
                            {tr.name}
                          </span>
                          <span className="text-[10px] text-[var(--color-text-tertiary)]">{pct}%</span>
                        </div>
                        <div className="mt-1.5 h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
                          <div
                            className="h-full rounded-full bg-[var(--color-brand)] transition-all"
                            style={{ width: `${pct}%` }}
                          />
                        </div>
                        <div className="mt-1 text-[10px] text-[var(--color-text-tertiary)]">
                          {formatBytes(tr.transferred)} / {formatBytes(tr.size)}
                        </div>
                      </div>
                    )
                  })}
                </div>
              </>
            )}
          </div>
        ) : (
          <div className="flex flex-1 flex-col overflow-hidden">
            <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-3 py-2">
              <button
                type="button"
                onClick={() => setView('list')}
                className="inline-flex items-center justify-center rounded-md p-1 text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
              >
                <span className="material-symbols-outlined text-[18px]">arrow_back</span>
              </button>
              <span className="truncate text-sm font-semibold text-[var(--color-text-primary)]">
                {activeNickname}
              </span>
              {!activePeer && (
                <span className="ml-1 text-[10px] text-[var(--color-text-tertiary)]">
                  {t('lan.offline')}
                </span>
              )}
            </div>

            <div ref={scrollRef} className="flex-1 space-y-2 overflow-y-auto px-3 py-3">
              {activeMessages.length === 0 ? (
                <div className="py-6 text-center text-xs text-[var(--color-text-tertiary)]">
                  {t('lan.noMessages')}
                </div>
              ) : (
                activeMessages.map((msg) => (
                  <MessageBubble
                    key={msg.id}
                    message={msg}
                    authorName={
                      msg.direction === 'out'
                        ? identity?.nickname ?? identity?.userId ?? ''
                        : activeNickname
                    }
                    fileLabel={t('lan.fileLabel')}
                    saveLabel={t('lan.saveToLocal')}
                    saveState={saveState[msg.id]}
                    savedLabel={t('lan.saved')}
                    savingLabel={t('lan.saving')}
                    saveFailedLabel={t('lan.saveFailed')}
                    imagePendingLabel={t('lan.imageLoading')}
                    imageFailedLabel={t('lan.imageFailed')}
                    onSave={() => void handleSaveFile(msg)}
                    onOpenImage={setLightbox}
                  />
                ))
              )}
            </div>

            {chatActiveTransfers.length > 0 && (
              <div className="border-t border-[var(--color-border)] px-3 py-2 space-y-2">
                {chatActiveTransfers.map((tr) => {
                  const pct =
                    tr.size > 0 ? Math.min(100, Math.round((tr.transferred / tr.size) * 100)) : 0
                  return (
                    <div key={tr.id}>
                      <div className="flex items-center justify-between gap-2">
                        <span className="flex min-w-0 items-center gap-1 text-xs text-[var(--color-text-secondary)]">
                          <span className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]">
                            {tr.direction === 'out' ? 'upload' : 'download'}
                          </span>
                          <span className="truncate">{tr.name}</span>
                        </span>
                        <span className="shrink-0 text-[10px] text-[var(--color-text-tertiary)]">
                          {formatBytes(tr.transferred)} / {formatBytes(tr.size)} · {pct}%
                        </span>
                      </div>
                      <div className="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-[var(--color-surface-hover)]">
                        <div
                          className="h-full rounded-full bg-[var(--color-brand)] transition-all"
                          style={{ width: `${pct}%` }}
                        />
                      </div>
                    </div>
                  )
                })}
              </div>
            )}

            <div className="border-t border-[var(--color-border)] p-2">
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  title={t('lan.sendFile')}
                  disabled={!activePeer}
                  onClick={() => void pickAndSend(false)}
                  className="inline-flex items-center justify-center rounded-md p-1.5 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-40"
                >
                  <span className="material-symbols-outlined text-[18px]">attach_file</span>
                </button>
                <button
                  type="button"
                  title={t('lan.sendFolder')}
                  disabled={!activePeer}
                  onClick={() => void pickAndSend(true)}
                  className="inline-flex items-center justify-center rounded-md p-1.5 text-[var(--color-text-tertiary)] transition-colors hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-40"
                >
                  <span className="material-symbols-outlined text-[18px]">folder</span>
                </button>
                <input
                  value={draft}
                  onChange={(e) => setDraft(e.target.value)}
                  onPaste={(e) => void handlePaste(e)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && !e.shiftKey) {
                      e.preventDefault()
                      void handleSend()
                    }
                  }}
                  placeholder={activePeer ? t('lan.messagePlaceholder') : t('lan.offlineSendDisabled')}
                  disabled={!activePeer}
                  className="h-9 flex-1 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] disabled:opacity-50"
                />
                <button
                  type="button"
                  onClick={() => void handleSend()}
                  disabled={!activePeer || !draft.trim()}
                  className="inline-flex h-9 items-center justify-center rounded-[var(--radius-md)] bg-[var(--color-brand)] px-3 text-sm font-semibold text-white transition-colors hover:opacity-90 disabled:opacity-40"
                >
                  {t('lan.send')}
                </button>
              </div>
            </div>
          </div>
        )}
        {lightbox && (
          <div
            className="fixed inset-0 z-[60] flex items-center justify-center bg-black/70 p-6"
            onClick={() => setLightbox(null)}
          >
            <img
              src={lightbox}
              alt=""
              className="max-h-full max-w-full rounded-lg object-contain"
              onClick={(e) => e.stopPropagation()}
            />
          </div>
        )}
    </div>,
    document.body,
  )
}

function MessageBubble({
  message,
  authorName,
  fileLabel,
  saveLabel,
  saveState,
  savedLabel,
  savingLabel,
  saveFailedLabel,
  imagePendingLabel,
  imageFailedLabel,
  onSave,
  onOpenImage,
}: {
  message: LanMessage
  authorName: string
  fileLabel: string
  saveLabel: string
  saveState?: 'saving' | 'saved' | 'error'
  savedLabel: string
  savingLabel: string
  saveFailedLabel: string
  imagePendingLabel: string
  imageFailedLabel: string
  onSave: () => void
  onOpenImage: (src: string) => void
}) {
  const [imageFailed, setImageFailed] = useState(false)
  const isOut = message.direction === 'out'
  const fileName = message.fileName ?? message.body
  const isImage =
    message.kind === 'file' && !!message.filePath && isImageFileName(fileName)
  const imageUrl =
    isImage && !imageFailed && message.filePath
      ? lanApi.rawFileUrl(message.filePath)
      : null
  const canSave = !isOut && message.kind === 'file' && !!message.filePath
  return (
    <div className={`flex flex-col ${isOut ? 'items-end' : 'items-start'}`}>
      {authorName && (
        <span className="px-1 pb-0.5 text-[10px] text-[var(--color-text-tertiary)]">
          {authorName}
        </span>
      )}
      <div
        className={`max-w-[78%] rounded-2xl px-3 py-1.5 text-sm ${
          isOut
            ? 'bg-[var(--color-brand)] text-white'
            : 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
        }`}
      >
        {imageUrl ? (
          <LanImage
            src={imageUrl}
            alt={fileName}
            onOpen={() => onOpenImage(imageUrl)}
            onError={() => setImageFailed(true)}
            pendingLabel={imagePendingLabel}
            failedLabel={imageFailedLabel}
          />
        ) : message.kind === 'file' ? (
          <div className="flex flex-col gap-1">
            <span className="flex items-center gap-1.5">
              <span className="material-symbols-outlined text-[16px]">description</span>
              <span className="break-all">
                {fileLabel}: {message.fileName ?? message.body}
              </span>
            </span>
            {canSave && (
              <button
                type="button"
                onClick={onSave}
                disabled={saveState === 'saving'}
                className="inline-flex items-center gap-1 self-start rounded-md bg-[var(--color-surface)] px-2 py-0.5 text-[11px] font-medium text-[var(--color-brand)] transition-colors hover:bg-[var(--color-surface-hover)] disabled:opacity-60"
              >
                <span className="material-symbols-outlined text-[14px]">
                  {saveState === 'saved' ? 'check' : 'save'}
                </span>
                {saveState === 'saving'
                  ? savingLabel
                  : saveState === 'saved'
                    ? savedLabel
                    : saveState === 'error'
                      ? saveFailedLabel
                      : saveLabel}
              </button>
            )}
          </div>
        ) : (
          <span className="whitespace-pre-wrap break-words">{message.body}</span>
        )}
      </div>
    </div>
  )
}
