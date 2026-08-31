// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { ImageGalleryModal } from './ImageGalleryModal'
import { workspaceFilesApi } from '../../api/workspaceFiles'

export type AttachmentPreview = {
  id?: string
  type: 'image' | 'file'
  name: string
  data?: string
  previewUrl?: string
  path?: string
}

type Props = {
  attachments: AttachmentPreview[]
  variant?: 'composer' | 'message'
  onRemove?: (id: string) => void
}

const pathImageSrcCache = new Map<string, Promise<string | null>>()
const rawIdByRootCache = new Map<string, Promise<string | null>>()
const MAX_ROOT_PROBE_LEVELS = 8

function rawIdForRoot(root: string): Promise<string | null> {
  let promise = rawIdByRootCache.get(root)
  if (!promise) {
    promise = workspaceFilesApi
      .rawHandle({ root })
      .then((r) => r.rawId)
      .catch(() => null)
    rawIdByRootCache.set(root, promise)
  }
  return promise
}

function resolvePathImageSrc(path: string): Promise<string | null> {
  let promise = pathImageSrcCache.get(path)
  if (!promise) {
    promise = (async () => {
      const parts = path.replace(/\\/g, '/').split('/').filter(Boolean)
      if (parts.length < 2) return null
      const maxCut = parts.length - 1
      const minCut = Math.max(1, maxCut - MAX_ROOT_PROBE_LEVELS)
      for (let cut = maxCut; cut >= minCut; cut--) {
        const root = parts.slice(0, cut).join('/')
        const rel = parts.slice(cut).join('/')
        if (!root || !rel) continue
        const rawId = await rawIdForRoot(root)
        if (rawId) return workspaceFilesApi.rawUrl(rawId, rel)
      }
      return null
    })().catch(() => null)
    pathImageSrcCache.set(path, promise)
  }
  return promise
}

export function AttachmentGallery({ attachments, variant = 'message', onRemove }: Props) {
  const [activeImageIndex, setActiveImageIndex] = useState<number | null>(null)
  const [resolvedByPath, setResolvedByPath] = useState<Record<string, string>>({})

  useEffect(() => {
    let cancelled = false
    for (const attachment of attachments) {
      if (attachment.type !== 'image' || attachment.previewUrl || attachment.data) continue
      const path = attachment.path
      if (!path || resolvedByPath[path] !== undefined) continue
      void resolvePathImageSrc(path).then((src) => {
        if (cancelled) return
        setResolvedByPath((cur) =>
          cur[path] !== undefined ? cur : { ...cur, [path]: src ?? '' },
        )
      })
    }
    return () => {
      cancelled = true
    }
  }, [attachments, resolvedByPath])

  const srcFor = (attachment: AttachmentPreview): string =>
    attachment.previewUrl ||
    attachment.data ||
    (attachment.path ? resolvedByPath[attachment.path] || '' : '')

  const images = useMemo(
    () =>
      attachments
        .filter(
          (attachment) =>
            attachment.type === 'image' &&
            (attachment.previewUrl ||
              attachment.data ||
              (attachment.path && resolvedByPath[attachment.path])),
        )
        .map((attachment) => ({
          src:
            attachment.previewUrl ||
            attachment.data ||
            (attachment.path ? resolvedByPath[attachment.path] || '' : ''),
          name: attachment.name,
        })),
    [attachments, resolvedByPath],
  )

  if (attachments.length === 0) return null

  const isComposer = variant === 'composer'

  return (
    <>
      <div className={isComposer ? 'flex flex-wrap items-center gap-2' : 'grid grid-cols-1 gap-2 sm:grid-cols-2'}>
        {attachments.map((attachment, index) => {
          if (
            attachment.type === 'image' &&
            !isComposer &&
            !srcFor(attachment) &&
            attachment.path &&
            resolvedByPath[attachment.path] === undefined
          ) {
            return (
              <div
                key={attachment.id || `${attachment.name}-${index}`}
                className="w-full max-w-[360px] animate-pulse overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)]"
                style={{ height: 240 }}
              />
            )
          }
          if (
            attachment.type === 'image' &&
            !isComposer &&
            !srcFor(attachment) &&
            attachment.path
          ) {
            return (
              <div
                key={attachment.id || `${attachment.name}-${index}`}
                className="flex w-full max-w-[360px] items-center justify-center overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)]"
                style={{ height: 240 }}
              >
                <span className="max-w-[80%] truncate text-[11px] text-[var(--color-text-tertiary)]">
                  {attachment.name}
                </span>
              </div>
            )
          }
          if (attachment.type === 'image' && srcFor(attachment)) {
            const src = srcFor(attachment)
            return (
              <div
                key={attachment.id || `${attachment.name}-${index}`}
                className={isComposer ? 'group relative' : ''}
              >
                <button
                  type="button"
                  onClick={() => setActiveImageIndex(images.findIndex((image) => image.src === src))}
                  className={
                    isComposer
                      ? 'overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)]'
                      : 'w-full max-w-[360px] overflow-hidden rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)] text-left shadow-sm transition-transform hover:scale-[1.01]'
                  }
                  style={isComposer ? undefined : { height: 240 }}
                >
                  <img
                    src={src}
                    alt={attachment.name}
                    className={
                      isComposer
                        ? 'h-16 w-16 object-cover'
                        : 'h-full w-full object-cover'
                    }
                  />
                </button>
                {onRemove && attachment.id && (
                  <button
                    type="button"
                    onClick={() => onRemove(attachment.id!)}
                    className="absolute -right-1 -top-1 flex h-5 w-5 items-center justify-center rounded-full bg-[var(--color-error)] text-[10px] text-[var(--color-on-error)] opacity-0 transition-opacity group-hover:opacity-100"
                    aria-label={`Remove ${attachment.name}`}
                  >
                    ×
                  </button>
                )}
              </div>
            )
          }

          return (
            <div
              key={attachment.id || `${attachment.name}-${index}`}
              className="flex items-center gap-2 rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 py-2 text-xs text-[var(--color-text-secondary)]"
            >
              <span className="material-symbols-outlined text-[14px]">attach_file</span>
              <span className="max-w-[220px] truncate">{attachment.name}</span>
              {onRemove && attachment.id && (
                <button
                  type="button"
                  onClick={() => onRemove(attachment.id!)}
                  className="ml-1 text-[var(--color-text-tertiary)] transition-colors hover:text-[var(--color-error)]"
                  aria-label={`Remove ${attachment.name}`}
                >
                  <span className="material-symbols-outlined text-[14px]">close</span>
                </button>
              )}
            </div>
          )
        })}
      </div>

      {activeImageIndex !== null && activeImageIndex >= 0 && (
        <ImageGalleryModal
          open={activeImageIndex !== null}
          images={images}
          activeIndex={activeImageIndex}
          onClose={() => setActiveImageIndex(null)}
          onSelect={setActiveImageIndex}
        />
      )}
    </>
  )
}
