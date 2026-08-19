// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding

import { useEffect, useMemo, useRef, useState } from 'react'
import { Icon } from '@iconify/react/dist/offline'
import { useTranslation } from '../../i18n'
import { workspaceFilesApi } from '../../api/workspaceFiles'
import { ensureVscodeIcons, getFileIconId, isVscodeIconsReady } from '../../lib/fileIcons'

export type MediaKind =
  | 'image'
  | 'svg'
  | 'video'
  | 'audio'
  | 'pdf'
  | 'unknown'

type Props = {

  content: string

  encoding: 'utf8' | 'base64'

  mimeType?: string

  fileName: string

  relPath: string

  sizeBytes: number

  workspaceRoot?: string

  modifiedAt?: string
}

const IMAGE_EXTS = new Set([
  'png', 'jpg', 'jpeg', 'gif', 'webp', 'bmp', 'tiff', 'tif', 'ico', 'avif',
])
const VIDEO_EXTS = new Set([
  'mp4', 'webm', 'mov', 'mkv', 'ogv', 'm4v', 'avi',
])
const AUDIO_EXTS = new Set([
  'mp3', 'wav', 'ogg', 'oga', 'flac', 'aac', 'm4a', 'opus',
])

function extOf(name: string): string {
  const dot = name.lastIndexOf('.')
  if (dot <= 0) return ''
  return name.slice(dot + 1).toLowerCase()
}

export function classifyMedia(name: string, mimeType?: string): MediaKind {
  const ext = extOf(name)
  if (ext === 'svg') return 'svg'
  if (IMAGE_EXTS.has(ext)) return 'image'
  if (VIDEO_EXTS.has(ext)) return 'video'
  if (AUDIO_EXTS.has(ext)) return 'audio'
  if (ext === 'pdf') return 'pdf'
  if (mimeType) {
    if (mimeType.startsWith('image/svg')) return 'svg'
    if (mimeType.startsWith('image/')) return 'image'
    if (mimeType.startsWith('video/')) return 'video'
    if (mimeType.startsWith('audio/')) return 'audio'
    if (mimeType === 'application/pdf') return 'pdf'
  }
  return 'unknown'
}

function inferImageMime(name: string, mimeType?: string): string {
  if (mimeType && mimeType.startsWith('image/')) return mimeType
  const ext = extOf(name)
  switch (ext) {
    case 'png':
      return 'image/png'
    case 'jpg':
    case 'jpeg':
      return 'image/jpeg'
    case 'gif':
      return 'image/gif'
    case 'webp':
      return 'image/webp'
    case 'bmp':
      return 'image/bmp'
    case 'tiff':
    case 'tif':
      return 'image/tiff'
    case 'ico':
      return 'image/x-icon'
    case 'avif':
      return 'image/avif'
    case 'svg':
      return 'image/svg+xml'
    default:
      return 'application/octet-stream'
  }
}

function inferVideoMime(name: string, mimeType?: string): string {
  if (mimeType && mimeType.startsWith('video/')) return mimeType
  const ext = extOf(name)
  switch (ext) {
    case 'mp4':
    case 'm4v':
      return 'video/mp4'
    case 'webm':
      return 'video/webm'
    case 'mov':
      return 'video/quicktime'
    case 'mkv':
      return 'video/x-matroska'
    case 'ogv':
      return 'video/ogg'
    case 'avi':
      return 'video/x-msvideo'
    default:
      return 'video/mp4'
  }
}

function inferAudioMime(name: string, mimeType?: string): string {
  if (mimeType && mimeType.startsWith('audio/')) return mimeType
  const ext = extOf(name)
  switch (ext) {
    case 'mp3':
      return 'audio/mpeg'
    case 'wav':
      return 'audio/wav'
    case 'ogg':
    case 'oga':
      return 'audio/ogg'
    case 'flac':
      return 'audio/flac'
    case 'aac':
      return 'audio/aac'
    case 'm4a':
      return 'audio/mp4'
    case 'opus':
      return 'audio/opus'
    default:
      return 'audio/mpeg'
  }
}

function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0 B'
  const units = ['B', 'KB', 'MB', 'GB']
  let i = 0
  let v = n
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024
    i += 1
  }
  return `${v.toFixed(v < 10 && i > 0 ? 1 : 0)} ${units[i]}`
}

