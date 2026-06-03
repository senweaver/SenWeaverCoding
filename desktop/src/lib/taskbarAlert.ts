// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { isTauriRuntime } from './desktopRuntime'
import { useSessionRunStateStore } from '../stores/sessionRunStateStore'

let dispose: (() => void) | null = null

let badgeBytesPromise: Promise<Uint8Array | null> | null = null

async function makeBadgePng(): Promise<Uint8Array | null> {
  try {
    const size = 32
    const canvas = document.createElement('canvas')
    canvas.width = size
    canvas.height = size
    const ctx = canvas.getContext('2d')
    if (!ctx) return null

    ctx.beginPath()
    ctx.arc(size / 2, size / 2, size / 2 - 1, 0, Math.PI * 2)
    ctx.fillStyle = '#22c55e'
    ctx.fill()

    ctx.strokeStyle = '#ffffff'
    ctx.lineWidth = 3.5
    ctx.lineCap = 'round'
    ctx.lineJoin = 'round'
    ctx.beginPath()
    ctx.moveTo(size * 0.28, size * 0.52)
    ctx.lineTo(size * 0.44, size * 0.68)
    ctx.lineTo(size * 0.74, size * 0.34)
    ctx.stroke()

    const blob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, 'image/png'),
    )
    if (!blob) return null
    return new Uint8Array(await blob.arrayBuffer())
  } catch {
    return null
  }
}

function getBadge(): Promise<Uint8Array | null> {
  if (!badgeBytesPromise) badgeBytesPromise = makeBadgePng()
  return badgeBytesPromise
}

async function loadWindow() {
  const mod = await import(/* @vite-ignore */ '@tauri-apps/api/window')
  return { win: mod.getCurrentWindow(), critical: mod.UserAttentionType.Critical }
}

export function startTaskbarAlertWatcher(): () => void {
  if (dispose) return dispose
  if (!isTauriRuntime()) {
    dispose = () => {}
    return dispose
  }

  let cancelled = false
  let indicatorActive = false
  let unlistenFocus: (() => void) | undefined
  let winApi: Awaited<ReturnType<typeof loadWindow>> | null = null

  const ensureWin = async () => {
    if (!winApi) winApi = await loadWindow()
    return winApi
  }

  const showIndicator = async () => {
    try {
      const { win, critical } = await ensureWin()
      const [focused, minimized] = await Promise.all([win.isFocused(), win.isMinimized()])
      if (cancelled) return
      if (focused && !minimized) return
      const badge = await getBadge()
      await win.requestUserAttention(critical)
      if (badge) await win.setOverlayIcon(badge)
      indicatorActive = true
    } catch {
      // window APIs unavailable; ignore
    }
  }

  const clearIndicator = async () => {
    if (!indicatorActive) return
    indicatorActive = false
    try {
      const { win } = await ensureWin()
      await win.requestUserAttention(null)
      await win.setOverlayIcon(undefined)
    } catch {
      // window APIs unavailable; ignore
    }
  }

  void ensureWin()
    .then(async ({ win }) => {
      if (cancelled) return
      const fn = await win.onFocusChanged(({ payload }) => {
        if (payload) void clearIndicator()
      })
      if (cancelled) fn()
      else unlistenFocus = fn
    })
    .catch(() => {})

  const unsubscribeStore = useSessionRunStateStore.subscribe((state, prev) => {
    if (state.running === prev.running) return
    let completed = false
    for (const id of prev.running) {
      if (!state.running.has(id)) {
        completed = true
        break
      }
    }
    if (completed) void showIndicator()
  })

  dispose = () => {
    cancelled = true
    unsubscribeStore()
    unlistenFocus?.()
    dispose = null
  }
  return dispose
}
