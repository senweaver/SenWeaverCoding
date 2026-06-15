// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { parseMarkdown } from '../lib/markdownParse'

type ParseRequest = { id: number; content: string }

self.onmessage = (event: MessageEvent<ParseRequest>) => {
  const { id, content } = event.data
  try {
    const result = parseMarkdown(content)
    self.postMessage({ id, ok: true, result })
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err) })
  }
}
