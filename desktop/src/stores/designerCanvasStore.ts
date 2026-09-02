// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { create } from 'zustand'
import { designerApi } from '../api/designer'

export type DesignSurface = 'html' | 'image' | 'video' | 'audio' | 'deck' | 'diagram' | 'other'

export type UnitDevice = 'auto' | 'desktop' | 'tablet' | 'mobile'

export type DesignUnit = {
  id: string
  relPath: string
  submode: string | null
  surface: DesignSurface
  x: number
  y: number
  width: number
  height: number
  title: string | null
  customName: string | null
  device: UnitDevice
}

function humanizeRelPath(relPath: string): string {
  const base = relPath.split('/').pop() ?? relPath
  const stem = base.replace(/\.[^.]+$/, '')
  const words = stem
    .replace(/[-_]+/g, ' ')
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .trim()
  if (!words) return base
  return words.replace(/\b\w/g, (c) => c.toUpperCase())
}

export function unitDisplayName(unit: DesignUnit): string {
  const custom = unit.customName?.trim()
  if (custom) return custom
  const title = unit.title?.trim()
  if (title) return title
  return humanizeRelPath(unit.relPath)
}

export type CanvasViewport = {
  panX: number
  panY: number
  zoom: number
}

export type CanvasTweaks = {
  accent: string | null
  scale: number
  density: number
  motion: number
  mode: 'auto' | 'light' | 'dark'
}

export const DEFAULT_TWEAKS: CanvasTweaks = {
  accent: null,
  scale: 1,
  density: 1,
  motion: 1,
  mode: 'auto',
}

export const DEVICE_PRESETS: Record<Exclude<UnitDevice, 'auto'>, { w: number; label: string }> = {
  desktop: { w: 1440, label: 'Desktop' },
  tablet: { w: 834, label: 'Tablet' },
  mobile: { w: 390, label: 'Mobile' },
}

export type DesignerCanvasPanelState = {
  visible: boolean
  columnWidth: number
  viewport: CanvasViewport
  units: DesignUnit[]
  selectedUnitId: string | null
  selectMode: boolean
  tweaks: CanvasTweaks
  loaded: boolean
  loading: boolean
}

export const DESIGNER_CANVAS_WIDTH_BOUNDS = {
  min: 360,
  max: 1400,
  default: 640,
} as const

export const CANVAS_ZOOM_BOUNDS = {
  min: 0.05,
  max: 3,
} as const

const UNIT_W = 360
const UNIT_H = 300
const GAP_X = 48
const GAP_Y = 64
const GRID_COLS = 4
const PADDING = 48

const IMAGE_EXTS = new Set(['png', 'jpg', 'jpeg', 'webp', 'gif', 'avif', 'bmp'])
const VIDEO_EXTS = new Set(['mp4', 'webm', 'mov', 'm4v'])
const AUDIO_EXTS = new Set(['mp3', 'wav', 'ogg', 'm4a', 'aac', 'flac'])
const HTML_EXTS = new Set(['html', 'htm', 'svg'])

function extOf(path: string): string {
  const idx = path.lastIndexOf('.')
  return idx === -1 ? '' : path.slice(idx + 1).toLowerCase()
}

function baseNameOf(path: string): string {
  const idx = path.lastIndexOf('/')
  return idx === -1 ? path : path.slice(idx + 1)
}

export function surfaceForPath(path: string): DesignSurface {
  const base = baseNameOf(path).toLowerCase()
  if (base === 'deck.json') return 'deck'
  if (base.endsWith('.mmd') || base.endsWith('.echarts.json') || base.endsWith('.mindmap.md')) {
    return 'diagram'
  }
  const ext = extOf(path)
  if (HTML_EXTS.has(ext)) return 'html'
  if (IMAGE_EXTS.has(ext)) return 'image'
  if (VIDEO_EXTS.has(ext)) return 'video'
  if (AUDIO_EXTS.has(ext)) return 'audio'
  return 'other'
}

