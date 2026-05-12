import { useMemo, useCallback } from 'react'
import DOMPurify from 'dompurify'
import katex from 'katex'
import { marked, type Tokens } from 'marked'
import { CodeViewer } from '../chat/CodeViewer'
import { MermaidRenderer } from '../chat/MermaidRenderer'

type Props = {
  content: string
  variant?: 'default' | 'document'
  className?: string

  scale?: 'default' | 'chat'
}

type CodeBlock = {
  id: string
  code: string
  language: string | undefined
}

const MERMAID_LANGUAGE = 'mermaid'
const PLAINTEXT_LANGUAGES = new Set(['', 'text', 'plaintext', 'plain'])
const MERMAID_DIAGRAM_START = /^(graph|flowchart|sequenceDiagram|classDiagram|stateDiagram(?:-v2)?|erDiagram|journey|gantt|pie|gitGraph|mindmap|timeline|requirementDiagram|quadrantChart|xychart-beta|sankey-beta|block-beta|packet-beta|architecture|kanban)\b/i

function normalizeCodeLanguage(language: string | undefined): string | undefined {
  const normalized = language?.trim().split(/\s+/)[0]?.toLowerCase()
  return normalized || undefined
}

function looksLikeMermaid(code: string): boolean {
  const firstMeaningfulLine = code
    .split('\n')
    .map((line) => line.trim())
    .find(Boolean)

  return firstMeaningfulLine ? MERMAID_DIAGRAM_START.test(firstMeaningfulLine) : false
}

function shouldRenderAsMermaid(block: CodeBlock): boolean {
  const normalizedLanguage = normalizeCodeLanguage(block.language)

  if (normalizedLanguage === MERMAID_LANGUAGE) {
    return true
  }

  if (!PLAINTEXT_LANGUAGES.has(normalizedLanguage ?? '')) {
    return false
  }

  return looksLikeMermaid(block.code)
}

const renderer = new marked.Renderer()

let pendingCodeBlocks: CodeBlock[] = []

renderer.code = function ({ text, lang }: Tokens.Code) {
  const id = `cb-${pendingCodeBlocks.length}`
  pendingCodeBlocks.push({
    id,
    code: text,
    language: normalizeCodeLanguage(lang || undefined),
  })
  return `<div data-codeblock-id="${id}"></div>`
}

function renderKatex(source: string, displayMode: boolean): string {
  try {
    return katex.renderToString(source, {
      throwOnError: false,
      displayMode,
      trust: false,
      output: 'html',
      strict: 'warn',
    })
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    const safe = message.replace(/[<&]/g, (ch) => (ch === '<' ? '&lt;' : '&amp;'))
    return `<span class="katex-error" title="${safe}">${safe}</span>`
  }
}

const BLOCK_MATH_RE = /^\$\$([\s\S]+?)\$\$(?:\n|$)/
const INLINE_MATH_RE = /^\$((?:\\.|[^$\\])+?)\$/

const blockMathExtension = {
  name: 'blockMath',
  level: 'block' as const,
  start(src: string) {
    const idx = src.indexOf('$$')
    return idx >= 0 ? idx : undefined
  },
  tokenizer(src: string) {
    const match = BLOCK_MATH_RE.exec(src)
    if (!match) return undefined
    const text = match[1]?.trim() ?? ''
    return {
      type: 'blockMath',
      raw: match[0],
      text,
    }
  },
  renderer(token: { text: string }) {
    return `<div class="md-math-block">${renderKatex(token.text, true)}</div>`
  },
}

const inlineMathExtension = {
  name: 'inlineMath',
  level: 'inline' as const,
  start(src: string) {
    const idx = src.indexOf('$')
    return idx >= 0 ? idx : undefined
  },
  tokenizer(src: string) {
    if (src.startsWith('$$')) return undefined
    const match = INLINE_MATH_RE.exec(src)
    if (!match) return undefined
    const text = match[1]?.trim() ?? ''
    if (!text) return undefined
    return {
      type: 'inlineMath',
      raw: match[0],
      text,
    }
  },
  renderer(token: { text: string }) {
    return `<span class="md-math-inline">${renderKatex(token.text, false)}</span>`
  },
}

marked.setOptions({
  breaks: true,
  gfm: true,
})
marked.use({ renderer })
marked.use({
  extensions: [blockMathExtension, inlineMathExtension],
})

function enhanceMarkdownHtml(html: string): string {
  const cleanHtml = DOMPurify.sanitize(html, {
    ADD_TAGS: ['use'],
    ADD_ATTR: ['xlink:href'],
  })

  if (typeof document === 'undefined') {
    return cleanHtml
  }

  const container = document.createElement('div')
  container.innerHTML = cleanHtml

  container.querySelectorAll('table').forEach((table) => {
    if (table.parentElement?.classList.contains('md-table-wrap')) return
    const wrapper = document.createElement('div')
    wrapper.className = 'md-table-wrap'
    table.parentNode?.insertBefore(wrapper, table)
    wrapper.appendChild(table)
  })

  container.querySelectorAll('a[href]').forEach((link) => {
    link.setAttribute('target', '_blank')
    link.setAttribute('rel', 'noreferrer noopener')
  })

  return container.innerHTML
}

