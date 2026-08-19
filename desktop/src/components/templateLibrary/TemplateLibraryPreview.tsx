// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import type { TemplateItem } from '../../api/templateLibrary'

const IFRAME_SANDBOX = 'allow-scripts allow-popups allow-forms allow-modals'

export function injectTokens(body: string, tokens: string): string {
  const styleTag = tokens.trim() ? `<style>${tokens}</style>` : ''
  if (/<head[^>]*>/i.test(body)) {
    return body.replace(/<head([^>]*)>/i, `<head$1>${styleTag}`)
  }
  if (/<html[^>]*>/i.test(body)) {
    return body.replace(/<html([^>]*)>/i, `<html$1><head>${styleTag}</head>`)
  }
  return `<!doctype html><html><head><meta charset="utf-8" />${styleTag}</head><body>${body}</body></html>`
}

export function TemplateLibraryPreview({
  item,
  buffers,
}: {
  item: TemplateItem
  buffers: Record<string, string>
}) {
  const t = useTranslation()
  const [imgFailed, setImgFailed] = useState(false)
  const [videoFailed, setVideoFailed] = useState(false)

  useEffect(() => {
    setImgFailed(false)
  }, [item.previewImageUrl])

  useEffect(() => {
    setVideoFailed(false)
  }, [item.previewVideoUrl])

  const srcDoc = useMemo(() => {
    if (item.kind === 'design-system') {
      const tokensFile = item.files.find((f) => f.file === 'tokens.css')
      const tokens = tokensFile ? buffers[tokensFile.path] ?? '' : ''
      const compFile =
        item.files.find((f) => f.file === 'components.html') ??
        item.files.find((f) => f.file.startsWith('preview/') && f.file.endsWith('.html'))
      const body = compFile ? buffers[compFile.path] ?? '' : ''
      if (!body.trim() && !tokens.trim()) return null
      return injectTokens(body, tokens)
    }
    if (item.kind === 'designer-template') {
      const tplFile = item.files.find((f) => f.file === 'template.html')
      const body = tplFile ? buffers[tplFile.path] ?? '' : ''
      return body.trim() ? body : null
    }
    return null
  }, [item, buffers])

  if (srcDoc) {
    return (
      <iframe
        title="template-preview"
        sandbox={IFRAME_SANDBOX}
        srcDoc={srcDoc}
        className="h-full w-full border-0 bg-white"
      />
    )
  }

  if (item.kind === 'prompt-template') {
    const file = item.files[0]
    let prompt = ''
    if (file) {
      try {
        const parsed = JSON.parse(buffers[file.path] ?? '{}') as { prompt?: string }
        prompt = typeof parsed.prompt === 'string' ? parsed.prompt : ''
      } catch {
        prompt = buffers[file.path] ?? ''
      }
    }
    const showImage = Boolean(item.previewImageUrl) && !imgFailed
    const showVideo = !showImage && Boolean(item.previewVideoUrl) && !videoFailed
    const media = showImage || showVideo
    return (
      <div className="h-full overflow-auto bg-[var(--color-surface)] p-4">
        {showImage && (
          <img
            src={item.previewImageUrl ?? undefined}
            alt=""
            onError={() => setImgFailed(true)}
            className="mb-3 max-h-[40%] w-full rounded-lg object-contain"
          />
        )}
        {showVideo && (
          <video
            src={item.previewVideoUrl ?? undefined}
            controls
            onError={() => setVideoFailed(true)}
            className="mb-3 max-h-[40%] w-full rounded-lg"
          />
        )}
        <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-tertiary)]">
          {t('templateLibrary.preview.prompt')}
        </div>
        <pre className="whitespace-pre-wrap break-words text-[12px] leading-relaxed text-[var(--color-text-primary)]">
          {prompt || (media ? '' : t('templateLibrary.preview.none'))}
        </pre>
      </div>
    )
  }

  if (item.kind === 'curator-template') {
    const file = item.files[0]
    let draft = ''
    if (file) {
      try {
        const parsed = JSON.parse(buffers[file.path] ?? '{}') as {
          draftMarkdown?: string
        }
        draft = typeof parsed.draftMarkdown === 'string' ? parsed.draftMarkdown : ''
      } catch {
        draft = ''
      }
    }
    return (
      <div className="h-full overflow-auto bg-[var(--color-surface)] p-4">
        {draft ? (
          <MarkdownRenderer content={draft} variant="document" />
        ) : (
          <div className="flex h-full items-center justify-center text-[12px] text-[var(--color-text-tertiary)]">
            {t('templateLibrary.preview.none')}
          </div>
        )}
      </div>
    )
  }

  return (
    <div className="flex h-full items-center justify-center text-[12px] text-[var(--color-text-tertiary)]">
      {t('templateLibrary.preview.none')}
    </div>
  )
}