export function deckManifestForPath(path: string): string | null {
  const normalized = path.replace(/\\/g, '/')
  const base = baseNameOf(normalized).toLowerCase()
  if (base === 'deck.json') return normalized
  if (base === 'render.json') {
    const dir = normalized.slice(0, normalized.length - 'render.json'.length)
    return `${dir}deck.json`
  }
  const slidesMatch = normalized.match(/^(.*)\/slides\/[^/]+\.json$/)
  if (slidesMatch) {
    return `${slidesMatch[1]}/deck.json`
  }
  return null
}

function isRelativeWorkspacePath(path: string): boolean {
  if (!path) return false
  if (path.startsWith('/') || path.startsWith('\\')) return false
  if (/^[a-zA-Z]:[/\\]/.test(path)) return false
  if (path.includes(':')) return false
  return true
}

export function isDesignArtifactPath(path: string): boolean {
  const normalized = path.replace(/\\/g, '/')
  if (normalized.startsWith('masks/') || normalized.includes('/masks/')) return false
  return isRelativeWorkspacePath(path) && surfaceForPath(path) !== 'other'
}

export function designerSessionDir(sessionId: string): string {
  const sanitized = (sessionId ?? '')
    .replace(/[^a-zA-Z0-9_-]/g, '-')
    .replace(/^-+|-+$/g, '')
  const id = sanitized.length ? sanitized : 'default'
  return `.senweavercoding/designer/${id}`
}

export function isInDesignerSessionDir(relPath: string, sessionId: string): boolean {
  const dir = `${designerSessionDir(sessionId)}/`
  return relPath.startsWith(dir)
}

const COLUMN_WIDTH_STORAGE_PREFIX = 'sen-designer-canvas-width:'
const LAYOUT_STORAGE_PREFIX = 'sen-designer-canvas-layout:'
const DEFAULT_WORKSPACE_KEY = '__default__'

function normalizeWorkspaceKey(key: string | null | undefined): string {
  if (!key) return DEFAULT_WORKSPACE_KEY
  const trimmed = key.trim()
  return trimmed.length ? trimmed : DEFAULT_WORKSPACE_KEY
}

function currentWorkspaceKey(): string {
  if (typeof window === 'undefined') return DEFAULT_WORKSPACE_KEY
  try {
    const w = window as unknown as { __sen_active_workspace_key__?: string | null }
    return normalizeWorkspaceKey(w.__sen_active_workspace_key__)
  } catch {
    return DEFAULT_WORKSPACE_KEY
  }
}

export function clampCanvasWidth(value: number): number {
  if (!Number.isFinite(value)) return DESIGNER_CANVAS_WIDTH_BOUNDS.default
  return Math.min(
    DESIGNER_CANVAS_WIDTH_BOUNDS.max,
    Math.max(DESIGNER_CANVAS_WIDTH_BOUNDS.min, Math.round(value)),
  )
}

export function clampZoom(value: number): number {
  if (!Number.isFinite(value)) return 1
  return Math.min(CANVAS_ZOOM_BOUNDS.max, Math.max(CANVAS_ZOOM_BOUNDS.min, value))
}

function readStoredColumnWidth(workspaceKey: string): number {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') {
    return DESIGNER_CANVAS_WIDTH_BOUNDS.default
  }
  try {
    const raw = localStorage.getItem(
      `${COLUMN_WIDTH_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}`,
    )
    if (!raw) return DESIGNER_CANVAS_WIDTH_BOUNDS.default
    const value = Number.parseInt(raw, 10)
    if (Number.isFinite(value)) return clampCanvasWidth(value)
  } catch {
  }
  return DESIGNER_CANVAS_WIDTH_BOUNDS.default
}

function writeStoredColumnWidth(workspaceKey: string, value: number) {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(
      `${COLUMN_WIDTH_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}`,
      String(clampCanvasWidth(value)),
    )
  } catch {
  }
}

