import { useCallback, useRef } from 'react'
import { RIGHT_SIDEBAR_BOUNDS, useUIStore } from '../../stores/uiStore'
import {
  BROWSER_COLUMN_WIDTH_BOUNDS,
  useBrowserPanelStore,
} from '../../stores/browserPanelStore'
import { useTabStore } from '../../stores/tabStore'

export function ResizeHandleRight() {
  const setWidth = useUIStore((s) => s.setRightSidebarWidth)
  const widthRef = useRef(0)
  const startXRef = useRef(0)
  const animFrame = useRef<number | null>(null)

  const onMouseDown = useCallback((event: React.MouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    event.preventDefault()

    const aside = document.querySelector(
      '[data-testid="right-sidebar"]',
    ) as HTMLElement | null
    const renderedWidth = aside ? aside.getBoundingClientRect().width : 0
    const fallback = useUIStore.getState().rightSidebarWidth
    const initialWidth = Math.max(
      RIGHT_SIDEBAR_BOUNDS.min,
      Math.min(
        RIGHT_SIDEBAR_BOUNDS.max,
        Math.round(renderedWidth > 0 ? renderedWidth : fallback),
      ),
    )
    widthRef.current = initialWidth
    setWidth(initialWidth)

    const sessionId = useTabStore.getState().activeTabId
    if (sessionId) {
      const browserPanel = useBrowserPanelStore.getState().panels[sessionId]
      if (browserPanel?.visible && browserPanel.columnWidthAuto) {
        const browserAside = document.querySelector(
          '[data-testid="embedded-browser-panel"]',
        ) as HTMLElement | null
        if (browserAside) {
          const colWidth = Math.max(
            BROWSER_COLUMN_WIDTH_BOUNDS.min,
            Math.min(
              BROWSER_COLUMN_WIDTH_BOUNDS.max,
              Math.round(browserAside.getBoundingClientRect().width),
            ),
          )
          useBrowserPanelStore.getState().setColumnWidth(sessionId, colWidth)
        }
      }
    }

    startXRef.current = event.clientX
    document.body.style.cursor = 'col-resize'
    document.body.style.userSelect = 'none'

    document.dispatchEvent(new CustomEvent('browser-panel-drag-start'))

    let lastNext = initialWidth

    const onMove = (ev: MouseEvent) => {
      const dx = startXRef.current - ev.clientX
      lastNext = Math.min(
        RIGHT_SIDEBAR_BOUNDS.max,
        Math.max(RIGHT_SIDEBAR_BOUNDS.min, widthRef.current + dx),
      )
      if (animFrame.current !== null) cancelAnimationFrame(animFrame.current)
      animFrame.current = requestAnimationFrame(() => setWidth(lastNext))
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
      setWidth(lastNext)
      requestAnimationFrame(() => {
        requestAnimationFrame(() => {
          document.dispatchEvent(new CustomEvent('browser-panel-drag-end'))
        })
      })
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
