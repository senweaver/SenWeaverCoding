// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import type { MinimalVariant } from '../lib/minimalMode'

const OPACITY_STORAGE_KEY = 'sen-minimal-opacity'
const OPACITY_MIN = 40
const OPACITY_MAX = 100

function clampOpacity(value: number): number {
  if (!Number.isFinite(value)) return OPACITY_MAX
  return Math.min(OPACITY_MAX, Math.max(OPACITY_MIN, Math.round(value)))
}

function readStoredOpacity(): number {
  try {
    const raw = localStorage.getItem(OPACITY_STORAGE_KEY)
    if (raw) return clampOpacity(Number.parseInt(raw, 10))
  } catch {

  }
  return OPACITY_MAX
}

type MinimalStore = {
  opacityPct: number
  variant: MinimalVariant
  setOpacityPct: (pct: number) => void
  setVariant: (variant: MinimalVariant) => void
}

export const MINIMAL_OPACITY_BOUNDS = { min: OPACITY_MIN, max: OPACITY_MAX }

export const useMinimalStore = create<MinimalStore>((set) => ({
  opacityPct: readStoredOpacity(),
  variant: 'code',
  setOpacityPct: (pct) => {
    const clamped = clampOpacity(pct)
    try {
      localStorage.setItem(OPACITY_STORAGE_KEY, String(clamped))
    } catch {

    }
    set({ opacityPct: clamped })
  },
  setVariant: (variant) => set({ variant }),
}))