type StoredLayout = Record<
  string,
  { x: number; y: number; width: number; height: number; name?: string; device?: UnitDevice }
>

const TWEAKS_STORAGE_PREFIX = 'sen-designer-canvas-tweaks:'

function tweaksStorageKey(workspaceKey: string, sessionId: string): string {
  return `${TWEAKS_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}:${sessionId}`
}

function readStoredTweaks(sessionId: string): CanvasTweaks {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return { ...DEFAULT_TWEAKS }
  try {
    const raw = localStorage.getItem(tweaksStorageKey(currentWorkspaceKey(), sessionId))
    if (!raw) return { ...DEFAULT_TWEAKS }
    const parsed = JSON.parse(raw) as Partial<CanvasTweaks>
    return { ...DEFAULT_TWEAKS, ...parsed }
  } catch {
    return { ...DEFAULT_TWEAKS }
  }
}

function writeStoredTweaks(sessionId: string, tweaks: CanvasTweaks) {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(
      tweaksStorageKey(currentWorkspaceKey(), sessionId),
      JSON.stringify(tweaks),
    )
  } catch {
  }
}

function layoutStorageKey(workspaceKey: string, sessionId: string): string {
  return `${LAYOUT_STORAGE_PREFIX}${normalizeWorkspaceKey(workspaceKey)}:${sessionId}`
}

function readStoredLayout(sessionId: string): StoredLayout {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return {}
  try {
    const raw = localStorage.getItem(layoutStorageKey(currentWorkspaceKey(), sessionId))
    if (!raw) return {}
    const parsed = JSON.parse(raw) as StoredLayout
    return parsed && typeof parsed === 'object' ? parsed : {}
  } catch {
    return {}
  }
}

function writeStoredLayout(sessionId: string, units: DesignUnit[]) {
  if (typeof window === 'undefined' || typeof localStorage === 'undefined') return
  try {
    const layout: StoredLayout = {}
    for (const u of units) {
      layout[u.relPath] = {
        x: u.x,
        y: u.y,
        width: u.width,
        height: u.height,
        ...(u.customName ? { name: u.customName } : {}),
        ...(u.device && u.device !== 'auto' ? { device: u.device } : {}),
      }
    }
    localStorage.setItem(
      layoutStorageKey(currentWorkspaceKey(), sessionId),
      JSON.stringify(layout),
    )
  } catch {
  }
}

function gridPosition(index: number): { x: number; y: number } {
  const col = index % GRID_COLS
  const row = Math.floor(index / GRID_COLS)
  return {
    x: PADDING + col * (UNIT_W + GAP_X),
    y: PADDING + row * (UNIT_H + GAP_Y),
  }
}

function buildUnit(
  relPath: string,
  index: number,
  submode: string | null,
  layout: StoredLayout,
): DesignUnit {
  const stored = layout[relPath]
  const pos = stored ?? gridPosition(index)
  return {
    id: relPath,
    relPath,
    submode,
    surface: surfaceForPath(relPath),
    x: pos.x,
    y: pos.y,
    width: stored?.width ?? UNIT_W,
    height: stored?.height ?? UNIT_H,
    title: null,
    customName: stored?.name ?? null,
    device: stored?.device ?? 'auto',
  }
}

const DEFAULT_STATE: DesignerCanvasPanelState = {
  visible: false,
  columnWidth: DESIGNER_CANVAS_WIDTH_BOUNDS.default,
  viewport: { panX: 0, panY: 0, zoom: 0.6 },
  units: [],
  selectedUnitId: null,
  selectMode: false,
  tweaks: { ...DEFAULT_TWEAKS },
  loaded: false,
  loading: false,
}

