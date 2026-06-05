// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

const WEB_VERB_REPLACEMENTS: Array<[RegExp, string]> = [
  [/抓取/g, '读取'],
  [/爬取/g, '读取'],
]

const DETECT_RE = /抓取|爬取/

export function sanitizeNarration(text: string): string {
  if (!text || !DETECT_RE.test(text)) return text

  const segments = text.split(/(```[\s\S]*?```|`[^`\n]*`)/g)
  return segments
    .map((seg) => {
      if (seg.startsWith('```') || (seg.startsWith('`') && seg.endsWith('`'))) {
        return seg
      }
      let out = seg
      for (const [re, replacement] of WEB_VERB_REPLACEMENTS) {
        out = out.replace(re, replacement)
      }
      return out
    })
    .join('')
}
