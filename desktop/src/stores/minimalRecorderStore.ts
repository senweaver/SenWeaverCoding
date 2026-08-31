// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import type { RecorderStatus } from './computerRecorderStore'
import type { MinimalRecorderProgress } from '../lib/minimalMode'

type MinimalRecorderStore = {
  status: RecorderStatus
  error: string | null
  statusMessage: string | null
  stepCount: number
  lastActionType: string | null
  lastActionValue: string | null
  savedRecordingName: string | null
  savedSkillName: string | null
  startedAt: number | null
  narrationEnabled: boolean
  narrationMuted: boolean
  applyProgress: (progress: MinimalRecorderProgress) => void
  reset: () => void
}

const initial = {
  status: 'idle' as RecorderStatus,
  error: null,
  statusMessage: null,
  stepCount: 0,
  lastActionType: null,
  lastActionValue: null,
  savedRecordingName: null,
  savedSkillName: null,
  startedAt: null,
  narrationEnabled: false,
  narrationMuted: false,
}

export const useMinimalRecorderStore = create<MinimalRecorderStore>((set) => ({
  ...initial,
  applyProgress: (progress) =>
    set({
      status: progress.status,
      error: progress.error,
      statusMessage: progress.statusMessage,
      stepCount: progress.stepCount,
      lastActionType: progress.lastActionType,
      lastActionValue: progress.lastActionValue,
      savedRecordingName: progress.savedRecordingName,
      savedSkillName: progress.savedSkillName,
      startedAt: progress.startedAt,
      narrationEnabled: progress.narrationEnabled ?? false,
      narrationMuted: progress.narrationMuted ?? false,
    }),
  reset: () => set({ ...initial }),
}))
