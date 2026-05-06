import { CodeViewer } from './CodeViewer'
import { useMemo, useState } from 'react'
import { useTranslation } from '../../i18n'
import { InlineImageGallery } from './InlineImageGallery'

type Props = {
  content: unknown
  isError: boolean
  toolName?: string
  standalone?: boolean
}

const PREVIEW_LIMIT = 200

export function ToolResultBlock({ content, isError, toolName, standalone = true }: Props) {
  const [expanded, setExpanded] = useState(false)
  const t = useTranslation()

  const previewInfo = useMemo(() => extractTextPreview(content, PREVIEW_LIMIT + 1), [content])
  const fullText = useMemo(
    () => (expanded ? extractText(content) : ''),
    [content, expanded],
  )

  if (!standalone) return null

  const preview = previewInfo.text.slice(0, PREVIEW_LIMIT)
  const hasMore = previewInfo.truncated
  const text = expanded ? fullText : preview

  return (
    <div className={`mb-2 overflow-hidden rounded-xl border ${
      isError
        ? 'border-[var(--color-error)]/20'
        : 'border-[var(--color-outline-variant)]/20'
    }`}>
      {}
      <button
        type="button"
        onClick={() => setExpanded((value) => !value)}
        className={`flex w-full items-center justify-between px-3 py-2 text-left text-[10px] font-bold uppercase tracking-wider ${
        isError
          ? 'bg-[var(--color-error-container)] text-[var(--color-error)]'
          : 'bg-[var(--color-surface-container-high)] text-[var(--color-outline)]'
      }`}
      >
        <span className="flex items-center gap-1.5">
          <span className="material-symbols-outlined text-[12px]">
            {isError ? 'error' : 'check_circle'}
          </span>
          {toolName ? t('tool.result', { toolName }) : t('tool.resultGeneric')}
        </span>
        <span className={`px-2 py-0.5 rounded-full text-[9px] ${
          isError
            ? 'bg-[var(--color-error)]/10'
            : 'bg-[var(--color-diff-added-bg)] text-[var(--color-diff-added-text)]'
        }`}>
          {isError ? t('tool.error') : t('tool.success')}
        </span>
      </button>

      {}
      {expanded && <InlineImageGallery text={text} />}

      {}
      {expanded ? (
        isError ? (
          <div className="bg-[var(--color-error-container)]/50 px-3 py-2.5 font-[var(--font-mono)] text-[11px] leading-[1.5] whitespace-pre-wrap break-words text-[var(--color-error)]">
            {text}
          </div>
        ) : (
          <CodeViewer
            code={text}
            language="plaintext"
            maxLines={12}
          />
        )
      ) : (
        <div className="bg-[var(--color-surface-container-lowest)] px-3 py-2 font-[var(--font-mono)] text-[10px] leading-[1.35] text-[var(--color-text-tertiary)]">
          {preview}
          {hasMore ? '…' : ''}
        </div>
      )}

      {hasMore && (
        <button
          onClick={() => setExpanded((value) => !value)}
          className="w-full py-1 text-[10px] font-medium text-[var(--color-text-accent)] hover:underline bg-[var(--color-surface-container-low)] border-t border-[var(--color-outline-variant)]/10"
        >
          {expanded
            ? t('tool.showLess')
            : t('tool.showMore', {
                count: expanded ? Math.max(0, fullText.length - PREVIEW_LIMIT) : 0,
              })}
        </button>
      )}
    </div>
  )
}

function extractText(content: unknown): string {
  if (typeof content === 'string') return content
  if (Array.isArray(content)) {
    return content
      .map((c: any) => (typeof c === 'string' ? c : c?.text || ''))
      .filter(Boolean)
      .join('\n')
  }
  if (content && typeof content === 'object') {
    return JSON.stringify(content, null, 2)
  }
  return String(content ?? '')
}

function extractTextPreview(
  content: unknown,
  maxChars: number,
): { text: string; truncated: boolean } {
  if (typeof content === 'string') {
    if (content.length <= maxChars) return { text: content, truncated: false }
    return { text: content.slice(0, maxChars), truncated: true }
  }
  if (Array.isArray(content)) {
    let buf = ''
    for (const c of content as unknown[]) {
      const piece = typeof c === 'string' ? c : ((c as { text?: string })?.text || '')
      if (!piece) continue
      if (buf.length > 0) buf += '\n'
      buf += piece
      if (buf.length > maxChars) {
        return { text: buf.slice(0, maxChars), truncated: true }
      }
    }
    return { text: buf, truncated: buf.length > maxChars }
  }
  if (content && typeof content === 'object') {
    const full = JSON.stringify(content, null, 2)
    if (full.length <= maxChars) return { text: full, truncated: false }
    return { text: full.slice(0, maxChars), truncated: true }
  }
  const s = String(content ?? '')
  if (s.length <= maxChars) return { text: s, truncated: false }
  return { text: s.slice(0, maxChars), truncated: true }
}
