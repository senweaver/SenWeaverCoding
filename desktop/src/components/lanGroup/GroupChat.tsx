// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useLanStore } from '../../stores/lanStore'
import { useLanGroupStore } from '../../stores/lanGroupStore'
import { useUIStore } from '../../stores/uiStore'
import type { LanGroupMessage, LanGroupSnapshot } from '../../types/lanGroup'
import { canContribute, initials } from './shared'
import { lanGroupApi } from '../../api/lanGroup'
import {
  clipboardHasImage,
  extractClipboardImage,
  isImageFileName,
} from '../../lib/clipboardImage'
import { LanImage } from '../lan/LanImage'

const EMPTY_MESSAGES: LanGroupMessage[] = []

export function GroupChat({
  groupId,
  snapshot,
}: {
  groupId: string
  snapshot: LanGroupSnapshot
}) {
  const t = useTranslation()
  const selfId = useLanStore((s) => s.identity?.userId ?? '')
  const selfNickname = useLanStore((s) => s.identity?.nickname ?? '')
  const messages = useLanGroupStore((s) => s.messagesByGroup[groupId] ?? EMPTY_MESSAGES)
  const sendMessage = useLanGroupStore((s) => s.sendMessage)
  const sendImage = useLanGroupStore((s) => s.sendImage)
  const downloadDocument = useLanGroupStore((s) => s.downloadDocument)
  const [draft, setDraft] = useState('')
  const [lightbox, setLightbox] = useState<string | null>(null)
  const scrollRef = useRef<HTMLDivElement>(null)

  const editable = canContribute(snapshot.group.role)

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [messages.length])

  async function handleSend() {
    const body = draft.trim()
    if (!body) return
    setDraft('')
    await sendMessage(groupId, body)
  }

  async function handlePaste(event: React.ClipboardEvent<HTMLInputElement>) {
    if (!editable) return
    if (!clipboardHasImage(event.clipboardData)) return
    event.preventDefault()
    try {
      const image = await extractClipboardImage(event.clipboardData)
      if (!image) return
      await sendImage(groupId, image.fileName, image.dataBase64)
    } catch (err) {
      useUIStore.getState().addToast({
        type: 'error',
        message: err instanceof Error ? err.message : String(err),
      })
    }
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div ref={scrollRef} className="flex-1 space-y-2 overflow-y-auto px-3 py-3">
        {messages.length === 0 ? (
          <div className="py-6 text-center text-xs text-[var(--color-text-tertiary)]">
            {t('lanGroup.noMessages')}
          </div>
        ) : (
          messages.map((msg) => {
            const isOut = msg.author === selfId
            const doc = msg.docId
              ? snapshot.documents.find((d) => d.id === msg.docId)
              : undefined
            const docName = doc?.name ?? msg.body
            const isImage =
              msg.kind === 'file' && !!doc && !doc.isDir && isImageFileName(docName)
            const authorName =
              msg.authorNickname || (isOut ? selfNickname : msg.author)
            return (
              <div key={msg.id} className={`flex ${isOut ? 'justify-end' : 'justify-start'}`}>
                <div className="flex max-w-[82%] flex-col gap-0.5">
                  {authorName && (
                    <span
                      className={`px-1 text-[10px] text-[var(--color-text-tertiary)] ${
                        isOut ? 'text-right' : ''
                      }`}
                    >
                      {authorName}
                    </span>
                  )}
                  <div
                    className={`rounded-2xl px-3 py-1.5 text-sm ${
                      isOut
                        ? 'bg-[var(--color-brand)] text-white'
                        : 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
                    }`}
                  >
                    {isImage && doc ? (
                      <GroupChatImage
                        groupId={groupId}
                        docId={doc.id}
                        name={docName}
                        available={doc.available}
                        version={doc.version}
                        onEnsure={() => void downloadDocument(groupId, doc.id)}
                        onOpen={setLightbox}
                        fileLabel={t('lanGroup.sharedDocument')}
                        pendingLabel={t('lanGroup.imageLoading')}
                        failedLabel={t('lanGroup.imageFailed')}
                      />
                    ) : msg.kind === 'file' ? (
                      <span className="flex items-center gap-1.5">
                        <span className="material-symbols-outlined text-[16px]">
                          {doc?.isDir ? 'folder' : 'description'}
                        </span>
                        <span className="break-all">
                          {t('lanGroup.sharedDocument')}: {doc?.name ?? msg.body}
                        </span>
                      </span>
                    ) : (
                      <span className="whitespace-pre-wrap break-words">{msg.body}</span>
                    )}
                  </div>
                </div>
              </div>
            )
          })
        )}
      </div>

      <div className="border-t border-[var(--color-border)] p-2">
        <div className="flex items-center gap-1.5">
          <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--color-surface-selected)] text-[10px] font-semibold text-[var(--color-text-primary)]">
            {initials(useLanStore.getState().identity?.nickname ?? '?')}
          </span>
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
            placeholder={t('lanGroup.messagePlaceholder')}
            disabled={!editable}
            className="h-9 flex-1 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] disabled:opacity-50"
          />
          <button
            type="button"
            onClick={() => void handleSend()}
            disabled={!editable || !draft.trim()}
            className="inline-flex h-9 items-center justify-center rounded-[var(--radius-md)] bg-[var(--color-brand)] px-3 text-sm font-semibold text-white transition-colors hover:opacity-90 disabled:opacity-40"
          >
            {t('lanGroup.send')}
          </button>
        </div>
      </div>

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
    </div>
  )
}

function GroupChatImage({
  groupId,
  docId,
  name,
  available,
  version,
  onEnsure,
  onOpen,
  fileLabel,
  pendingLabel,
  failedLabel,
}: {
  groupId: string
  docId: string
  name: string
  available: boolean
  version: number
  onEnsure: () => void
  onOpen: (src: string) => void
  fileLabel: string
  pendingLabel: string
  failedLabel: string
}) {
  const [failed, setFailed] = useState(false)

  useEffect(() => {
    if (!available) onEnsure()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [available, docId])

  useEffect(() => {
    setFailed(false)
  }, [docId, version])

  if (failed) {
    return (
      <span className="flex items-center gap-1.5">
        <span className="material-symbols-outlined text-[16px]">image</span>
        <span className="break-all">
          {fileLabel}: {name}
        </span>
      </span>
    )
  }

  if (!available) {
    return (
      <span className="flex h-24 w-40 items-center justify-center rounded-lg bg-[var(--color-surface-hover)] text-[11px] text-[var(--color-text-tertiary)]">
        <span className="material-symbols-outlined animate-spin text-[16px]">
          progress_activity
        </span>
        <span className="ml-1">{pendingLabel}</span>
      </span>
    )
  }

  const src = lanGroupApi.rawDocumentUrl(groupId, docId)
  return (
    <LanImage
      src={src}
      alt={name}
      reloadKey={`${docId}-${version}`}
      onOpen={() => onOpen(src)}
      onError={() => setFailed(true)}
      pendingLabel={pendingLabel}
      failedLabel={failedLabel}
    />
  )
}