function parseMarkdown(content: string): { html: string; codeBlocks: CodeBlock[] } {
  pendingCodeBlocks = []
  const html = marked.parse(content) as string
  const codeBlocks = [...pendingCodeBlocks]
  pendingCodeBlocks = []
  return { html, codeBlocks }
}

const BASE_PROSE_CLASSES = `markdown-prose prose prose-sm max-w-none text-[var(--color-text-primary)]
  prose-headings:text-[var(--color-text-primary)] prose-headings:font-semibold
  prose-p:my-2 prose-p:leading-relaxed
  prose-p:break-words
  prose-code:text-[13px] prose-code:text-[var(--color-code-fg)] prose-code:font-[var(--font-mono)] prose-code:bg-[var(--color-code-bg)] prose-code:border prose-code:border-[var(--color-border)] prose-code:px-1.5 prose-code:py-0.5 prose-code:rounded-md prose-code:before:hidden prose-code:after:hidden
  prose-pre:!bg-transparent prose-pre:!p-0 prose-pre:!shadow-none
  prose-a:text-[var(--color-text-accent)] prose-a:no-underline hover:prose-a:underline
  prose-strong:text-[var(--color-text-primary)]
  prose-ul:my-2 prose-ol:my-2
  prose-li:my-0.5
  prose-table:my-0 prose-table:w-full prose-table:table-auto prose-table:text-sm
  prose-th:bg-[var(--color-surface-info)] prose-th:px-3 prose-th:py-2 prose-th:text-left prose-th:whitespace-normal prose-th:break-words prose-th:align-top prose-th:border-b prose-th:border-[var(--color-border)]
  prose-td:px-3 prose-td:py-2 prose-td:border-b prose-td:border-[var(--color-border)] prose-td:whitespace-normal prose-td:break-words prose-td:align-top prose-td:bg-[var(--color-surface)]
  [&_.md-table-wrap]:my-5 [&_.md-table-wrap]:overflow-x-auto [&_.md-table-wrap]:rounded-xl [&_.md-table-wrap]:border [&_.md-table-wrap]:border-[var(--color-border)] [&_.md-table-wrap]:bg-[var(--color-surface-container-lowest)]`

const DOCUMENT_PROSE_CLASSES = `
  prose-p:text-[15px] prose-p:leading-7
  prose-headings:scroll-mt-6 prose-headings:tracking-[-0.01em]
  prose-h1:mb-4 prose-h1:text-2xl prose-h1:font-semibold prose-h1:leading-tight
  prose-h2:mt-8 prose-h2:mb-3 prose-h2:border-b prose-h2:border-[var(--color-border)] prose-h2:pb-2 prose-h2:text-xl prose-h2:font-semibold
  prose-h3:mt-6 prose-h3:mb-2 prose-h3:text-base prose-h3:font-semibold
  prose-h4:mt-5 prose-h4:mb-2 prose-h4:text-sm prose-h4:font-semibold
  prose-blockquote:my-4 prose-blockquote:rounded-r-lg prose-blockquote:border-l-4 prose-blockquote:border-[var(--color-outline-variant)] prose-blockquote:bg-[var(--color-surface-container-low)] prose-blockquote:px-4 prose-blockquote:py-2 prose-blockquote:italic
  prose-hr:my-6 prose-hr:border-[var(--color-border)]
  prose-img:rounded-lg prose-img:border prose-img:border-[var(--color-border)]
  prose-kbd:rounded prose-kbd:border prose-kbd:border-[var(--color-border)] prose-kbd:bg-[var(--color-surface-container-lowest)] prose-kbd:px-1.5 prose-kbd:py-0.5 prose-kbd:font-[var(--font-mono)] prose-kbd:text-[12px] prose-kbd:font-normal prose-kbd:text-[var(--color-text-secondary)] prose-kbd:shadow-none
  prose-ul:pl-5 prose-ul:[&>li]:marker:text-[var(--color-text-tertiary)]
  prose-ol:pl-5 prose-ol:[&>li]:marker:text-[var(--color-text-tertiary)]
  prose-li:my-1.5
  prose-table:my-0`

const CHAT_BODY_SCALE_CLASSES = `
  !text-[13px]
  prose-p:!text-[13px] prose-p:!leading-relaxed
  prose-li:!text-[13px]
  prose-code:!text-[12px]
  prose-table:!text-[12px]
  prose-th:!text-[12px] prose-td:!text-[12px]
`

const CHAT_HEADING_DEFAULT_CLASSES = `
  prose-h1:!text-base prose-h2:!text-[15px] prose-h3:!text-[14px] prose-h4:!text-[13px]
`

