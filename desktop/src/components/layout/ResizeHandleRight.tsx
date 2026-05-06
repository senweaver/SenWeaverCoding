import { useCallback, useRef } from 'react'
import { RIGHT_SIDEBAR_BOUNDS, useUIStore } from '../../stores/uiStore'

export function ResizeHandleRight() {
  const setWidth = useUIStore((s) => s.setRightSidebarWidth)
  const widthRef = useRef(0)
  const startXRef = useRef(0)
  const animFrame = useRef<number | null>(null)

  const onMouseDown = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    event.preventDefault()
    widthRef.current = useUIStore.getState().rightSidebarWidth
    startXRef.current = event.clientX
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'

    const onMove = (ev: MouseEvent) => {
      const dx = startXRef.current - ev.clientX
      const next = Math.min(
        RIGHT_SIDEBAR_BOUNDS.max,
        Math.max(RIGHT_SIDEBAR_BOUNDS.min, widthRef.current + dx),
      )
      if (animFrame.current !== null) cancelAnimationFrame(animFrame.current)
      animFrame.current = requestAnimationFrame(() => setWidth(next))
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
    }

    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [setWidth])

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      data-testid="right-sidebar-resize-handle"
      onMouseDown={onMouseDown}
      className="group relative flex-shrink-0 w-1 cursor-col-resize bg-transparent hover:bg-[var(--color-accent)]/30 transition-colors"
    >
      <div className="pointer-events-none absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-[var(--color-border)] group-hover:bg-[var(--color-accent)]" />
    </div>
  )
}
