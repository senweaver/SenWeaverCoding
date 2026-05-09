// SPDX-License-Identifier: MIT

import { useCallback, useEffect, useRef, useState } from 'react'

export type SplitterOrientation = 'horizontal' | 'vertical'

type Props = {
  orientation: SplitterOrientation
  onDrag: (deltaPx: number) => void
  onCommit?: () => void
  ariaLabel?: string
  className?: string
}

export function BrowserPanelSplitter({
  orientation,
  onDrag,
  onCommit,
  ariaLabel,
  className,
}: Props) {
  const [active, setActive] = useState(false)
  const lastPosRef = useRef<number>(0)
  const activeRef = useRef(false)

  const handlePointerMove = useCallback(
    (e: PointerEvent) => {
      if (!activeRef.current) return
      const cur = orientation === 'horizontal' ? e.clientY : e.clientX
      const delta = cur - lastPosRef.current
      lastPosRef.current = cur
      if (delta !== 0) onDrag(delta)
    },
    [onDrag, orientation],
  )

  const handlePointerUp = useCallback(() => {
    if (!activeRef.current) return
    activeRef.current = false
    setActive(false)
    document.body.style.removeProperty('cursor')
    document.body.style.removeProperty('user-select')
    window.removeEventListener('pointermove', handlePointerMove)
    window.removeEventListener('pointerup', handlePointerUp)
    window.removeEventListener('pointercancel', handlePointerUp)
    onCommit?.()
  }, [handlePointerMove, onCommit])

  useEffect(() => {
    return () => {
      window.removeEventListener('pointermove', handlePointerMove)
      window.removeEventListener('pointerup', handlePointerUp)
      window.removeEventListener('pointercancel', handlePointerUp)
    }
  }, [handlePointerMove, handlePointerUp])

  const onPointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      e.preventDefault()
      e.stopPropagation()
      activeRef.current = true
      setActive(true)
      lastPosRef.current = orientation === 'horizontal' ? e.clientY : e.clientX
      document.body.style.cursor = orientation === 'horizontal' ? 'row-resize' : 'col-resize'
      document.body.style.userSelect = 'none'
      window.addEventListener('pointermove', handlePointerMove)
      window.addEventListener('pointerup', handlePointerUp)
      window.addEventListener('pointercancel', handlePointerUp)
    },
    [handlePointerMove, handlePointerUp, orientation],
  )

  const baseClass =
    orientation === 'horizontal'
      ? 'group relative h-1 w-full shrink-0 cursor-row-resize'
      : 'group relative h-full w-1 shrink-0 cursor-col-resize'

  const indicatorClass =
    orientation === 'horizontal'
      ? 'absolute left-0 right-0 top-1/2 h-px -translate-y-1/2 transition-colors'
      : 'absolute top-0 bottom-0 left-1/2 w-px -translate-x-1/2 transition-colors'

  return (
    <div
      role="separator"
      aria-orientation={orientation}
      aria-label={ariaLabel}
      className={`${baseClass} ${className ?? ''}`.trim()}
      onPointerDown={onPointerDown}
      data-active={active ? 'true' : 'false'}
    >
      <span
        className={`${indicatorClass} ${
          active
            ? 'bg-[var(--color-brand)]'
            : 'bg-[var(--color-border)] group-hover:bg-[var(--color-border-strong,var(--color-brand))]'
        }`}
      />
    </div>
  )
}
