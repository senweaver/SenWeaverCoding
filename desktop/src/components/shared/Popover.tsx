import {
  cloneElement,
  isValidElement,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
  type ReactElement,
  type ReactNode,
} from 'react'
import { createPortal } from 'react-dom'

type Placement = 'top' | 'bottom' | 'auto'
type Trigger = 'hover' | 'click'

type PopoverProps = {

  children: ReactElement

  content: ReactNode
  placement?: Placement
  trigger?: Trigger

  minWidth?: number

  maxWidth?: number

  panelClassName?: string

  disabled?: boolean
}

const HOVER_OPEN_DELAY = 90
const HOVER_CLOSE_DELAY = 110
const VIEWPORT_MARGIN = 8

export function Popover({
  children,
  content,
  placement = 'auto',
  trigger = 'hover',
  minWidth = 240,
  maxWidth = 420,
  panelClassName = '',
  disabled = false,
}: PopoverProps) {
  const triggerRef = useRef<HTMLElement | null>(null)
  const surfaceRef = useRef<HTMLDivElement | null>(null)
  const openTimer = useRef<number | null>(null)
  const closeTimer = useRef<number | null>(null)
  const [open, setOpen] = useState(false)
  const [style, setStyle] = useState<CSSProperties>({ visibility: 'hidden' })
  const id = useId()

  const clearTimers = useCallback(() => {
    if (openTimer.current != null) {
      window.clearTimeout(openTimer.current)
      openTimer.current = null
    }
    if (closeTimer.current != null) {
      window.clearTimeout(closeTimer.current)
      closeTimer.current = null
    }
  }, [])

  useEffect(() => () => clearTimers(), [clearTimers])

  const measure = useCallback(() => {
    const trig = triggerRef.current
    const surf = surfaceRef.current
    if (!trig || !surf) return
    const rect = trig.getBoundingClientRect()
    const surfRect = surf.getBoundingClientRect()

    const spaceAbove = rect.top
    const spaceBelow = window.innerHeight - rect.bottom
    let useTop: boolean
    if (placement === 'top') useTop = true
    else if (placement === 'bottom') useTop = false
    else useTop = spaceBelow < surfRect.height + VIEWPORT_MARGIN && spaceAbove > spaceBelow

    const top = useTop
      ? Math.max(VIEWPORT_MARGIN, rect.top - surfRect.height - 6)
      : Math.min(window.innerHeight - surfRect.height - VIEWPORT_MARGIN, rect.bottom + 6)

    const desiredLeft = rect.left
    const maxLeft = window.innerWidth - surfRect.width - VIEWPORT_MARGIN
    const left = Math.max(VIEWPORT_MARGIN, Math.min(desiredLeft, maxLeft))

    setStyle({
      position: 'fixed',
      top,
      left,
      minWidth,
      maxWidth,
      zIndex: 9999,
    })
  }, [maxWidth, minWidth, placement])

  useLayoutEffect(() => {
    if (!open) return
    measure()
    const onScroll = () => measure()
    const onResize = () => measure()
    window.addEventListener('scroll', onScroll, true)
    window.addEventListener('resize', onResize)
    return () => {
      window.removeEventListener('scroll', onScroll, true)
      window.removeEventListener('resize', onResize)
    }
  }, [open, measure, content])

  useEffect(() => {
    if (!open || trigger !== 'click') return
    const onDocClick = (event: MouseEvent) => {
      const target = event.target as Node | null
      if (!target) return
      if (triggerRef.current?.contains(target)) return
      if (surfaceRef.current?.contains(target)) return
      setOpen(false)
    }
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onDocClick)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDocClick)
      document.removeEventListener('keydown', onKey)
    }
  }, [open, trigger])

  const handleHoverEnter = useCallback(() => {
    if (disabled) return
    clearTimers()
    openTimer.current = window.setTimeout(() => setOpen(true), HOVER_OPEN_DELAY)
  }, [clearTimers, disabled])

  const handleHoverLeave = useCallback(() => {
    if (disabled) return
    clearTimers()
    closeTimer.current = window.setTimeout(() => setOpen(false), HOVER_CLOSE_DELAY)
  }, [clearTimers, disabled])

  const handleClick = useCallback(
    (event: ReactMouseEvent<HTMLElement>) => {
      if (disabled) return
      event.stopPropagation()
      setOpen((v) => !v)
    },
    [disabled],
  )

  const childRefRef = useRef<unknown>(null)
  childRefRef.current = isValidElement(children)
    ? (children as { ref?: unknown }).ref ?? null
    : null

  const setTriggerRef = useCallback((el: HTMLElement | null) => {
    triggerRef.current = el
    const ref = childRefRef.current
    if (typeof ref === 'function') {
      ;(ref as (instance: HTMLElement | null) => void)(el)
    } else if (ref && typeof ref === 'object' && 'current' in (ref as object)) {
      ;(ref as { current: HTMLElement | null }).current = el
    }
  }, [])

  if (!isValidElement(children)) {
    return children as unknown as ReactElement
  }

  const triggerProps: Record<string, unknown> = {
    ref: setTriggerRef,
    'aria-describedby': open ? id : undefined,
  }

  if (trigger === 'hover') {
    triggerProps.onMouseEnter = handleHoverEnter
    triggerProps.onMouseLeave = handleHoverLeave
    triggerProps.onFocus = handleHoverEnter
    triggerProps.onBlur = handleHoverLeave
  } else {
    triggerProps.onClick = handleClick
  }

  const panel = open
    ? createPortal(
        <div
          ref={surfaceRef}
          id={id}
          role={trigger === 'click' ? 'dialog' : 'tooltip'}
          style={style}
          className={`pointer-events-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] text-[12px] text-[var(--color-text-primary)] shadow-[0_18px_40px_-18px_rgba(0,0,0,0.45)] ${
            panelClassName
          }`}
          onMouseEnter={trigger === 'hover' ? handleHoverEnter : undefined}
          onMouseLeave={trigger === 'hover' ? handleHoverLeave : undefined}
        >
          {content}
        </div>,
        document.body,
      )
    : null

  return (
    <>
      {cloneElement(children, triggerProps)}
      {panel}
    </>
  )
}
