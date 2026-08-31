// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useMemo } from 'react'
import { useTranslation } from '../../i18n'
import { isCredentialGroup } from '../../api/credentials'
import { useCredentialsStore } from '../../stores/credentialsStore'
import { wsManager } from '../../api/websocket'
import type { UIAttachment } from '../../types/chat'
import { AttachmentGallery } from './AttachmentGallery'
import {
  credChipLabel,
  hasRefTokens,
  parseRefSegments,
  refIconName,
  refKind,
} from './composerRefs'

type Props = {
  content: string
  attachments?: UIAttachment[]

  onRewind?: () => void

  onRestore?: () => void

  rewindLabel?: string

  restoreLabel?: string

  onEditAsDraft?: () => void

  superseded?: boolean

  pending?: boolean

  clientMsgId?: string

  sessionId?: string | null

  designRef?: string
  designRefName?: string
  designRefElement?: string
  designRefElementLabel?: string
}

function CredChip({ name, field }: { name: string; field?: string }) {
  const meta = useCredentialsStore((s) => s.credentials.find((c) => c.name === name))
  const group = !field && meta != null && isCredentialGroup(meta)
  const fieldCount = meta?.fields?.length ?? 0
  const title = group
    ? fieldCount > 0
      ? `${name} (${fieldCount})`
      : name
    : credChipLabel(name, field)
  return (
    <span
      data-ref-kind="cred"
      className="mx-0.5 inline-flex select-none items-center gap-1 rounded-md bg-[var(--color-ref-chip-cred-bg)] px-1.5 align-middle text-[var(--color-text-secondary)]"
      title={title}
    >
      <span className="material-symbols-outlined text-[13px] leading-none">
        {group ? 'vpn_key' : 'key'}
      </span>
      <span className="text-[12px] font-medium text-[var(--color-text-primary)]">
        {credChipLabel(name, field)}
      </span>
    </span>
  )
}

function renderMessageContent(content: string) {
  if (!hasRefTokens(content)) return content
  return parseRefSegments(content).map((segment, index) => {
    if (segment.type === 'text') {
      return <span key={index}>{segment.text}</span>
    }
    if (segment.type === 'cred') {
      return <CredChip key={index} name={segment.name} field={segment.field} />
    }
    const bgClass =
      refKind(segment.relPath) === 'session'
        ? 'bg-[var(--color-ref-chip-session-bg)]'
        : 'bg-[var(--color-surface-container-high)]'
    return (
      <span
        key={index}
        data-ref-kind={refKind(segment.relPath)}
        className={`mx-0.5 inline-flex select-none items-center gap-1 rounded-md ${bgClass} px-1.5 align-middle text-[var(--color-text-secondary)]`}
        title={segment.relPath}
      >
        <span className="material-symbols-outlined text-[13px] leading-none">
          {refIconName(segment.relPath)}
        </span>
        <span className="text-[12px] font-medium text-[var(--color-text-primary)]">
          {segment.name || segment.relPath}
        </span>
      </span>
    )
  })
}

function designRefIcon(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  if (['png', 'jpg', 'jpeg', 'webp', 'gif', 'avif', 'bmp'].includes(ext)) return 'image'
  if (['mp4', 'webm', 'mov', 'm4v'].includes(ext)) return 'movie'
  if (['mp3', 'wav', 'ogg', 'm4a', 'aac', 'flac'].includes(ext)) return 'graphic_eq'
  return 'draw'
}

