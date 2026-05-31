// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'

const STORAGE_KEY = 'sen-workspace-tree-width'
const DEFAULT_LEFT_WIDTH = 220
const MIN_LEFT_WIDTH = 160
const MAX_LEFT_WIDTH = 400

function loadInitialWidth(): number {
  if (typeof window === 'undefined' || !window.localStorage) return DEFAULT_LEFT_WIDTH
  const raw = window.localStorage.getItem(STORAGE_KEY)
  if (!raw) return DEFAULT_LEFT_WIDTH
  const parsed = Number.parseInt(raw, 10)
  if (!Number.isFinite(parsed)) return DEFAULT_LEFT_WIDTH
  return Math.max(MIN_LEFT_WIDTH, Math.min(MAX_LEFT_WIDTH, parsed))
}

type Props = {

  left: ReactNode

  right: ReactNode
}

export function WorkspaceSplit({ left, right }: Props) {
  const containerRef = useRef<HTMLDivElement>(null)
  const [leftWidth, setLeftWidth] = useState<number>(loadInitialWidth)
  const draggingRef = useRef(false)
  const startXRef = useRef(0)
  const startWidthRef = useRef(0)
  const animFrame = useRef<number | null>(null)

  useEffect(() => {
    if (typeof window === 'undefined' || !window.localStorage) return
    window.localStorage.setItem(STORAGE_KEY, String(leftWidth))
  }, [leftWidth])

  const onMouseDown = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    draggingRef.current = true
    startXRef.current = event.clientX
    startWidthRef.current = leftWidth
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'

    const onMove = (ev: MouseEvent) => {
      if (!draggingRef.current) return
      const dx = ev.clientX - startXRef.current
      const containerWidth = containerRef.current?.clientWidth ?? Number.POSITIVE_INFINITY

      const cap = Math.max(MIN_LEFT_WIDTH, Math.min(MAX_LEFT_WIDTH, containerWidth - 240))
      const next = Math.max(MIN_LEFT_WIDTH, Math.min(cap, startWidthRef.current + dx))
      if (animFrame.current !== null) cancelAnimationFrame(animFrame.current)
      animFrame.current = requestAnimationFrame(() => setLeftWidth(next))
    }

    const onUp = () => {
      draggingRef.current = false
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
      if (animFrame.current !== null) {
        cancelAnimationFrame(animFrame.current)
        animFrame.current = null
      }
    }

    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [leftWidth])

  return (
    <div ref={containerRef} className="flex min-h-0 flex-1 overflow-hidden">
      <div
        className="flex min-h-0 flex-shrink-0 flex-col overflow-hidden border-r border-[var(--color-border)]"
        style={{ width: `${leftWidth}px` }}
      >
        {left}
      </div>
      <div
        role="separator"
        aria-orientation="vertical"
        onMouseDown={onMouseDown}
        className="group relative w-1 flex-shrink-0 cursor-col-resize bg-transparent transition-colors hover:bg-[var(--color-accent)]/30"
      >
        <div className="pointer-events-none absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-[var(--color-border)] group-hover:bg-[var(--color-accent)]" />
      </div>
      <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">{right}</div>
    </div>
  )
}