type StoreState = {
  panels: Record<string, DesignerCanvasPanelState>
  ensure: (sessionId: string) => DesignerCanvasPanelState
  toggle: (sessionId: string) => void
  setVisible: (sessionId: string, visible: boolean) => void
  setColumnWidth: (sessionId: string, px: number) => void
  setViewport: (sessionId: string, patch: Partial<CanvasViewport>) => void
  selectUnit: (sessionId: string, unitId: string | null) => void
  updateUnitLayout: (
    sessionId: string,
    unitId: string,
    patch: Partial<Pick<DesignUnit, 'x' | 'y' | 'width' | 'height'>>,
  ) => void
  autoFitUnit: (sessionId: string, unitId: string, naturalW: number, naturalH: number) => void
  setUnitDevice: (sessionId: string, unitId: string, device: UnitDevice) => void
  setSelectMode: (sessionId: string, on: boolean) => void
  setTweaks: (sessionId: string, patch: Partial<CanvasTweaks>) => void
  resetTweaks: (sessionId: string) => void
  focusUnit: (
    sessionId: string,
    unitId: string,
    viewportSize?: { width: number; height: number },
  ) => void
  setUnitTitle: (sessionId: string, unitId: string, title: string) => void
  renameUnit: (sessionId: string, unitId: string, name: string) => void
  addArtifactLive: (sessionId: string, relPath: string, submode?: string | null) => void
  removeUnit: (sessionId: string, unitId: string) => Promise<boolean>
  removeUnitLocal: (sessionId: string, relPath: string) => void
  migrateUnitPath: (sessionId: string, fromRelPath: string, toRelPath: string) => void
  loadHistory: (sessionId: string) => Promise<boolean>
  reset: (sessionId: string) => void
}

function patchPanel(
  panels: Record<string, DesignerCanvasPanelState>,
  sessionId: string,
  patch: Partial<DesignerCanvasPanelState>,
): Record<string, DesignerCanvasPanelState> {
  const prev = panels[sessionId] ?? DEFAULT_STATE
  return { ...panels, [sessionId]: { ...prev, ...patch } }
}

