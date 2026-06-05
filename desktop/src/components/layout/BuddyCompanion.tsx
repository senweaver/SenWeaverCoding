// SPDX-License-Identifier: MIT

import { useEffect, useState } from 'react'
import { useBuddyStore, BUDDY_MOOD_EMOJI } from '../../stores/buddyStore'

const VISIBLE_MS = 6000

export function BuddyCompanion() {
  const enabled = useBuddyStore((s) => s.enabled)
  const mood = useBuddyStore((s) => s.mood)
  const greeting = useBuddyStore((s) => s.greeting)
  const lastUpdated = useBuddyStore((s) => s.lastUpdated)
  const [expanded, setExpanded] = useState(false)

  useEffect(() => {
    if (!lastUpdated) return
    setExpanded(true)
    const id = window.setTimeout(() => setExpanded(false), VISIBLE_MS)
    return () => window.clearTimeout(id)
  }, [lastUpdated])

  if (!enabled) return null

  const animated = mood === 'thinking' || mood === 'working'

  return (
    <div className="pointer-events-none fixed bottom-16 right-4 z-40 flex items-end gap-2">
      {expanded && greeting && (
        <div className="pointer-events-auto max-w-[240px] rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container)] px-3 py-2 text-xs text-[var(--color-text-secondary)] shadow-lg">
          {greeting}
        </div>
      )}
      <button
        type="button"
        title={greeting || mood}
        onClick={() => setExpanded((v) => !v)}
        data-mood={mood}
        className={`pointer-events-auto flex h-10 w-10 items-center justify-center rounded-full border border-[var(--color-border)] bg-[var(--color-surface-container)] text-xl shadow-lg transition-transform hover:scale-105 ${
          animated ? 'animate-pulse' : ''
        }`}
      >
        <span aria-hidden>{BUDDY_MOOD_EMOJI[mood]}</span>
      </button>
    </div>
  )
}
