import { useMemo } from 'react'

type Props = {
  content: string
  className?: string
}

const URL_RE = /(https?:\/\/[^\s<>]+)/g
const INLINE_CODE_RE = /`([^`\n]+)`/g
const BOLD_RE = /\*\*([^*\n]+)\*\*/g
const ITALIC_RE = /(^|[^*])\*([^*\n]+)\*/g

function escapeHtml(input: string): string {
  return input
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

function applyInlineFormatting(escaped: string): string {
  let html = escaped.replace(INLINE_CODE_RE, (_m, code: string) => {
    return `<code class="md-stream-code">${code}</code>`
  })
  html = html.replace(BOLD_RE, '<strong>$1</strong>')
  html = html.replace(ITALIC_RE, '$1<em>$2</em>')
  html = html.replace(URL_RE, (url) => {
    return `<a href="${url}" target="_blank" rel="noreferrer noopener">${url}</a>`
  })
  return html
}

function buildStreamingHtml(content: string): string {
  if (!content) return ''
  const segments = content.split(/(```[\s\S]*?(?:```|$))/g)
  const parts: string[] = []
  for (const seg of segments) {
    if (!seg) continue
    if (seg.startsWith('```')) {
      const closed = seg.endsWith('```')
      const inner = closed ? seg.slice(3, -3) : seg.slice(3)
      const firstNl = inner.indexOf('\n')
      const body = firstNl >= 0 ? inner.slice(firstNl + 1) : inner
      const lang = firstNl >= 0 ? inner.slice(0, firstNl).trim() : ''
      const langAttr = lang ? ` data-lang="${escapeHtml(lang)}"` : ''
      parts.push(
        `<pre class="md-stream-pre"${langAttr}><code>${escapeHtml(body)}</code></pre>`,
      )
    } else {
      const escaped = escapeHtml(seg)
      const formatted = applyInlineFormatting(escaped).replace(/\n/g, '<br />')
      parts.push(formatted)
    }
  }
  return parts.join('')
}

export function StreamingMarkdownRenderer({ content, className }: Props) {
  const html = useMemo(() => buildStreamingHtml(content), [content])
  return (
    <div
      className={`streaming-markdown ${className ?? ''}`.trim()}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  )
}