export const useDesignerCanvasStore = create<StoreState>((set, get) => ({
  panels: {},

  ensure: (sessionId) => {
    const existing = get().panels[sessionId]
    if (existing) return existing
    const next: DesignerCanvasPanelState = {
      ...DEFAULT_STATE,
      columnWidth: readStoredColumnWidth(currentWorkspaceKey()),
      tweaks: readStoredTweaks(sessionId),
    }
    set((state) => ({ panels: { ...state.panels, [sessionId]: next } }))
    return next
  },

  toggle: (sessionId) => {
    const cur = get().panels[sessionId] ?? get().ensure(sessionId)
    const wantsVisible = !cur.visible
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { visible: wantsVisible }) }))
    if (wantsVisible && !cur.loading) {
      void get().loadHistory(sessionId)
    }
  },

  setVisible: (sessionId, visible) => {
    const prev = get().panels[sessionId] ?? get().ensure(sessionId)
    const becameVisible = visible && !prev.visible
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { visible }) }))
    if (becameVisible && !prev.loading) {
      void get().loadHistory(sessionId)
    }
  },

  setColumnWidth: (sessionId, px) => {
    const next = clampCanvasWidth(px)
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { columnWidth: next }) }))
    writeStoredColumnWidth(currentWorkspaceKey(), next)
  },

  setViewport: (sessionId, patch) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const zoom = patch.zoom !== undefined ? clampZoom(patch.zoom) : prev.viewport.zoom
      return {
        panels: patchPanel(state.panels, sessionId, {
          viewport: { ...prev.viewport, ...patch, zoom },
        }),
      }
    }),

  selectUnit: (sessionId, unitId) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { selectedUnitId: unitId }) })),

  updateUnitLayout: (sessionId, unitId, patch) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const units = prev.units.map((u) => (u.id === unitId ? { ...u, ...patch } : u))
      writeStoredLayout(sessionId, units)
      return { panels: patchPanel(state.panels, sessionId, { units }) }
    }),

  autoFitUnit: (sessionId, unitId, naturalW, naturalH) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const unit = prev.units.find((u) => u.id === unitId)
      if (!unit || unit.surface !== 'html') return {}
      if ((unit.device ?? 'auto') !== 'auto') return {}
      if (unit.submode === 'live-artifact') {
        const height = Math.min(760, Math.max(220, Math.round((unit.width * 1080) / 1920) + 28))
        if (height === unit.height) return {}
        const units = prev.units.map((u) => (u.id === unitId ? { ...u, height } : u))
        writeStoredLayout(sessionId, units)
        return { panels: patchPanel(state.panels, sessionId, { units }) }
      }
      if (unit.width !== UNIT_W || unit.height !== UNIT_H) return {}
      if (!Number.isFinite(naturalW) || !Number.isFinite(naturalH)) return {}
      if (naturalW <= 0 || naturalH <= 0) return {}
      const height = Math.min(
        760,
        Math.max(220, Math.round((UNIT_W * naturalH) / naturalW) + 28),
      )
      if (height === unit.height) return {}
      const units = prev.units.map((u) => (u.id === unitId ? { ...u, height } : u))
      writeStoredLayout(sessionId, units)
      return { panels: patchPanel(state.panels, sessionId, { units }) }
    }),

  setUnitDevice: (sessionId, unitId, device) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const units = prev.units.map((u) => (u.id === unitId ? { ...u, device } : u))
      writeStoredLayout(sessionId, units)
      return { panels: patchPanel(state.panels, sessionId, { units }) }
    }),

  setSelectMode: (sessionId, on) =>
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { selectMode: on }) })),

  setTweaks: (sessionId, patch) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const tweaks = { ...prev.tweaks, ...patch }
      writeStoredTweaks(sessionId, tweaks)
      return { panels: patchPanel(state.panels, sessionId, { tweaks }) }
    }),

  resetTweaks: (sessionId) =>
    set((state) => {
      const tweaks = { ...DEFAULT_TWEAKS }
      writeStoredTweaks(sessionId, tweaks)
      return { panels: patchPanel(state.panels, sessionId, { tweaks }) }
    }),

  focusUnit: (sessionId, unitId, viewportSize) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const unit = prev.units.find((u) => u.id === unitId)
      if (!unit) return {}
      const zoom = clampZoom(Math.max(prev.viewport.zoom, 0.6))
      const panX = viewportSize
        ? viewportSize.width / 2 - (unit.x + unit.width / 2) * zoom
        : PADDING - unit.x * zoom + 24
      const panY = viewportSize
        ? viewportSize.height / 2 - (unit.y + unit.height / 2) * zoom
        : PADDING - unit.y * zoom + 24
      return {
        panels: patchPanel(state.panels, sessionId, {
          selectedUnitId: unitId,
          viewport: { panX, panY, zoom },
        }),
      }
    }),

  setUnitTitle: (sessionId, unitId, title) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const trimmed = title.trim()
      let changed = false
      const units = prev.units.map((u) => {
        if (u.id === unitId && trimmed && u.title !== trimmed) {
          changed = true
          return { ...u, title: trimmed }
        }
        return u
      })
      if (!changed) return {}
      return { panels: patchPanel(state.panels, sessionId, { units }) }
    }),

  renameUnit: (sessionId, unitId, name) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const trimmed = name.trim()
      const customName = trimmed.length ? trimmed : null
      const units = prev.units.map((u) =>
        u.id === unitId ? { ...u, customName } : u,
      )
      writeStoredLayout(sessionId, units)
      return { panels: patchPanel(state.panels, sessionId, { units }) }
    }),

  addArtifactLive: (sessionId, relPath, submode) => {
    if (!isDesignArtifactPath(relPath)) return
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      if (prev.units.some((u) => u.relPath === relPath)) {
        return {
          panels: patchPanel(state.panels, sessionId, { selectedUnitId: relPath }),
        }
      }
      const layout = readStoredLayout(sessionId)
      const unit = buildUnit(relPath, prev.units.length, submode ?? null, layout)
      const units = [...prev.units, unit]
      writeStoredLayout(sessionId, units)
      return {
        panels: patchPanel(state.panels, sessionId, {
          units,
          selectedUnitId: relPath,
        }),
      }
    })
  },

  removeUnit: async (sessionId, unitId) => {
    const panel = get().panels[sessionId]
    const unit = panel?.units.find((u) => u.id === unitId)
    if (!unit) return true
    try {
      const res = await designerApi.deleteArtifact(sessionId, unit.relPath)
      if (!res.ok) return false
    } catch {
      return false
    }
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      if (!prev.units.some((u) => u.id === unitId)) return {}
      const units = prev.units.filter((u) => u.id !== unitId)
      writeStoredLayout(sessionId, units)
      return {
        panels: patchPanel(state.panels, sessionId, {
          units,
          selectedUnitId:
            prev.selectedUnitId === unitId ? null : prev.selectedUnitId,
        }),
      }
    })
    return true
  },

  removeUnitLocal: (sessionId, relPath) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      if (!prev.units.some((u) => u.relPath === relPath)) return {}
      const units = prev.units.filter((u) => u.relPath !== relPath)
      writeStoredLayout(sessionId, units)
      return {
        panels: patchPanel(state.panels, sessionId, {
          units,
          selectedUnitId:
            prev.selectedUnitId === relPath ? null : prev.selectedUnitId,
        }),
      }
    }),

  migrateUnitPath: (sessionId, fromRelPath, toRelPath) =>
    set((state) => {
      const prev = state.panels[sessionId] ?? DEFAULT_STATE
      const existing = prev.units.find((u) => u.relPath === fromRelPath)
      if (!existing || prev.units.some((u) => u.relPath === toRelPath)) return {}
      const units = prev.units.map((u) =>
        u.relPath === fromRelPath
          ? {
              ...u,
              id: toRelPath,
              relPath: toRelPath,
              surface: surfaceForPath(toRelPath),
            }
          : u,
      )
      writeStoredLayout(sessionId, units)
      return {
        panels: patchPanel(state.panels, sessionId, {
          units,
          selectedUnitId:
            prev.selectedUnitId === fromRelPath ? toRelPath : prev.selectedUnitId,
        }),
      }
    }),

  loadHistory: async (sessionId) => {
    get().ensure(sessionId)
    set((state) => ({ panels: patchPanel(state.panels, sessionId, { loading: true }) }))
    try {
      const res = await designerApi.designArtifacts(sessionId)
      const layout = readStoredLayout(sessionId)
      const records = (res.artifacts ?? []).filter((r) => isDesignArtifactPath(r.relPath))
      const units = records.map((r, i) => buildUnit(r.relPath, i, r.submode, layout))
      set((state) => {
        const prev = state.panels[sessionId] ?? DEFAULT_STATE
        const known = new Set(units.map((u) => u.relPath))
        const merged = units.map((u) => {
          const live = prev.units.find((p) => p.relPath === u.relPath)
          return live ? { ...u, title: live.title, device: live.device } : u
        })
        const selected =
          prev.selectedUnitId && known.has(prev.selectedUnitId)
            ? prev.selectedUnitId
            : null
        return {
          panels: patchPanel(state.panels, sessionId, {
            units: merged,
            selectedUnitId: selected,
            loaded: true,
            loading: false,
          }),
        }
      })
      return true
    } catch {
      set((state) => ({
        panels: patchPanel(state.panels, sessionId, { loaded: true, loading: false }),
      }))
      return false
    }
  },

  reset: (sessionId) =>
    set((state) => {
      const next = { ...state.panels }
      delete next[sessionId]
      return { panels: next }
    }),
}))