function buildDataUrl(
  kind: MediaKind,
  content: string,
  encoding: 'utf8' | 'base64',
  fileName: string,
  mimeType: string | undefined,
): string {
  if (kind === 'svg' && encoding === 'utf8') {
    return `data:image/svg+xml;utf8,${encodeURIComponent(content)}`
  }
  const mime =
    kind === 'image' || kind === 'svg'
      ? inferImageMime(fileName, mimeType)
      : kind === 'video'
        ? inferVideoMime(fileName, mimeType)
        : kind === 'audio'
          ? inferAudioMime(fileName, mimeType)
          : kind === 'pdf'
            ? 'application/pdf'
            : (mimeType ?? 'application/octet-stream')
  if (encoding === 'base64') {
    return `data:${mime};base64,${content}`
  }

  try {
    return `data:${mime};base64,${btoa(unescape(encodeURIComponent(content)))}`
  } catch {
    return `data:${mime};utf8,${encodeURIComponent(content)}`
  }
}

const ZOOM_STEPS = [0.25, 0.33, 0.5, 0.67, 0.75, 1, 1.25, 1.5, 2, 3, 4, 6, 8]

function clampZoom(target: number): number {
  return Math.max(ZOOM_STEPS[0]!, Math.min(ZOOM_STEPS[ZOOM_STEPS.length - 1]!, target))
}

function nextZoom(current: number, dir: 1 | -1): number {
  if (dir === 1) {
    for (const step of ZOOM_STEPS) {
      if (step > current + 1e-3) return step
    }
    return ZOOM_STEPS[ZOOM_STEPS.length - 1]!
  }
  for (let i = ZOOM_STEPS.length - 1; i >= 0; i -= 1) {
    const step = ZOOM_STEPS[i]!
    if (step < current - 1e-3) return step
  }
  return ZOOM_STEPS[0]!
}

const RAW_STREAM_IMAGE_THRESHOLD = 512 * 1024

export function MediaPreview({
  content,
  encoding,
  mimeType,
  fileName,
  relPath,
  sizeBytes,
  workspaceRoot,
  modifiedAt,
}: Props) {
  const t = useTranslation()
  const kind = classifyMedia(fileName, mimeType)
  const dataUrl = useMemo(
    () => buildDataUrl(kind, content, encoding, fileName, mimeType),
    [kind, content, encoding, fileName, mimeType],
  )

  const wantsRawStream =
    !!workspaceRoot &&
    (kind === 'video' ||
      kind === 'audio' ||
      kind === 'pdf' ||
      ((kind === 'image' || kind === 'svg') &&
        (sizeBytes > RAW_STREAM_IMAGE_THRESHOLD || content.length === 0)))

  const [rawSrc, setRawSrc] = useState<string | null>(null)
  const [rawFailed, setRawFailed] = useState(false)
  useEffect(() => {
    setRawSrc(null)
    setRawFailed(false)
    if (!wantsRawStream || !workspaceRoot) return
    let cancelled = false
    workspaceFilesApi
      .rawHandle({ root: workspaceRoot })
      .then(({ rawId }) => {
        if (cancelled) return
        const version = modifiedAt ? Date.parse(modifiedAt) || undefined : undefined
        setRawSrc(workspaceFilesApi.rawUrl(rawId, relPath, version))
      })
      .catch(() => {
        if (!cancelled) setRawFailed(true)
      })
    return () => {
      cancelled = true
    }
  }, [wantsRawStream, workspaceRoot, relPath, modifiedAt])

  const mediaSrc = wantsRawStream
    ? rawSrc ?? (rawFailed && content.length > 0 ? dataUrl : null)
    : content.length > 0
      ? dataUrl
      : null
  const mediaFailed = mediaSrc === null && (rawFailed || !wantsRawStream)

  const [iconsReady, setIconsReady] = useState(() => isVscodeIconsReady())
  useEffect(() => {
    if (iconsReady) return
    let cancelled = false
    void ensureVscodeIcons().then(() => {
      if (!cancelled) setIconsReady(true)
    })
    return () => {
      cancelled = true
    }
  }, [iconsReady])

  return (
    <div className="flex h-full min-h-0 flex-col">
      <Header
        fileName={fileName}
        relPath={relPath}
        sizeBytes={sizeBytes}
        kind={kind}
        iconsReady={iconsReady}
      />
      <div className="relative flex min-h-0 flex-1 items-center justify-center overflow-auto bg-[var(--color-surface-elevated)]">
        {mediaSrc === null && kind !== 'unknown' ? (
          <div className="px-4 text-center text-xs text-[var(--color-text-tertiary)]">
            {mediaFailed ? t('files.preview.loadFailed') : t('rightSidebar.loading')}
          </div>
        ) : kind === 'image' || kind === 'svg' ? (
          <ImageViewer src={mediaSrc ?? dataUrl} alt={fileName} />
        ) : kind === 'video' ? (
          <VideoViewer
            src={mediaSrc ?? dataUrl}
            mime={inferVideoMime(fileName, mimeType)}
          />
        ) : kind === 'audio' ? (
          <AudioViewer
            src={mediaSrc ?? dataUrl}
            mime={inferAudioMime(fileName, mimeType)}
          />
        ) : kind === 'pdf' ? (
          <PdfViewer src={mediaSrc ?? dataUrl} title={fileName} />
        ) : (
          <div className="px-4 text-center text-xs text-[var(--color-text-tertiary)]">
            {t('files.binaryNotPreviewable')}
          </div>
        )}
      </div>
    </div>
  )
}

