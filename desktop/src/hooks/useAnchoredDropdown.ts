// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useLayoutEffect, useRef, useState } from 'react'
import type { CSSProperties, RefObject } from 'react'

type Options = {
  estimatedHeight?: number
  align?: 'left' | 'right'
  gap?: number
  preferUp?: boolean
  anchorRef?: RefObject<HTMLElement | null>
  viewportMargin?: number
  minVisible?: number
  overflow?: CSSProperties['overflowY']
}

const DEFAULT_Z = 30000

const MEASURE_STYLE: CSSProperties = {
  position: 'fixed',
  top: 0,
  left: 0,
  zIndex: DEFAULT_Z,
  visibility: 'hidden',
  pointerEvents: 'none',
  maxHeight: 'none',
  overflowY: 'visible',
}

let overlayRoot: HTMLDivElement | null = null

export function getOverlayRoot(): HTMLElement {
  if (typeof document === 'undefined') {
    return overlayRoot as unknown as HTMLElement
  }
  if (overlayRoot?.isConnected) return overlayRoot
  overlayRoot = document.createElement('div')
  overlayRoot.id = 'sen-overlay-root'
  overlayRoot.style.position = 'absolute'
  overlayRoot.style.left = '0'
  overlayRoot.style.top = '0'
  overlayRoot.style.zIndex = String(DEFAULT_Z)
  document.body.appendChild(overlayRoot)
  return overlayRoot
}

function stylesEqual(a: CSSProperties | null, b: CSSProperties): boolean {
  if (!a) return false
  return (
    a.top === b.top &&
    a.left === b.left &&
    a.right === b.right &&
    a.maxHeight === b.maxHeight &&
    a.height === b.height &&
    a.overflowY === b.overflowY &&
    a.visibility === b.visibility &&
    a.pointerEvents === b.pointerEvents
  )
}

function readContentHeight(menu: HTMLElement, fallback: number): number {
  return Math.max(Math.ceil(menu.scrollHeight || menu.getBoundingClientRect().height || fallback), 1)
}

function triggerScrollParent(anchor: HTMLElement | null): HTMLElement | null {
  let el = anchor?.parentElement ?? null
  while (el && el !== document.body) {
    const overflowY = getComputedStyle(el).overflowY
    if ((overflowY === 'auto' || overflowY === 'scroll') && el.scrollHeight > el.clientHeight + 1) {
      return el
    }
    el = el.parentElement
  }
  return null
}

function nearestScroller(start: EventTarget | null, root: HTMLElement): HTMLElement | null {
  let el: HTMLElement | null =
    start instanceof HTMLElement ? start : start instanceof Node ? start.parentElement : null
  while (el) {
    const overflowY = getComputedStyle(el).overflowY
    if ((overflowY === 'auto' || overflowY === 'scroll') && el.scrollHeight > el.clientHeight + 1) {
      return el
    }
    if (el === root) break
    el = el.parentElement
  }
  return null
}

