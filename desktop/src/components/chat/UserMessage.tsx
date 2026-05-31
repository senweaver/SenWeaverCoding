// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useTranslation } from '../../i18n'
import type { UIAttachment } from '../../types/chat'
import { AttachmentGallery } from './AttachmentGallery'

type Props = {
  content: string
  attachments?: UIAttachment[]

  onRewind?: () => void

  onRestore?: () => void

  rewindLabel?: string

  restoreLabel?: string

  onEditAsDraft?: () => void

  superseded?: boolean
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
}: Props) {
  const t = useTranslation()
  const hasText = content.trim().length > 0
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
                {content}
              </button>
            ) : (
              <div
                className="inline-block max-w-full bg-[var(--color-surface-user-msg)] px-4 py-3 pr-9 text-sm leading-relaxed text-[var(--color-text-primary)] whitespace-pre-wrap break-words"
                style={{ borderRadius: '18px 4px 18px 18px' }}
              >
                {content}
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
      </div>
    </div>
  )
}
