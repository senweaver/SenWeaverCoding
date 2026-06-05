// SPDX-License-Identifier: MIT

import { create } from 'zustand'

export type BuddyMood =
  | 'happy'
  | 'thinking'
  | 'working'
  | 'celebrating'
  | 'confused'
  | 'sleeping'
  | 'error'
  | 'neutral'

export const BUDDY_MOOD_EMOJI: Record<BuddyMood, string> = {
  happy: '\u{1F60A}',
  thinking: '\u{1F914}',
  working: '\u2699\uFE0F',
  celebrating: '\u{1F389}',
  confused: '\u{1F615}',
  sleeping: '\u{1F4A4}',
  error: '\u{1F635}',
  neutral: '\u{1F916}',
}

type BuddyEvent =
  | { type: 'mood_changed'; mood: string }
  | { type: 'notification'; message: string }
  | { type: 'tip'; tip: string }

interface BuddyState {
  enabled: boolean
  mood: BuddyMood
  greeting: string
  lastUpdated: number
  applyEvent: (event: BuddyEvent, greeting?: string, showNotifications?: boolean) => void
}

function normalizeMood(mood: string): BuddyMood {
  return (mood in BUDDY_MOOD_EMOJI ? mood : 'neutral') as BuddyMood
}

export const useBuddyStore = create<BuddyState>((set) => ({
  enabled: false,
  mood: 'neutral',
  greeting: '',
  lastUpdated: 0,
  applyEvent: (event, greeting, showNotifications) => {
    if (event.type === 'mood_changed') {
      set({
        enabled: true,
        mood: normalizeMood(event.mood),
        greeting: greeting ?? '',
        lastUpdated: Date.now(),
      })
      return
    }
    if (event.type === 'notification') {
      set({ enabled: true, greeting: event.message, lastUpdated: Date.now() })
      if (showNotifications) {
        void import('./uiStore')
          .then(({ useUIStore }) => {
            useUIStore.getState().addToast({
              type: 'info',
              message: event.message,
              duration: 4000,
            })
          })
          .catch(() => {})
      }
      return
    }
    if (event.type === 'tip') {
      set({ enabled: true, greeting: event.tip, lastUpdated: Date.now() })
    }
  },
}))
