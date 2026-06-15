// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import katex from 'katex'
import { marked, type Tokens } from 'marked'
import { normalizeCodeLanguage } from './mermaidDetect'

export type CodeBlock = {
  id: string
  code: string
  language: string | undefined
}

export type ParsedMarkdown = {
  html: string
  codeBlocks: CodeBlock[]
}

const renderer = new marked.Renderer()

let activeCodeBlockSink: CodeBlock[] | null = null

renderer.code = function ({ text, lang }: Tokens.Code) {
  const sink = activeCodeBlockSink
  if (!sink) {
    const escaped = text.replace(
      /[<&]/g,
      (ch) => (ch === '<' ? '&lt;' : '&amp;'),
    )
    return `<pre><code>${escaped}</code></pre>`
  }
  const id = `cb-${sink.length}`
  sink.push({
    id,
    code: text,
    language: normalizeCodeLanguage(lang || undefined) || undefined,
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

export function parseMarkdown(content: string): ParsedMarkdown {
  const sink: CodeBlock[] = []
  const previousSink = activeCodeBlockSink
  activeCodeBlockSink = sink
  try {
    const html = marked.parse(content) as string
    return { html, codeBlocks: sink }
  } finally {
    activeCodeBlockSink = previousSink
  }
}
