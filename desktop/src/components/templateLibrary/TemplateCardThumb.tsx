// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useRef, useState } from 'react'
import { templateLibraryApi, type TemplateItem } from '../../api/templateLibrary'
import { injectTokens } from './TemplateLibraryPreview'

const THUMB_LOGICAL_W = 1200
const THUMB_LOGICAL_H = 750

const srcDocCache = new Map<string, Promise<string | null>>()
const promptCache = new Map<string, Promise<string | null>>()

export function invalidateThumbCache() {
  srcDocCache.clear()
  promptCache.clear()
}

function thumbCacheKey(item: TemplateItem): string {
  return `${item.kind}|${item.id}|${item.surface ?? ''}`
}

async function buildSrcDoc(item: TemplateItem): Promise<string | null> {
  if (item.kind === 'designer-template') {
    const tplFile = item.files.find((f) => f.file === 'template.html')
    if (!tplFile) return null
    const res = await templateLibraryApi.file(tplFile.path)
    const body = res.content ?? ''
    return body.trim() ? body : null
  }
  if (item.kind === 'design-system') {
    const tokensFile = item.files.find((f) => f.file === 'tokens.css')
    const compFile =
      item.files.find((f) => f.file === 'components.html') ??
      item.files.find((f) => f.file.startsWith('preview/') && f.file.endsWith('.html'))
    if (!compFile) return null
    const [tokensRes, compRes] = await Promise.all([
      tokensFile
        ? templateLibraryApi.file(tokensFile.path).catch(() => ({ content: '' }))
        : Promise.resolve({ content: '' }),
      templateLibraryApi.file(compFile.path),
    ])
    const body = compRes.content ?? ''
    const tokens = tokensRes.content ?? ''
    if (!body.trim() && !tokens.trim()) return null
    return injectTokens(body, tokens)
  }
  return null
}

function loadSrcDoc(item: TemplateItem): Promise<string | null> {
  const key = thumbCacheKey(item)
  let pending = srcDocCache.get(key)
  if (!pending) {
    pending = buildSrcDoc(item).catch(() => {
      srcDocCache.delete(key)
      return null
    })
    srcDocCache.set(key, pending)
  }
  return pending
}

async function buildPromptText(item: TemplateItem): Promise<string | null> {
  const file = item.files[0]
  if (!file) return null
  const res = await templateLibraryApi.file(file.path)
  try {
    const parsed = JSON.parse(res.content ?? '{}') as { prompt?: string }
    const prompt = typeof parsed.prompt === 'string' ? parsed.prompt.trim() : ''
    return prompt.length ? prompt : null
  } catch {
    return null
  }
}

function loadPromptText(item: TemplateItem): Promise<string | null> {
  const key = thumbCacheKey(item)
  let pending = promptCache.get(key)
  if (!pending) {
    pending = buildPromptText(item).catch(() => {
      promptCache.delete(key)
      return null
    })
    promptCache.set(key, pending)
  }
  return pending
}

export function TemplateCardThumb({ item }: { item: TemplateItem }) {
  const wrapRef = useRef<HTMLDivElement | null>(null)
  const [visible, setVisible] = useState(false)
  const [srcDoc, setSrcDoc] = useState<string | null>(null)
  const [promptText, setPromptText] = useState<string | null>(null)
  const [imgFailed, setImgFailed] = useState(false)
  const [scale, setScale] = useState(0)

  const isIframeKind = item.kind === 'designer-template' || item.kind === 'design-system'
  const isPromptKind = item.kind === 'prompt-template'
  const imageUrl = isPromptKind ? item.previewImageUrl ?? null : null
  const showImage = imageUrl !== null && !imgFailed
  const needPromptFallback = isPromptKind && !showImage

  useEffect(() => {
    setImgFailed(false)
  }, [imageUrl])

  useEffect(() => {
    const el = wrapRef.current
    if (!el || visible) return
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setVisible(true)
            observer.disconnect()
            break
          }
        }
      },
      { rootMargin: '200px' },
    )
    observer.observe(el)
    return () => observer.disconnect()
  }, [visible])

  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const update = () => {
      const w = el.clientWidth
      setScale(w > 0 ? w / THUMB_LOGICAL_W : 0)
    }
    update()
    const observer = new ResizeObserver(update)
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (!visible || !isIframeKind) return
    let cancelled = false
    void loadSrcDoc(item).then((doc) => {
      if (!cancelled) setSrcDoc(doc)
    })
    return () => {
      cancelled = true
    }
  }, [visible, isIframeKind, item])

  useEffect(() => {
    if (!visible || !needPromptFallback || promptText !== null) return
    let cancelled = false
    void loadPromptText(item).then((text) => {
      if (!cancelled && text) setPromptText(text)
    })
    return () => {
      cancelled = true
    }
  }, [visible, needPromptFallback, promptText, item])

  if (!isIframeKind && !isPromptKind) return null

  return (
    <div
      ref={wrapRef}
      aria-hidden
      className="pointer-events-none relative w-full select-none overflow-hidden rounded-md border border-[var(--color-border)] bg-white"
      style={{ aspectRatio: `${THUMB_LOGICAL_W} / ${THUMB_LOGICAL_H}` }}
    >
      {showImage && visible ? (
        <img
          src={imageUrl}
          alt=""
          loading="lazy"
          onError={() => setImgFailed(true)}
          className="absolute inset-0 h-full w-full object-cover"
        />
      ) : needPromptFallback && promptText ? (
        <div className="absolute inset-0 bg-[var(--color-surface-secondary)] p-2.5">
          <p
            className="m-0 whitespace-pre-wrap break-words text-[10px] leading-[1.55] text-[var(--color-text-secondary)]"
            style={{
              display: '-webkit-box',
              WebkitLineClamp: 8,
              WebkitBoxOrient: 'vertical',
              overflow: 'hidden',
            }}
          >
            {promptText}
          </p>
          <div className="absolute inset-x-0 bottom-0 h-7 bg-gradient-to-t from-[var(--color-surface-secondary)] to-transparent" />
        </div>
      ) : !isPromptKind && srcDoc && scale > 0 ? (
        <iframe
          title={`thumb-${item.id}`}
          sandbox=""
          srcDoc={srcDoc}
          tabIndex={-1}
          scrolling="no"
          className="border-0 bg-white"
          style={{
            position: 'absolute',
            left: 0,
            top: 0,
            width: `${THUMB_LOGICAL_W}px`,
            height: `${THUMB_LOGICAL_H}px`,
            transform: `scale(${scale})`,
            transformOrigin: 'top left',
            pointerEvents: 'none',
          }}
        />
      ) : (
        <div className="absolute inset-0 bg-[var(--color-surface-secondary)]" />
      )}
    </div>
  )
}
