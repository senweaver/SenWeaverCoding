// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from '../../i18n'
import { useTabStore } from '../../stores/tabStore'
import { useUIStore } from '../../stores/uiStore'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import {
  CANVAS_ZOOM_BOUNDS,
  clampZoom,
  deckManifestForPath,
  isDesignArtifactPath,
  isInDesignerSessionDir,
  useDesignerCanvasStore,
  type CanvasTweaks,
} from '../../stores/designerCanvasStore'
import { workspaceFilesApi } from '../../api/workspaceFiles'
import { designerApi } from '../../api/designer'
import { useDesignerStore } from '../../stores/designerStore'
import { useActiveTabWorkDir } from '../../lib/activeWorkDir'
import { printUnitsMerged } from '../../lib/designerPrint'
import { DesignArtifactFrame } from './DesignArtifactFrame'
import { DesignerAddUnitButton } from './DesignerAddUnit'

export function DesignerCanvasPanel() {
  const t = useTranslation()
  const activeTabId = useTabStore((s) => s.activeTabId)
  const explorerRoot = useWorkspaceFilesStore((s) => s.root)
  const sessionWorkDir = useActiveTabWorkDir()
  const root = sessionWorkDir ?? explorerRoot

  const panel = useDesignerCanvasStore((s) =>
    activeTabId ? s.panels[activeTabId] : undefined,
  )
  const ensure = useDesignerCanvasStore((s) => s.ensure)
  const setVisible = useDesignerCanvasStore((s) => s.setVisible)
  const setViewport = useDesignerCanvasStore((s) => s.setViewport)
  const selectUnit = useDesignerCanvasStore((s) => s.selectUnit)
  const updateUnitLayout = useDesignerCanvasStore((s) => s.updateUnitLayout)
  const autoFitUnit = useDesignerCanvasStore((s) => s.autoFitUnit)
  const setUnitTitle = useDesignerCanvasStore((s) => s.setUnitTitle)
  const renameUnit = useDesignerCanvasStore((s) => s.renameUnit)
  const addArtifactLive = useDesignerCanvasStore((s) => s.addArtifactLive)
  const removeUnit = useDesignerCanvasStore((s) => s.removeUnit)
  const removeUnitLocal = useDesignerCanvasStore((s) => s.removeUnitLocal)
  const migrateUnitPath = useDesignerCanvasStore((s) => s.migrateUnitPath)
  const loadHistory = useDesignerCanvasStore((s) => s.loadHistory)
  const setUnitDevice = useDesignerCanvasStore((s) => s.setUnitDevice)
  const setSelectMode = useDesignerCanvasStore((s) => s.setSelectMode)

  const addToast = useUIStore((s) => s.addToast)
  const viewportRef = useRef<HTMLDivElement | null>(null)
  const [viewportEl, setViewportEl] = useState<HTMLElement | null>(null)
  const [refreshTokens, setRefreshTokens] = useState<Record<string, number>>({})
  const [exporting, setExporting] = useState(false)
  const [printing, setPrinting] = useState(false)
  const [deleteArmed, setDeleteArmed] = useState(false)

  useEffect(() => {
    setDeleteArmed(false)
  }, [panel?.selectedUnitId])

  useEffect(() => {
    if (!deleteArmed) return
    const timer = window.setTimeout(() => setDeleteArmed(false), 3000)
    return () => window.clearTimeout(timer)
  }, [deleteArmed])

  useEffect(() => {
    if (!activeTabId || !panel?.visible) return
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      const target = event.target as HTMLElement | null
      if (target?.closest('input, textarea, select, [contenteditable]')) return
      const state = useDesignerCanvasStore.getState().panels[activeTabId]
      if (state?.selectMode) {
        setSelectMode(activeTabId, false)
      } else if (state?.selectedUnitId) {
        selectUnit(activeTabId, null)
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  }, [activeTabId, panel?.visible, selectUnit, setSelectMode])

  useEffect(() => {
    if (activeTabId) ensure(activeTabId)
  }, [activeTabId, ensure])

  useEffect(() => {
    if (!activeTabId || !panel?.visible) return
    if (!panel.loaded && !panel.loading) {
      void loadHistory(activeTabId)
    }
  }, [activeTabId, panel?.visible, panel?.loaded, panel?.loading, loadHistory])

  useEffect(() => {
    if (!activeTabId || !panel?.visible || !root) return
    const stop = workspaceFilesApi.watch(root, (event) => {
      const inSession = isInDesignerSessionDir(event.relPath, activeTabId)
      const deckManifest = deckManifestForPath(event.relPath)
      if (event.kind === 'removed') {
        if (!inSession) return
        if (deckManifest && deckManifest !== event.relPath) {
          setRefreshTokens((prev) => ({
            ...prev,
            [deckManifest]: (prev[deckManifest] ?? 0) + 1,
          }))
        } else {
          removeUnitLocal(activeTabId, event.relPath)
        }
        return
      }
      const unitPath = deckManifest ?? event.relPath
      if (!isDesignArtifactPath(unitPath) || !inSession) return
      if (
        event.kind === 'renamed' &&
        !deckManifest &&
        event.fromRelPath &&
        isInDesignerSessionDir(event.fromRelPath, activeTabId)
      ) {
        migrateUnitPath(activeTabId, event.fromRelPath, event.relPath)
      }
      if (
        event.kind === 'created' ||
        event.kind === 'modified' ||
        event.kind === 'renamed'
      ) {
        const submode =
          useDesignerStore.getState().sessions[activeTabId]?.selectedSubmodeId ?? null
        addArtifactLive(activeTabId, unitPath, submode)
        setRefreshTokens((prev) => ({
          ...prev,
          [unitPath]: (prev[unitPath] ?? 0) + 1,
        }))
      }
    })
    return stop
  }, [activeTabId, panel?.visible, root, addArtifactLive, removeUnitLocal, migrateUnitPath])

  const viewport = panel?.viewport ?? { panX: 0, panY: 0, zoom: 0.6 }
  const columnWidth = panel?.columnWidth ?? 640
  const units = panel?.units ?? []
  const selectMode = panel?.selectMode ?? false
  const tweaks = panel?.tweaks ?? {
    accent: null,
    scale: 1,
    density: 1,
    motion: 1,
    mode: 'auto' as const,
  }

  const onPanPointerDown = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0 && event.button !== 1) return
      if (!activeTabId) return
      const target = event.target as HTMLElement | null
      if (target?.closest('button, input, select, textarea, a')) return
      event.preventDefault()
      try {
        window.getSelection()?.removeAllRanges()
      } catch {
      }
      selectUnit(activeTabId, null)
      const startX = event.clientX
      const startY = event.clientY
      const originX = viewport.panX
      const originY = viewport.panY
      const el = viewportRef.current
      const pointerId = event.pointerId
      if (el) {
        el.style.cursor = 'grabbing'
        try {
          el.setPointerCapture(pointerId)
        } catch {
        }
      }
      const onMove = (ev: PointerEvent) => {
        setViewport(activeTabId, {
          panX: originX + (ev.clientX - startX),
          panY: originY + (ev.clientY - startY),
        })
      }
      const onUp = () => {
        if (el) {
          el.style.cursor = ''
          try {
            el.releasePointerCapture(pointerId)
          } catch {
          }
        }
        window.removeEventListener('pointermove', onMove)
        window.removeEventListener('pointerup', onUp)
        window.removeEventListener('pointercancel', onUp)
      }
      window.addEventListener('pointermove', onMove)
      window.addEventListener('pointerup', onUp)
      window.addEventListener('pointercancel', onUp)
    },
    [activeTabId, viewport.panX, viewport.panY, selectUnit, setViewport],
  )

  const onWheel = useCallback(
    (event: React.WheelEvent<HTMLDivElement>) => {
      if (!activeTabId) return
      event.preventDefault()
      const el = viewportRef.current
      const rect = el?.getBoundingClientRect()
      const cx = rect ? event.clientX - rect.left : 0
      const cy = rect ? event.clientY - rect.top : 0
      const oldZoom = viewport.zoom
      const factor = event.deltaY < 0 ? 1.1 : 1 / 1.1
      const newZoom = clampZoom(oldZoom * factor)
      const ratio = newZoom / oldZoom
      setViewport(activeTabId, {
        zoom: newZoom,
        panX: cx - (cx - viewport.panX) * ratio,
        panY: cy - (cy - viewport.panY) * ratio,
      })
    },
    [activeTabId, viewport.zoom, viewport.panX, viewport.panY, setViewport],
  )

  const zoomBy = useCallback(
    (factor: number) => {
      if (!activeTabId) return
      const el = viewportRef.current
      const rect = el?.getBoundingClientRect()
      const cx = rect ? rect.width / 2 : 0
      const cy = rect ? rect.height / 2 : 0
      const oldZoom = viewport.zoom
      const newZoom = clampZoom(oldZoom * factor)
      const ratio = newZoom / oldZoom
      setViewport(activeTabId, {
        zoom: newZoom,
        panX: cx - (cx - viewport.panX) * ratio,
        panY: cy - (cy - viewport.panY) * ratio,
      })
    },
    [activeTabId, viewport.zoom, viewport.panX, viewport.panY, setViewport],
  )

  const fitView = useCallback(() => {
    if (!activeTabId) return
    const el = viewportRef.current
    if (!el || units.length === 0) {
      setViewport(activeTabId, { panX: 0, panY: 0, zoom: 0.6 })
      return
    }
    const rect = el.getBoundingClientRect()
    let minX = Infinity
    let minY = Infinity
    let maxX = -Infinity
    let maxY = -Infinity
    for (const u of units) {
      minX = Math.min(minX, u.x)
      minY = Math.min(minY, u.y)
      maxX = Math.max(maxX, u.x + u.width)
      maxY = Math.max(maxY, u.y + u.height)
    }
    const pad = 56
    const contentW = Math.max(1, maxX - minX)
    const contentH = Math.max(1, maxY - minY)
    const availW = Math.max(1, rect.width - pad * 2)
    const availH = Math.max(1, rect.height - pad * 2)
    const zoom = clampZoom(Math.min(availW / contentW, availH / contentH, 1))
    const centerX = (minX + maxX) / 2
    const centerY = (minY + maxY) / 2
    setViewport(activeTabId, {
      zoom,
      panX: rect.width / 2 - centerX * zoom,
      panY: rect.height / 2 - centerY * zoom,
    })
  }, [activeTabId, units, setViewport])

  const onEditUnit = useCallback(
    (relPath: string) => {
      const filesStore = useWorkspaceFilesStore.getState()
      if (root && filesStore.root !== root) {
        filesStore.setRoot(root)
      }
      useUIStore.getState().setRightSidebarOpen(true)
      void useWorkspaceFilesStore.getState().openTab(relPath)
    },
    [root],
  )

  const onLintSelected = useCallback(async () => {
    if (!activeTabId) return
    const sel = panel?.selectedUnitId
    const unit = panel?.units.find((u) => u.id === sel)
    if (!unit || (unit.surface !== 'html' && unit.surface !== 'deck')) {
      addToast({ type: 'info', message: t('designer.canvas.lintSelectHint') })
      return
    }
    try {
      const res = await designerApi.lintArtifact(activeTabId, unit.relPath)
      if (res.ok && res.report) {
        const { p0, p1, p2 } = res.report
        addToast({
          type: p0 > 0 ? 'error' : 'success',
          message: t('designer.canvas.lintResult')
            .replace('{p0}', String(p0))
            .replace('{p1}', String(p1))
            .replace('{p2}', String(p2)),
        })
      } else {
        addToast({ type: 'error', message: res.error ?? t('designer.canvas.lintFailed') })
      }
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : t('designer.canvas.lintFailed'),
      })
    }
  }, [activeTabId, panel?.selectedUnitId, panel?.units, addToast, t])

  const onExportPdf = useCallback(async () => {
    if (!root || printing) return
    const htmlUnits = (panel?.units ?? [])
      .filter((u) => u.surface === 'html')
      .slice()
      .sort((a, b) => a.y - b.y || a.x - b.x)
      .map((u) => ({ relPath: u.relPath }))
    if (htmlUnits.length === 0) {
      addToast({ type: 'info', message: t('designer.canvas.pdfSelectHint') })
      return
    }
    setPrinting(true)
    try {
      let rawId: string | null = null
      try {
        rawId = (await workspaceFilesApi.rawHandle({ root })).rawId ?? null
      } catch {
        rawId = null
      }
      const readHtml = async (relPath: string): Promise<string | null> => {
        try {
          const res = await workspaceFilesApi.readFile({ root, path: relPath })
          return res.encoding === 'utf8' ? res.content : null
        } catch {
          return null
        }
      }
      const count = await printUnitsMerged({ root, rawId, units: htmlUnits, readHtml })
      if (count === 0) {
        addToast({ type: 'error', message: t('designer.canvas.pdfFailed') })
      } else {
        addToast({
          type: 'success',
          message: t('designer.canvas.pdfDone').replace('{n}', String(count)),
          duration: 6000,
        })
      }
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : t('designer.canvas.pdfFailed'),
      })
    } finally {
      setPrinting(false)
    }
  }, [root, printing, panel?.units, addToast, t])

  const onExportHandoff = useCallback(async () => {
    if (!activeTabId || exporting) return
    setExporting(true)
    try {
      const res = await designerApi.exportHandoff(activeTabId)
      if (res.ok && res.handoff) {
        addToast({
          type: 'success',
          message: t('designer.canvas.handoffDone')
            .replace('{n}', String(res.handoff.fileCount))
            .replace('{path}', res.handoff.zipPath),
        })
        onEditUnit(res.handoff.handoffPath)
      } else {
        addToast({ type: 'error', message: res.error ?? t('designer.canvas.handoffFailed') })
      }
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : t('designer.canvas.handoffFailed'),
      })
    } finally {
      setExporting(false)
    }
  }, [activeTabId, exporting, addToast, t, onEditUnit])

  if (!activeTabId || !panel?.visible) return null

  return (
    <aside
      data-testid="designer-canvas-panel"
      className="flex h-full min-h-0 min-w-[280px] flex-col overflow-hidden border-l border-[var(--color-border)] bg-[var(--color-surface)]"
      style={{ flex: `0 1 ${columnWidth}px`, maxWidth: '100%' }}
    >
      <header className="flex h-9 flex-shrink-0 items-center justify-between border-b border-[var(--color-border)] px-2">
        <div className="flex min-w-0 items-center gap-1.5">
          <span className="material-symbols-outlined text-[16px] text-[var(--color-text-tertiary)]">
            palette
          </span>
          <span className="truncate text-xs font-medium text-[var(--color-text-secondary)]">
            {t('designer.canvas.title')}
          </span>
          <span className="text-[10px] text-[var(--color-text-tertiary)]">
            {units.length}
          </span>
        </div>
        <div className="flex items-center gap-0.5">
          <DesignerAddUnitButton
            sessionId={activeTabId}
            onAdded={() => void loadHistory(activeTabId)}
          />
          <button
            type="button"
            onClick={() => activeTabId && setSelectMode(activeTabId, !selectMode)}
            title={t('designer.canvas.selectMode')}
            className={`flex h-6 w-6 items-center justify-center rounded ${
              selectMode
                ? 'bg-[var(--color-accent)] text-[var(--color-on-accent)]'
                : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
            }`}
          >
            <span className="material-symbols-outlined text-[16px]">ads_click</span>
          </button>
          {activeTabId && (
            <TweaksPopover sessionId={activeTabId} tweaks={tweaks} />
          )}
          <button
            type="button"
            disabled={!panel?.selectedUnitId}
            onClick={() => void onLintSelected()}
            title={t('designer.canvas.lint')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-30"
          >
            <span className="material-symbols-outlined text-[16px]">rule</span>
          </button>
          <button
            type="button"
            disabled={!panel?.selectedUnitId}
            onClick={() => {
              const sel = panel?.selectedUnitId
              if (!activeTabId || !sel) return
              if (!deleteArmed) {
                setDeleteArmed(true)
                addToast({
                  type: 'info',
                  message: t('designer.canvas.deleteConfirm'),
                  duration: 3000,
                })
                return
              }
              setDeleteArmed(false)
              void removeUnit(activeTabId, sel).then((ok) => {
                if (!ok) {
                  addToast({
                    type: 'error',
                    message: t('designer.canvas.deleteFailed'),
                  })
                }
              })
            }}
            title={
              deleteArmed
                ? t('designer.canvas.deleteConfirm')
                : t('designer.canvas.deleteUnit')
            }
            className={`flex h-6 w-6 items-center justify-center rounded disabled:opacity-30 ${
              deleteArmed
                ? 'bg-[var(--color-danger)]/15 text-[var(--color-danger)]'
                : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-danger)]/12 hover:text-[var(--color-danger)]'
            }`}
          >
            <span className="material-symbols-outlined text-[16px]">
              {deleteArmed ? 'delete_forever' : 'delete'}
            </span>
          </button>
          <button
            type="button"
            disabled={printing || !units.some((u) => u.surface === 'html')}
            onClick={() => void onExportPdf()}
            title={t('designer.canvas.exportPdf')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-30"
          >
            <span className="material-symbols-outlined text-[16px]">
              {printing ? 'hourglass_top' : 'picture_as_pdf'}
            </span>
          </button>
          <button
            type="button"
            disabled={exporting || units.length === 0}
            onClick={() => void onExportHandoff()}
            title={t('designer.canvas.exportHandoff')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-30"
          >
            <span className="material-symbols-outlined text-[16px]">
              {exporting ? 'hourglass_top' : 'inventory_2'}
            </span>
          </button>
          <button
            type="button"
            onClick={() => {
              if (!activeTabId) return
              void loadHistory(activeTabId).then((ok) => {
                if (!ok) {
                  addToast({
                    type: 'error',
                    message: t('designer.canvas.refreshFailed'),
                  })
                }
              })
            }}
            title={t('designer.canvas.refresh')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[16px]">refresh</span>
          </button>
          <button
            type="button"
            onClick={() => setVisible(activeTabId, false)}
            title={t('designer.canvas.close')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[16px]">close</span>
          </button>
        </div>
      </header>

      <div
        ref={(el) => {
          viewportRef.current = el
          setViewportEl(el)
        }}
        className="relative min-h-0 flex-1 cursor-grab select-none overflow-hidden"
        style={{
          backgroundColor: 'var(--color-surface-secondary)',
          backgroundImage:
            'radial-gradient(var(--color-border) 1px, transparent 1px)',
          backgroundSize: '24px 24px',
        }}
        onPointerDown={onPanPointerDown}
        onWheel={onWheel}
      >
        {units.length === 0 && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center px-6 text-center text-xs text-[var(--color-text-tertiary)]">
            {t('designer.canvas.empty')}
          </div>
        )}
        <div
          className="absolute left-0 top-0 origin-top-left"
          style={{
            transform: `translate(${viewport.panX}px, ${viewport.panY}px) scale(${viewport.zoom})`,
          }}
        >
          {units.map((unit) => (
            <DesignArtifactFrame
              key={unit.id}
              root={root ?? ''}
              sessionId={activeTabId}
              unit={unit}
              selected={panel.selectedUnitId === unit.id}
              selectMode={selectMode}
              tweaks={tweaks}
              viewportEl={viewportEl}
              refreshToken={refreshTokens[unit.relPath] ?? 0}
              zoom={viewport.zoom}
              onSelect={(id) => selectUnit(activeTabId, id)}
              onEdit={onEditUnit}
              onDragStartUnit={() => selectUnit(activeTabId, unit.id)}
              onSendToComposer={(relPath) => {
                selectUnit(activeTabId, unit.id)
                window.dispatchEvent(
                  new CustomEvent('designer:composer-ref', {
                    detail: { sessionId: activeTabId, relPath },
                  }),
                )
              }}
              onPickElement={(relPath, pick) => {
                selectUnit(activeTabId, unit.id)
                const element =
                  pick.odId || (pick.cssPath ? `css:${pick.cssPath}` : undefined)
                window.dispatchEvent(
                  new CustomEvent('designer:composer-ref', {
                    detail: {
                      sessionId: activeTabId,
                      relPath,
                      element,
                      elementLabel: pick.label,
                    },
                  }),
                )
                setSelectMode(activeTabId, false)
              }}
              onSetDevice={(id, device) => setUnitDevice(activeTabId, id, device)}
              onLayoutDrag={(id, next) =>
                updateUnitLayout(activeTabId, id, next)
              }
              onTitleResolved={(id, title) => setUnitTitle(activeTabId, id, title)}
              onRename={(id, name) => renameUnit(activeTabId, id, name)}
              onNaturalSize={(id, w, h) => autoFitUnit(activeTabId, id, w, h)}
            />
          ))}
        </div>

        <div
          className="absolute bottom-2 right-2 flex items-center gap-0.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-1 py-0.5 shadow-sm"
          onPointerDown={(e) => e.stopPropagation()}
        >
          <button
            type="button"
            onClick={() => zoomBy(1 / 1.2)}
            title={t('designer.canvas.zoomOut')}
            disabled={viewport.zoom <= CANVAS_ZOOM_BOUNDS.min}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-40"
          >
            <span className="material-symbols-outlined text-[15px]">remove</span>
          </button>
          <button
            type="button"
            onClick={() => {
              if (!activeTabId) return
              const el = viewportRef.current
              const rect = el?.getBoundingClientRect()
              const cx = rect ? rect.width / 2 : 0
              const cy = rect ? rect.height / 2 : 0
              const ratio = 1 / viewport.zoom
              setViewport(activeTabId, {
                zoom: 1,
                panX: cx - (cx - viewport.panX) * ratio,
                panY: cy - (cy - viewport.panY) * ratio,
              })
            }}
            title={t('designer.canvas.zoomReset')}
            className="min-w-[40px] rounded px-1 text-center text-[11px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            {Math.round(viewport.zoom * 100)}%
          </button>
          <button
            type="button"
            onClick={() => zoomBy(1.2)}
            title={t('designer.canvas.zoomIn')}
            disabled={viewport.zoom >= CANVAS_ZOOM_BOUNDS.max}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-40"
          >
            <span className="material-symbols-outlined text-[15px]">add</span>
          </button>
          <button
            type="button"
            onClick={fitView}
            title={t('designer.canvas.fitView')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[15px]">fit_screen</span>
          </button>
        </div>
      </div>
    </aside>
  )
}

function TweaksPopover({
  sessionId,
  tweaks,
}: {
  sessionId: string
  tweaks: CanvasTweaks
}) {
  const t = useTranslation()
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement | null>(null)
  const setTweaks = useDesignerCanvasStore((s) => s.setTweaks)
  const resetTweaks = useDesignerCanvasStore((s) => s.resetTweaks)

  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', onDoc)
    return () => document.removeEventListener('mousedown', onDoc)
  }, [open])

  const active =
    tweaks.accent !== null ||
    tweaks.scale !== 1 ||
    tweaks.density !== 1 ||
    tweaks.motion !== 1 ||
    tweaks.mode !== 'auto'

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        title={t('designer.canvas.tweaks')}
        className={`flex h-6 w-6 items-center justify-center rounded ${
          active
            ? 'bg-[var(--color-accent)] text-[var(--color-on-accent)]'
            : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
        }`}
      >
        <span className="material-symbols-outlined text-[16px]">tune</span>
      </button>
      {open && (
        <div className="absolute right-0 top-full z-[9999] mt-1 w-[248px] rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] p-3 shadow-[var(--shadow-dropdown)]">
          <div className="mb-2 flex items-center justify-between">
            <span className="text-[11px] font-medium uppercase tracking-wide text-[var(--color-text-secondary)]">
              {t('designer.canvas.tweaks')}
            </span>
            <button
              type="button"
              onClick={() => resetTweaks(sessionId)}
              className="text-[11px] text-[var(--color-accent)] hover:underline"
            >
              {t('designer.canvas.tweaksReset')}
            </button>
          </div>

          <label className="mb-2 flex items-center justify-between gap-2 text-[12px] text-[var(--color-text-primary)]">
            <span>{t('designer.canvas.tweakAccent')}</span>
            <span className="flex items-center gap-1">
              <input
                type="color"
                value={tweaks.accent ?? '#3b82f6'}
                onChange={(e) => setTweaks(sessionId, { accent: e.target.value })}
                className="h-5 w-7 cursor-pointer rounded border border-[var(--color-border)] bg-transparent p-0"
              />
              {tweaks.accent && (
                <button
                  type="button"
                  onClick={() => setTweaks(sessionId, { accent: null })}
                  className="material-symbols-outlined text-[14px] text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
                >
                  close
                </button>
              )}
            </span>
          </label>

          <TweakSlider
            label={t('designer.canvas.tweakScale')}
            value={tweaks.scale}
            min={0.8}
            max={1.3}
            step={0.05}
            onChange={(v) => setTweaks(sessionId, { scale: v })}
          />
          <TweakSlider
            label={t('designer.canvas.tweakDensity')}
            value={tweaks.density}
            min={0.7}
            max={1.4}
            step={0.05}
            onChange={(v) => setTweaks(sessionId, { density: v })}
          />
          <TweakSlider
            label={t('designer.canvas.tweakMotion')}
            value={tweaks.motion}
            min={0}
            max={1.5}
            step={0.1}
            onChange={(v) => setTweaks(sessionId, { motion: v })}
          />

          <div className="mt-2 flex items-center justify-between text-[12px] text-[var(--color-text-primary)]">
            <span>{t('designer.canvas.tweakMode')}</span>
            <div className="flex overflow-hidden rounded-md border border-[var(--color-border)]">
              {(['auto', 'light', 'dark'] as const).map((m) => (
                <button
                  key={m}
                  type="button"
                  onClick={() => setTweaks(sessionId, { mode: m })}
                  className={`px-2 py-0.5 text-[11px] ${
                    tweaks.mode === m
                      ? 'bg-[var(--color-accent)] text-[var(--color-on-accent)]'
                      : 'text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]'
                  }`}
                >
                  {t(`designer.canvas.mode.${m}`)}
                </button>
              ))}
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

function TweakSlider({
  label,
  value,
  min,
  max,
  step,
  onChange,
}: {
  label: string
  value: number
  min: number
  max: number
  step: number
  onChange: (v: number) => void
}) {
  return (
    <label className="mb-2 block text-[12px] text-[var(--color-text-primary)]">
      <span className="flex items-center justify-between">
        <span>{label}</span>
        <span className="text-[11px] text-[var(--color-text-tertiary)]">
          {value.toFixed(2)}
        </span>
      </span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="mt-1 w-full accent-[var(--color-accent)]"
      />
    </label>
  )
}
