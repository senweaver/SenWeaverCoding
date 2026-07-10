// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import type { ComputerStatus } from './computerUseStore'
import type { MinimalComputerProgress } from '../lib/minimalMode'

type MinimalComputerStore = {
  status: ComputerStatus
  statusMessage: string | null
  error: string | null
  lastThought: string | null
  lastAction: string | null
  stepCount: number
  applyProgress: (progress: MinimalComputerProgress) => void
  reset: () => void
}

const initial = {
  status: 'idle' as ComputerStatus,
  statusMessage: null,
  error: null,
  lastThought: null,
  lastAction: null,
  stepCount: 0,
}

export const useMinimalComputerStore = create<MinimalComputerStore>((set) => ({
  ...initial,
  applyProgress: (progress) =>
    set({
      status: progress.status,
      statusMessage: progress.statusMessage,
      error: progress.error,
      lastThought: progress.lastThought,
      lastAction: progress.lastAction,
      stepCount: progress.stepCount,
    }),
  reset: () => set({ ...initial }),
}))

export function isComputerBusy(status: ComputerStatus): boolean {
  return status === 'running' || status === 'thinking' || status === 'connecting'
}