function Header({
  fileName,
  relPath,
  sizeBytes,
  kind,
  iconsReady,
}: {
  fileName: string
  relPath: string
  sizeBytes: number
  kind: MediaKind
  iconsReady: boolean
}) {
  const t = useTranslation()
  const kindLabel =
    kind === 'image' || kind === 'svg'
      ? t('files.preview.image')
      : kind === 'video'
        ? t('files.preview.video')
        : kind === 'audio'
          ? t('files.preview.audio')
          : kind === 'pdf'
            ? t('files.preview.pdf')
            : t('files.preview.binary')
  return (
    <div
      className="flex h-7 flex-shrink-0 items-center gap-2 border-b border-[var(--color-border)] bg-[var(--color-surface-container)] px-2 text-xs"
      title={relPath}
    >
      {iconsReady ? (
        <Icon
          aria-hidden="true"
          icon={getFileIconId(fileName, false, false)}
          width={14}
          height={14}
          className="flex-shrink-0"
        />
      ) : (
        <span
          aria-hidden="true"
          className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)]"
        >
          {kind === 'image' || kind === 'svg'
            ? 'image'
            : kind === 'video'
              ? 'movie'
              : kind === 'audio'
                ? 'audiotrack'
                : kind === 'pdf'
                  ? 'picture_as_pdf'
                  : 'description'}
        </span>
      )}
      <span className="truncate font-medium text-[var(--color-text-primary)]">
        {fileName}
      </span>
      <span className="rounded-sm bg-[var(--color-surface-hover)] px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-[var(--color-text-tertiary)]">
        {kindLabel}
      </span>
      <span className="ml-auto text-[10px] text-[var(--color-text-tertiary)]">
        {formatBytes(sizeBytes)}
      </span>
    </div>
  )
}

type Fit = 'contain' | 'actual'