export function UserMessage({
  content,
  attachments,
  onRewind,
  onRestore,
  rewindLabel,
  restoreLabel,
  onEditAsDraft,
  superseded,
  pending,
  clientMsgId,
  sessionId,
  designRef,
  designRefName,
  designRefElement,
  designRefElementLabel,
}: Props) {
  const t = useTranslation()
  const hasText = content.trim().length > 0
  const renderedContent = useMemo(() => renderMessageContent(content), [content])
  const canRetrySend = pending === true && !!clientMsgId && !!sessionId
  const refLabel = designRef
    ? (designRefName?.trim() || designRef.split('/').pop() || designRef)
    : ''
  const elementLabel = designRefElement
    ? (designRefElementLabel?.trim() || designRefElement)
    : ''
  const editTooltip = t('chat.editPromptHint')
  const showRestore = !!onRestore
  const showRewind = !showRestore && !!onRewind

  return (
    <div
      className={`group mb-5 flex justify-end ${superseded ? 'opacity-60 saturate-50' : ''}`}
    >
      <div
        data-message-shell="user"

        className="flex min-w-0 max-w-[82%] flex-col items-end gap-2 sm:max-w-[78%] lg:max-w-[72%]"
      >
        {attachments && attachments.length > 0 && (
          <AttachmentGallery attachments={attachments} variant="message" />
        )}

        {designRef && (
          <div
            className="inline-flex max-w-full items-center gap-1.5 rounded-lg border border-[var(--color-accent)]/40 bg-[var(--color-accent)]/10 py-1 pl-1.5 pr-2 text-[11px] text-[var(--color-text-secondary)]"
            title={designRef}
          >
            <span className="flex h-5 w-5 flex-shrink-0 items-center justify-center rounded bg-[var(--color-accent)]/15">
              <span className="material-symbols-outlined text-[13px] text-[var(--color-accent)]">
                {designRefIcon(designRef)}
              </span>
            </span>
            <span className="flex min-w-0 flex-col leading-tight">
              <span className="max-w-[220px] truncate text-[9px] uppercase tracking-wide text-[var(--color-accent)]">
                {elementLabel
                  ? `${t('designer.canvas.elementRef')}: ${elementLabel}`
                  : t('designer.canvas.editRefChip')}
              </span>
              <span className="max-w-[220px] truncate text-[11px] font-medium text-[var(--color-text-primary)]">
                {refLabel}
              </span>
            </span>
          </div>
        )}

        {hasText && (

          <div className="relative inline-flex max-w-full">
            {onEditAsDraft ? (
              <button
                type="button"
                onClick={onEditAsDraft}
                title={editTooltip}
                aria-label={editTooltip}
                className="inline-block max-w-full bg-[var(--color-surface-user-msg)] px-4 py-3 pr-9 text-left text-sm leading-relaxed text-[var(--color-text-primary)] whitespace-pre-wrap break-words transition-shadow hover:ring-1 hover:ring-[var(--color-brand)]/35 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-brand)]/35"
                style={{ borderRadius: '18px 4px 18px 18px' }}
              >
                {renderedContent}
              </button>
            ) : (
              <div
                className="inline-block max-w-full bg-[var(--color-surface-user-msg)] px-4 py-3 pr-9 text-sm leading-relaxed text-[var(--color-text-primary)] whitespace-pre-wrap break-words"
                style={{ borderRadius: '18px 4px 18px 18px' }}
              >
                {renderedContent}
              </div>
            )}

            {(showRestore || showRewind) && (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation()
                  if (showRestore) {
                    onRestore?.()
                  } else {
                    onRewind?.()
                  }
                }}
                aria-label={showRestore ? restoreLabel : rewindLabel}
                title={showRestore ? restoreLabel : rewindLabel}
                className="absolute bottom-1.5 right-1.5 inline-flex h-5 w-5 items-center justify-center rounded-full text-[var(--color-text-tertiary)] opacity-0 transition-opacity duration-200 hover:bg-[var(--color-surface-container-low)] hover:text-[var(--color-text-primary)] group-hover:opacity-100 group-focus-within:opacity-100 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-brand)]/35"
              >
                <span
                  className="material-symbols-outlined text-[12px]"
                  style={{ fontVariationSettings: "'wght' 300, 'opsz' 20, 'GRAD' 0, 'FILL' 0" }}
                >
                  {showRestore ? 'restore' : 'history'}
                </span>
              </button>
            )}
          </div>
        )}

        {pending === true && (
          <button
            type="button"
            onClick={
              canRetrySend
                ? () => wsManager.retryUserMessage(sessionId!, clientMsgId!)
                : undefined
            }
            title={canRetrySend ? t('chat.retrySendTitle') : undefined}
            className={`inline-flex items-center gap-1.5 px-1 text-[10px] text-[var(--color-text-tertiary)] ${
              canRetrySend ? 'hover:text-[var(--color-text-primary)]' : 'cursor-default'
            }`}
          >
            <span className="size-1.5 rounded-full bg-[var(--color-warning)] animate-pulse" />
            <span>{t('chat.sendingPending')}</span>
          </button>
        )}
      </div>
    </div>
  )
}
