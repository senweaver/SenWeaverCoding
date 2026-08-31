// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

type FrameParseRequest = { id: number; raw: string }

self.onmessage = (event: MessageEvent<FrameParseRequest>) => {
  const { id, raw } = event.data
  try {
    const result: unknown = JSON.parse(raw)
    self.postMessage({ id, ok: true, result })
  } catch (err) {
    self.postMessage({ id, ok: false, error: String(err) })
  }
}