function ImageViewer({ src, alt }: { src: string; alt: string }) {
  const t = useTranslation()
  const [fit, setFit] = useState<Fit>('contain')
  const [zoom, setZoom] = useState(1)
  const containerRef = useRef<HTMLDivElement | null>(null)
  const imgRef = useRef<HTMLImageElement | null>(null)
  const [naturalSize, setNaturalSize] = useState<{ w: number; h: number } | null>(null)

  useEffect(() => {
    const el = containerRef.current
    if (!el) return
    const onWheel = (e: WheelEvent) => {
      if (!(e.ctrlKey || e.metaKey)) return
      e.preventDefault()
      setFit('actual')
      setZoom((z) => clampZoom(z * (e.deltaY > 0 ? 0.9 : 1.1)))
    }
    el.addEventListener('wheel', onWheel, { passive: false })
    return () => el.removeEventListener('wheel', onWheel)
  }, [])

  return (
    <div ref={containerRef} className="relative flex h-full w-full items-center justify-center overflow-auto">
      {}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0"
        style={{
          backgroundImage:
            'linear-gradient(45deg, var(--color-border) 25%, transparent 25%),' +
            'linear-gradient(-45deg, var(--color-border) 25%, transparent 25%),' +
            'linear-gradient(45deg, transparent 75%, var(--color-border) 75%),' +
            'linear-gradient(-45deg, transparent 75%, var(--color-border) 75%)',
          backgroundSize: '16px 16px',
          backgroundPosition: '0 0, 0 8px, 8px -8px, -8px 0px',
          opacity: 0.18,
        }}
      />
      <img
        ref={imgRef}
        src={src}
        alt={alt}
        draggable={false}
        onLoad={(e) => {
          const img = e.currentTarget
          setNaturalSize({ w: img.naturalWidth, h: img.naturalHeight })
        }}
        className="relative select-none"
        style={
          fit === 'contain'
            ? { maxWidth: '100%', maxHeight: '100%', objectFit: 'contain' }
            : {
                width: naturalSize ? `${naturalSize.w * zoom}px` : 'auto',
                height: naturalSize ? `${naturalSize.h * zoom}px` : 'auto',
                imageRendering: zoom >= 2 ? 'pixelated' : 'auto',
              }
        }
      />
      <Toolbar
        items={[
          {
            icon: 'remove',
            title: t('files.preview.zoomOut'),
            onClick: () => {
              setFit('actual')
              setZoom((z) => nextZoom(z, -1))
            },
          },
          {
            label: fit === 'contain' ? t('files.preview.fit') : `${Math.round(zoom * 100)}%`,
            title: t('files.preview.actualSize'),
            onClick: () => {
              setFit('actual')
              setZoom(1)
            },
          },
          {
            icon: 'add',
            title: t('files.preview.zoomIn'),
            onClick: () => {
              setFit('actual')
              setZoom((z) => nextZoom(z, 1))
            },
          },
          {
            icon: 'fit_screen',
            title: t('files.preview.fit'),
            onClick: () => setFit('contain'),
            active: fit === 'contain',
          },
        ]}
      />
      {naturalSize && (
        <div className="absolute bottom-2 left-2 rounded bg-[var(--color-surface)]/85 px-2 py-0.5 text-[10px] text-[var(--color-text-tertiary)] backdrop-blur-sm">
          {naturalSize.w} × {naturalSize.h}
        </div>
      )}
    </div>
  )
}

function VideoViewer({ src, mime }: { src: string; mime: string }) {
  return (
    <video
      controls
      preload="metadata"
      className="max-h-full max-w-full bg-black"
    >
      <source src={src} type={mime} />
    </video>
  )
}

function AudioViewer({ src, mime }: { src: string; mime: string }) {
  return (
    <div className="flex w-full max-w-md flex-col items-center gap-3 px-6">
      <span className="material-symbols-outlined text-[64px] text-[var(--color-text-tertiary)]">
        graphic_eq
      </span>
      <audio controls preload="metadata" className="w-full">
        <source src={src} type={mime} />
      </audio>
    </div>
  )
}

function PdfViewer({ src, title }: { src: string; title: string }) {

  return (
    <iframe
      title={title}
      src={src}
      className="h-full w-full bg-white"
      style={{ border: 'none' }}
    />
  )
}

type ToolbarItem = {
  icon?: string
  label?: string
  title: string
  onClick: () => void
  active?: boolean
}

function Toolbar({ items }: { items: ToolbarItem[] }) {
  return (
    <div className="absolute bottom-2 right-2 flex items-center gap-0.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]/90 p-0.5 shadow-sm backdrop-blur-sm">
      {items.map((item, idx) => (
        <button
          key={idx}
          type="button"
          title={item.title}
          aria-label={item.title}
          onClick={item.onClick}
          className={`flex h-6 min-w-[24px] items-center justify-center gap-0.5 rounded px-1.5 text-[11px] ${
            item.active
              ? 'bg-[var(--color-accent)]/15 text-[var(--color-accent)]'
              : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
          }`}
        >
          {item.icon && (
            <span className="material-symbols-outlined text-[14px]">{item.icon}</span>
          )}
          {item.label && <span>{item.label}</span>}
        </button>
      ))}
    </div>
  )
}
