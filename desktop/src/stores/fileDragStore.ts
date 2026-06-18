// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import type { FileRefDragPayload } from '../components/chat/composerRefs'

export type FileDropZone = {
  id: string
  getRect: () => DOMRect | null
  onDrop: (payload: FileRefDragPayload, x: number, y: number) => void
}

type FileDragState = {
  payload: FileRefDragPayload | null
  pointer: { x: number; y: number } | null
  activeZoneId: string | null
  zones: FileDropZone[]
  registerZone: (zone: FileDropZone) => void
  unregisterZone: (id: string) => void
  begin: (payload: FileRefDragPayload, x: number, y: number) => void
  move: (x: number, y: number) => void
  finish: () => void
  cancel: () => void
}

function rectContains(rect: DOMRect | null, x: number, y: number): boolean {
  if (!rect) return false
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

export const useFileDragStore = create<FileDragState>((set, get) => ({
  payload: null,
  pointer: null,
  activeZoneId: null,
  zones: [],
  registerZone: (zone) =>
    set((state) => ({ zones: [...state.zones.filter((z) => z.id !== zone.id), zone] })),
  unregisterZone: (id) =>
    set((state) => ({ zones: state.zones.filter((z) => z.id !== id) })),
  begin: (payload, x, y) => set({ payload, pointer: { x, y }, activeZoneId: null }),
  move: (x, y) => {
    const { payload, zones } = get()
    if (!payload) return
    const zone = zones.find((z) => rectContains(z.getRect(), x, y))
    set({ pointer: { x, y }, activeZoneId: zone?.id ?? null })
  },
  finish: () => {
    const { payload, pointer, zones } = get()
    if (payload && pointer) {
      const zone = zones.find((z) => rectContains(z.getRect(), pointer.x, pointer.y))
      if (zone) zone.onDrop(payload, pointer.x, pointer.y)
    }
    set({ payload: null, pointer: null, activeZoneId: null })
  },
  cancel: () => set({ payload: null, pointer: null, activeZoneId: null }),
}))
