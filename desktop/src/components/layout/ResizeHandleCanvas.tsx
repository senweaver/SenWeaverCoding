// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useRef } from 'react'
import {
  DESIGNER_CANVAS_WIDTH_BOUNDS,
  useDesignerCanvasStore,
} from '../../stores/designerCanvasStore'
import { useTabStore } from '../../stores/tabStore'

export function ResizeHandleCanvas() {
  const setColumnWidth = useDesignerCanvasStore((s) => s.setColumnWidth)
  const widthRef = useRef(0)
  const startXRef = useRef(0)
  const animFrame = useRef<number | null>(null)
  const sessionRef = useRef<string | null>(null)

  const onMouseDown = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      if (event.button !== 0) return
      event.preventDefault()
      const sessionId = useTabStore.getState().activeTabId
      if (!sessionId) return
      sessionRef.current = sessionId

      const aside = document.querySelector(
        '[data-testid="designer-canvas-panel"]',
      ) as HTMLElement | null
      const renderedWidth = aside ? aside.getBoundingClientRect().width : 0
      const fallback =
        useDesignerCanvasStore.getState().panels[sessionId]?.columnWidth ??
        DESIGNER_CANVAS_WIDTH_BOUNDS.default
      const initialWidth = Math.max(
        DESIGNER_CANVAS_WIDTH_BOUNDS.min,
        Math.min(
          DESIGNER_CANVAS_WIDTH_BOUNDS.max,
          Math.round(renderedWidth > 0 ? renderedWidth : fallback),
        ),
      )
      widthRef.current = initialWidth
      setColumnWidth(sessionId, initialWidth)

      startXRef.current = event.clientX
      document.body.style.cursor = 'col-resize'
      document.body.style.userSelect = 'none'

      let lastNext = initialWidth

      const onMove = (ev: MouseEvent) => {
        const dx = startXRef.current - ev.clientX
        lastNext = Math.min(
          DESIGNER_CANVAS_WIDTH_BOUNDS.max,
          Math.max(DESIGNER_CANVAS_WIDTH_BOUNDS.min, widthRef.current + dx),
        )
        if (animFrame.current !== null) cancelAnimationFrame(animFrame.current)
        animFrame.current = requestAnimationFrame(() => {
          const target = sessionRef.current
          if (target) setColumnWidth(target, lastNext)
        })
      }

      const onUp = () => {
        document.body.style.cursor = ''
        document.body.style.userSelect = ''
        window.removeEventListener('mousemove', onMove)
        window.removeEventListener('mouseup', onUp)
        if (animFrame.current !== null) {
          cancelAnimationFrame(animFrame.current)
          animFrame.current = null
        }
        const target = sessionRef.current
        if (target) setColumnWidth(target, lastNext)
        sessionRef.current = null
      }

      window.addEventListener('mousemove', onMove)
      window.addEventListener('mouseup', onUp)
    },
    [setColumnWidth],
  )

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      data-testid="designer-canvas-resize-handle"
      onMouseDown={onMouseDown}
      className="group relative flex-shrink-0 w-1 cursor-col-resize bg-transparent hover:bg-[var(--color-accent)]/30 transition-colors"
    >
      <div className="pointer-events-none absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-[var(--color-border)] group-hover:bg-[var(--color-accent)]" />
    </div>
  )
}