export function useAnchoredDropdown<TTrigger extends HTMLElement = HTMLElement>(
  open: boolean,
  onClose: () => void,
  options?: Options,
) {
  const triggerRef = useRef<TTrigger>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const [style, setStyle] = useState<CSSProperties | null>(null)

  const estimatedHeight = options?.estimatedHeight ?? 320
  const align = options?.align ?? 'left'
  const gap = options?.gap ?? 4
  const preferUp = options?.preferUp ?? false
  const externalAnchor = options?.anchorRef
  const viewportMargin = options?.viewportMargin ?? 8
  const minVisible = options?.minVisible ?? 96
  const overflow = options?.overflow ?? 'auto'

  const updatePosition = useCallback(() => {
    const anchor = externalAnchor?.current ?? triggerRef.current
    if (!anchor) return
    const rect = anchor.getBoundingClientRect()
    const spaceAbove = Math.max(0, rect.top - viewportMargin)
    const spaceBelow = Math.max(0, window.innerHeight - rect.bottom - viewportMargin)
    const menu = menuRef.current
    const contentHeight = menu ? readContentHeight(menu, estimatedHeight) : estimatedHeight

    const canFitBelow = spaceBelow >= contentHeight
    const canFitAbove = spaceAbove >= contentHeight
    let direction: 'up' | 'down'
    if (canFitBelow && !canFitAbove) {
      direction = 'down'
    } else if (canFitAbove && !canFitBelow) {
      direction = 'up'
    } else if (canFitBelow && canFitAbove) {
      direction = preferUp ? 'up' : 'down'
    } else {
      direction = spaceBelow >= spaceAbove ? 'down' : 'up'
    }

    const available = direction === 'down' ? spaceBelow : spaceAbove
    const needsScroll = contentHeight > available + 1
    const maxHeight = needsScroll ? Math.max(minVisible, available) : contentHeight
    const next: CSSProperties = {
      position: 'fixed',
      zIndex: DEFAULT_Z,
      visibility: 'visible',
      pointerEvents: 'auto',
      boxSizing: 'border-box',
      overscrollBehavior: 'contain',
    }
    if (align === 'right') {
      next.right = Math.max(viewportMargin, window.innerWidth - rect.right)
    } else {
      const menuWidth = menu?.offsetWidth ?? 0
      if (menuWidth > 0) {
        const maxLeft = window.innerWidth - viewportMargin - menuWidth
        next.left = Math.max(viewportMargin, Math.min(rect.left, maxLeft))
      } else {
        next.left = Math.max(viewportMargin, rect.left)
      }
    }
    if (direction === 'down') {
      next.top = rect.bottom + gap
    } else {
      next.top = rect.top - gap - maxHeight
    }
    if (overflow === 'hidden') {
      next.overflowY = 'hidden'
      next.maxHeight = maxHeight
      if (needsScroll) next.height = maxHeight
    } else if (needsScroll) {
      next.maxHeight = maxHeight
      next.overflowY = 'auto'
    } else {
      next.overflowY = 'visible'
    }
    setStyle((prev) => (stylesEqual(prev, next) ? prev : next))
  }, [align, estimatedHeight, externalAnchor, gap, minVisible, overflow, preferUp, viewportMargin])

  useLayoutEffect(() => {
    if (!open) {
      setStyle(null)
      return
    }
    let observer: ResizeObserver | null = null
    updatePosition()
    const frame = window.requestAnimationFrame(() => {
      updatePosition()
      const menu = menuRef.current
      if (!menu) return
      observer = new ResizeObserver(() => updatePosition())
      observer.observe(menu)
    })
    window.addEventListener('scroll', updatePosition, true)
    window.addEventListener('resize', updatePosition)
    return () => {
      window.cancelAnimationFrame(frame)
      observer?.disconnect()
      window.removeEventListener('scroll', updatePosition, true)
      window.removeEventListener('resize', updatePosition)
    }
  }, [open, updatePosition])

  useLayoutEffect(() => {
    if (!open) return
    const handleClick = (e: MouseEvent) => {
      const target = e.target as Node
      const anchor = externalAnchor?.current ?? triggerRef.current
      if (anchor?.contains(target)) return
      if (menuRef.current?.contains(target)) return
      onClose()
    }
    const handleEsc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopImmediatePropagation()
        onClose()
      }
    }
    document.addEventListener('mousedown', handleClick)
    document.addEventListener('keydown', handleEsc, true)
    return () => {
      document.removeEventListener('mousedown', handleClick)
      document.removeEventListener('keydown', handleEsc, true)
    }
  }, [open, onClose, externalAnchor])

  useLayoutEffect(() => {
    if (!open) return
    const onWheel = (event: WheelEvent) => {
      const menu = menuRef.current
      const target = event.target
      if (menu && target instanceof Node && menu.contains(target)) {
        const scroller = nearestScroller(target, menu)
        if (!scroller) {
          event.preventDefault()
        } else {
          const atTop = scroller.scrollTop <= 0 && event.deltaY < 0
          const atBottom =
            scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 1 &&
            event.deltaY > 0
          if (atTop || atBottom) event.preventDefault()
        }
        event.stopPropagation()
        return
      }
      const anchor = externalAnchor?.current ?? triggerRef.current
      const parent = triggerScrollParent(anchor)
      if (parent && target instanceof Node && parent.contains(target)) {
        event.preventDefault()
        event.stopPropagation()
      }
    }
    document.addEventListener('wheel', onWheel, { capture: true, passive: false })
    return () => {
      document.removeEventListener('wheel', onWheel, true)
    }
  }, [open, externalAnchor])

  return {
    triggerRef,
    menuRef,
    style: open ? (style ?? MEASURE_STYLE) : null,
    updatePosition,
    portalTarget: typeof document !== 'undefined' ? getOverlayRoot() : (null as unknown as HTMLElement),
  }
}
