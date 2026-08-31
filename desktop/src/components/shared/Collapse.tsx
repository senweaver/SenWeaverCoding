// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState, type ReactNode } from 'react'

type Props = {
  open: boolean
  children: ReactNode
  durationMs?: number
}

export function Collapse({ open, children, durationMs = 180 }: Props) {
  const [mounted, setMounted] = useState(open)
  const [expanded, setExpanded] = useState(open)

  useEffect(() => {
    if (open) {
      setMounted(true)
      const raf = requestAnimationFrame(() => setExpanded(true))
      return () => cancelAnimationFrame(raf)
    }
    setExpanded(false)
    const timer = window.setTimeout(() => setMounted(false), durationMs)
    return () => window.clearTimeout(timer)
  }, [open, durationMs])

  if (!mounted && !open) return null

  return (
    <div
      style={{
        display: 'grid',
        gridTemplateRows: expanded ? '1fr' : '0fr',
        opacity: expanded ? 1 : 0,
        pointerEvents: expanded ? undefined : 'none',
        transition: `grid-template-rows ${durationMs}ms ease, opacity ${durationMs}ms ease`,
      }}
      aria-hidden={expanded ? undefined : true}
    >
      <div style={{ overflow: 'hidden', minHeight: 0 }}>{children}</div>
    </div>
  )
}
