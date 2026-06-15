// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from '../../../i18n'
import { workspaceFilesApi } from '../../../api/workspaceFiles'
import { useUIStore } from '../../../stores/uiStore'

export type ImageRegionPick = {
  odId: string
  label: string
}

type RegionTool = 'rect' | 'lasso'

type Props = {
  root: string
  relPath: string
  src: string
  selectMode: boolean
  onPicked: (pick: ImageRegionPick) => void
  onLoadFailed: () => void
}

type Point = { x: number; y: number }

function maskRelPathFor(relPath: string): string {
  const normalized = relPath.replace(/\\/g, '/')
  const slash = normalized.lastIndexOf('/')
  const dir = slash === -1 ? '' : normalized.slice(0, slash)
  const base = slash === -1 ? normalized : normalized.slice(slash + 1)
  const dot = base.lastIndexOf('.')
  const stem = dot === -1 ? base : base.slice(0, dot)
  const safeStem = stem.replace(/[^a-zA-Z0-9_-]/g, '_')
  const ts = Date.now().toString(36)
  return `${dir ? `${dir}/` : ''}masks/${safeStem}-region-${ts}.png`
}

export function ImageRegionSelector({
  root,
  relPath,
  src,
  selectMode,
  onPicked,
  onLoadFailed,
}: Props) {
  const t = useTranslation()
  const addToast = useUIStore((s) => s.addToast)
  const containerRef = useRef<HTMLDivElement | null>(null)
  const imgRef = useRef<HTMLImageElement | null>(null)
  const overlayRef = useRef<HTMLCanvasElement | null>(null)
  const [tool, setTool] = useState<RegionTool>('rect')
  const [saving, setSaving] = useState(false)
  const [pickedBadge, setPickedBadge] = useState(false)
  const drawingRef = useRef(false)
  const startRef = useRef<Point>({ x: 0, y: 0 })
  const pointsRef = useRef<Point[]>([])
  const committedRef = useRef<{ tool: RegionTool; points: Point[] } | null>(null)

  useEffect(() => {
    if (!selectMode) {
      committedRef.current = null
      setPickedBadge(false)
      const canvas = overlayRef.current
      const ctx = canvas?.getContext('2d')
      if (canvas && ctx) ctx.clearRect(0, 0, canvas.width, canvas.height)
    }
  }, [selectMode])

  const syncOverlaySize = useCallback(() => {
    const container = containerRef.current
    const canvas = overlayRef.current
    if (!container || !canvas) return
    const rect = container.getBoundingClientRect()
    const w = Math.max(1, Math.round(rect.width))
    const h = Math.max(1, Math.round(rect.height))
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w
      canvas.height = h
    }
  }, [])

  const redraw = useCallback(
    (livePoints: Point[] | null) => {
      const canvas = overlayRef.current
      const ctx = canvas?.getContext('2d')
      if (!canvas || !ctx) return
      ctx.clearRect(0, 0, canvas.width, canvas.height)
      const drawShape = (shapeTool: RegionTool, pts: Point[], emphasize: boolean) => {
        const first = pts[0]
        const last = pts[pts.length - 1]
        if (!first || !last || pts.length < 2) return
        ctx.beginPath()
        if (shapeTool === 'rect') {
          ctx.rect(
            Math.min(first.x, last.x),
            Math.min(first.y, last.y),
            Math.abs(last.x - first.x),
            Math.abs(last.y - first.y),
          )
        } else {
          ctx.moveTo(first.x, first.y)
          for (const p of pts.slice(1)) ctx.lineTo(p.x, p.y)
          ctx.closePath()
        }
        ctx.fillStyle = emphasize ? 'rgba(59,130,246,0.28)' : 'rgba(59,130,246,0.16)'
        ctx.fill()
        ctx.lineWidth = 2
        ctx.strokeStyle = '#3b82f6'
        ctx.setLineDash(emphasize ? [] : [6, 4])
        ctx.stroke()
        ctx.setLineDash([])
      }
      if (committedRef.current) {
        drawShape(committedRef.current.tool, committedRef.current.points, true)
      }
      if (livePoints && livePoints.length >= 2) {
        drawShape(tool, livePoints, false)
      }
    },
    [tool],
  )

  const localPoint = (e: React.PointerEvent): Point => {
    const rect = containerRef.current?.getBoundingClientRect()
    return {
      x: e.clientX - (rect?.left ?? 0),
      y: e.clientY - (rect?.top ?? 0),
    }
  }

  const finishSelection = useCallback(
    async (points: Point[]) => {
      const img = imgRef.current
      const container = containerRef.current
      if (!img || !container || points.length < 2) return
      const naturalW = img.naturalWidth
      const naturalH = img.naturalHeight
      if (naturalW <= 0 || naturalH <= 0) return
      const crect = container.getBoundingClientRect()
      const imgRect = img.getBoundingClientRect()
      if (imgRect.width <= 0 || imgRect.height <= 0) return
      const fit = {
        x: imgRect.left - crect.left,
        y: imgRect.top - crect.top,
        w: imgRect.width,
        h: imgRect.height,
      }
      const toImage = (p: Point): Point => ({
        x: Math.min(Math.max((p.x - fit.x) / fit.w, 0), 1) * naturalW,
        y: Math.min(Math.max((p.y - fit.y) / fit.h, 0), 1) * naturalH,
      })
      const imgPoints = points.map(toImage)
      const xs = imgPoints.map((p) => p.x)
      const ys = imgPoints.map((p) => p.y)
      const minX = Math.min(...xs)
      const maxX = Math.max(...xs)
      const minY = Math.min(...ys)
      const maxY = Math.max(...ys)
      if (maxX - minX < 4 || maxY - minY < 4) return

      const nx = minX / naturalW
      const ny = minY / naturalH
      const nw = (maxX - minX) / naturalW
      const nh = (maxY - minY) / naturalH

      const maskCanvas = document.createElement('canvas')
      maskCanvas.width = naturalW
      maskCanvas.height = naturalH
      const mctx = maskCanvas.getContext('2d')
      if (!mctx) return
      mctx.fillStyle = '#000000'
      mctx.fillRect(0, 0, naturalW, naturalH)
      mctx.fillStyle = '#ffffff'
      const firstPoint = imgPoints[0]
      if (tool === 'rect' || !firstPoint) {
        mctx.fillRect(minX, minY, maxX - minX, maxY - minY)
      } else {
        mctx.beginPath()
        mctx.moveTo(firstPoint.x, firstPoint.y)
        for (const p of imgPoints.slice(1)) {
          mctx.lineTo(p.x, p.y)
        }
        mctx.closePath()
        mctx.fill()
      }
      const dataUrl = maskCanvas.toDataURL('image/png')
      const base64 = dataUrl.slice(dataUrl.indexOf(',') + 1)
      const maskRel = maskRelPathFor(relPath)

      setSaving(true)
      try {
        await workspaceFilesApi.writeFile({
          root,
          path: maskRel,
          content: base64,
          encoding: 'base64',
        })
      } catch {
        setSaving(false)
        addToast({
          type: 'error',
          message: t('designer.image.maskSaveFailed'),
          duration: 5000,
        })
        return
      }
      setSaving(false)
      committedRef.current = { tool, points }
      redraw(null)
      setPickedBadge(true)
      const coords = [nx, ny, nw, nh].map((v) => v.toFixed(4)).join(',')
      onPicked({
        odId: `image-region:${coords}:${maskRel}`,
        label: t('designer.image.regionLabel', {
          w: Math.round(nw * 100),
          h: Math.round(nh * 100),
        }),
      })
    },
    [addToast, onPicked, redraw, relPath, root, t, tool],
  )

  const onPointerDown = (e: React.PointerEvent) => {
    if (!selectMode || e.button !== 0 || saving) return
    e.preventDefault()
    syncOverlaySize()
    drawingRef.current = true
    const p = localPoint(e)
    startRef.current = p
    pointsRef.current = [p]
    try {
      ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    } catch {
      /* ignore */
    }
  }

  const onPointerMove = (e: React.PointerEvent) => {
    if (!drawingRef.current) return
    const p = localPoint(e)
    if (tool === 'rect') {
      pointsRef.current = [startRef.current, p]
    } else {
      pointsRef.current.push(p)
    }
    redraw(pointsRef.current)
  }

  const onPointerUp = (e: React.PointerEvent) => {
    if (!drawingRef.current) return
    drawingRef.current = false
    try {
      ;(e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId)
    } catch {
      /* ignore */
    }
    const points = pointsRef.current
    pointsRef.current = []
    redraw(null)
    void finishSelection(points)
  }

  return (
    <div
      ref={containerRef}
      className="relative flex h-full items-center justify-center bg-[var(--color-surface)] p-2"
    >
      <img
        ref={imgRef}
        src={src}
        alt=""
        draggable={false}
        onError={onLoadFailed}
        className="max-h-full max-w-full select-none object-contain"
      />
      {selectMode && (
        <>
          <canvas
            ref={overlayRef}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
            className="absolute inset-0 h-full w-full touch-none"
            style={{ cursor: 'crosshair' }}
          />
          <div className="absolute left-1.5 top-1.5 flex items-center gap-0.5 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]/95 p-0.5 shadow-sm">
            <button
              type="button"
              onClick={() => setTool('rect')}
              title={t('designer.image.regionRect')}
              className={`flex h-5 w-5 items-center justify-center rounded ${
                tool === 'rect'
                  ? 'bg-[var(--color-accent)] text-[var(--color-on-accent)]'
                  : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)]'
              }`}
            >
              <span className="material-symbols-outlined text-[13px]">crop_square</span>
            </button>
            <button
              type="button"
              onClick={() => setTool('lasso')}
              title={t('designer.image.regionLasso')}
              className={`flex h-5 w-5 items-center justify-center rounded ${
                tool === 'lasso'
                  ? 'bg-[var(--color-accent)] text-[var(--color-on-accent)]'
                  : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)]'
              }`}
            >
              <span className="material-symbols-outlined text-[13px]">gesture</span>
            </button>
          </div>
          {(pickedBadge || saving) && (
            <div className="pointer-events-none absolute bottom-1.5 right-1.5 rounded bg-black/60 px-1.5 py-0.5 text-[10px] text-white/90">
              {saving ? '…' : t('designer.image.regionPicked')}
            </div>
          )}
        </>
      )}
    </div>
  )
}