const CHAT_DOCUMENT_PROSE_CLASSES = `
  prose-p:!text-[13px] prose-p:!leading-[1.62]
  prose-headings:scroll-mt-6 prose-headings:tracking-[-0.01em]
  prose-h1:!mb-3 prose-h1:!text-xl prose-h1:!font-semibold prose-h1:!leading-tight
  prose-h2:!mt-6 prose-h2:!mb-2 prose-h2:border-b prose-h2:border-[var(--color-border)] prose-h2:!pb-2 prose-h2:!text-lg prose-h2:!font-semibold
  prose-h3:!mt-5 prose-h3:!mb-1.5 prose-h3:!text-[13px] prose-h3:!font-semibold
  prose-h4:!mt-4 prose-h4:!mb-1.5 prose-h4:!text-[13px] prose-h4:!font-semibold
  prose-blockquote:my-3 prose-blockquote:rounded-r-lg prose-blockquote:border-l-4 prose-blockquote:border-[var(--color-outline-variant)] prose-blockquote:bg-[var(--color-surface-container-low)] prose-blockquote:px-3 prose-blockquote:py-2 prose-blockquote:italic
  prose-hr:!my-5 prose-hr:border-[var(--color-border)]
  prose-img:rounded-lg prose-img:border prose-img:border-[var(--color-border)]
  prose-kbd:rounded prose-kbd:border prose-kbd:border-[var(--color-border)] prose-kbd:bg-[var(--color-surface-container-lowest)] prose-kbd:px-1.5 prose-kbd:py-0.5 prose-kbd:font-[var(--font-mono)] prose-kbd:!text-[11px] prose-kbd:font-normal prose-kbd:text-[var(--color-text-secondary)] prose-kbd:shadow-none
  prose-ul:pl-5 prose-ul:[&>li]:marker:text-[var(--color-text-tertiary)]
  prose-ol:pl-5 prose-ol:[&>li]:marker:text-[var(--color-text-tertiary)]
  prose-li:my-1
  prose-table:my-0`

function getProseClasses(
  variant: 'default' | 'document',
  className?: string,
  scale: 'default' | 'chat' = 'default',
) {
  const chunks: string[] = [BASE_PROSE_CLASSES]

  if (scale === 'chat') {
    chunks.push(CHAT_BODY_SCALE_CLASSES)
  }

  if (variant === 'document') {
    chunks.push(scale === 'chat' ? CHAT_DOCUMENT_PROSE_CLASSES : DOCUMENT_PROSE_CLASSES)
  } else if (scale === 'chat') {
    chunks.push(CHAT_HEADING_DEFAULT_CLASSES)
  }

  if (className) chunks.push(className)

  return chunks.filter(Boolean).join(' ')
}

export function MarkdownRenderer({ content, variant = 'default', className, scale = 'default' }: Props) {
  const { html, codeBlocks } = useMemo(() => parseMarkdown(content), [content])
  const proseClasses = useMemo(
    () => getProseClasses(variant, className, scale),
    [variant, className, scale],
  )

  const parts = useMemo(() => {
    if (codeBlocks.length === 0) {
      return [{ type: 'html' as const, content: html }]
    }

    const result: Array<{ type: 'html'; content: string } | { type: 'code'; block: CodeBlock }> = []
    let remaining = html

    for (const block of codeBlocks) {
      const marker = `<div data-codeblock-id="${block.id}"></div>`
      const idx = remaining.indexOf(marker)
      if (idx === -1) continue

      const before = remaining.slice(0, idx)
      if (before) {
        result.push({ type: 'html', content: before })
      }
      result.push({ type: 'code', block })
      remaining = remaining.slice(idx + marker.length)
    }

    if (remaining) {
      result.push({ type: 'html', content: remaining })
    }

    return result
  }, [html, codeBlocks])

  const handleClick = useCallback(async (event: React.MouseEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement | null
    const button = target?.closest<HTMLButtonElement>('[data-copy-code]')
    if (!button) return

    const text = button.getAttribute('data-copy-code')
    if (!text) return

    try {
      await navigator.clipboard.writeText(text)
      const original = button.textContent
      button.textContent = 'Copied'
      window.setTimeout(() => {
        button.textContent = original
      }, 1500)
    } catch {

    }
  }, [])

  if (codeBlocks.length === 0) {
    const cleanHtml = enhanceMarkdownHtml(html)
    return (
      <div
        className={proseClasses}
        dangerouslySetInnerHTML={{ __html: cleanHtml }}
        onClick={handleClick}
      />
    )
  }

  return (
    <div className={proseClasses} onClick={handleClick}>
      {parts.map((part, i) =>
        part.type === 'html' ? (
          <div key={i} dangerouslySetInnerHTML={{ __html: enhanceMarkdownHtml(part.content) }} />
        ) : shouldRenderAsMermaid(part.block) ? (
          <MermaidRenderer key={part.block.id} code={part.block.code} />
        ) : (
          <div key={part.block.id} className="my-4">
            <CodeViewer
              code={part.block.code}
              language={part.block.language}
            />
          </div>
        )
      )}
    </div>
  )
}
