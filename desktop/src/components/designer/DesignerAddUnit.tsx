// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from '../../i18n'
import { useAnchoredDropdown } from '../../hooks/useAnchoredDropdown'
import { useUIStore } from '../../stores/uiStore'
import { useDesignerStore } from '../../stores/designerStore'
import { designerApi } from '../../api/designer'

type Stroke = {
  color: string
  width: number
  points: Array<[number, number]>
}

const SKETCH_W = 960
const SKETCH_H = 640
const SKETCH_COLORS = ['#1a1a1a', '#d23f31', '#1a73e8', '#188038', '#f29900']
const SKETCH_WIDTHS = [2, 4, 8]

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function strokesToSvg(strokes: Stroke[]): string {
  const paths = strokes
    .filter((s) => s.points.length > 0)
    .map((s) => {
      const d = s.points
        .map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(1)} ${y.toFixed(1)}`)
        .join(' ')
      return `<path d="${d}" fill="none" stroke="${s.color}" stroke-width="${s.width}" stroke-linecap="round" stroke-linejoin="round"/>`
    })
    .join('\n      ')
  return `<svg viewBox="0 0 ${SKETCH_W} ${SKETCH_H}" xmlns="http://www.w3.org/2000/svg" role="img">\n      ${paths}\n    </svg>`
}

function buildSketchHtml(name: string, strokes: Stroke[]): string {
  const title = escapeHtml(name)
  return `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title}</title>
<style>
:root { --od-accent: #1a73e8; --od-scale: 1; --od-density: 1; --od-motion: 1; --bg: #fcfbf9; --ink: #1a1a1a; --frame: #e6e2da; }
html[data-od-mode="dark"] { --bg: #17181a; --ink: #ececec; --frame: #2c2e33; }
* { box-sizing: border-box; }
body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: var(--bg); padding: calc(24px * var(--od-density)); }
main { width: min(100%, ${SKETCH_W}px); }
figure { margin: 0; border: 1px solid var(--frame); border-radius: 12px; background: #fff; overflow: hidden; }
html[data-od-mode="dark"] figure { background: #1f2125; }
svg { display: block; width: 100%; height: auto; }
figcaption { padding: calc(10px * var(--od-density)) calc(14px * var(--od-density)); border-top: 1px solid var(--frame); color: var(--ink); font: 500 calc(13px * var(--od-scale))/1.4 system-ui, sans-serif; }
</style>
</head>
<body>
<main data-od-id="sketch-unit" data-od-label="${title}">
  <figure data-od-id="sketch-figure" data-od-label="Sketch">
    ${strokesToSvg(strokes)}
    <figcaption data-od-id="sketch-caption" data-od-label="Caption">${title}</figcaption>
  </figure>
</main>
</body>
</html>
`
}

export function DesignerAddUnitButton({
  sessionId,
  onAdded,
}: {
  sessionId: string
  onAdded: (relPath: string) => void
}) {
  const t = useTranslation()
  const addToast = useUIStore((s) => s.addToast)
  const [menuOpen, setMenuOpen] = useState(false)
  const [templateOpen, setTemplateOpen] = useState(false)
  const [sketchOpen, setSketchOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [query, setQuery] = useState('')
  const rootRef = useRef<HTMLDivElement | null>(null)
  const { menuRef, style, portalTarget } = useAnchoredDropdown(
    menuOpen || templateOpen,
    () => {
      setMenuOpen(false)
      setTemplateOpen(false)
    },
    { anchorRef: rootRef, align: 'right', estimatedHeight: 360, overflow: 'hidden' },
  )

  const htmlTemplates = useDesignerStore((s) => s.htmlTemplates)
  const storeLoaded = useDesignerStore((s) => s.loaded)

  useEffect(() => {
    if ((menuOpen || templateOpen) && !storeLoaded) {
      void useDesignerStore.getState().load()
    }
  }, [menuOpen, templateOpen, storeLoaded])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return htmlTemplates
    return htmlTemplates.filter(
      (tpl) =>
        tpl.title.toLowerCase().includes(q) ||
        tpl.id.toLowerCase().includes(q) ||
        tpl.category.toLowerCase().includes(q) ||
        tpl.tags.some((tag) => tag.toLowerCase().includes(q)),
    )
  }, [htmlTemplates, query])

  const submit = async (body: {
    source: 'template' | 'html'
    templateId?: string
    name?: string
    html?: string
  }) => {
    if (busy) return
    setBusy(true)
    try {
      const res = await designerApi.addUnit(sessionId, body)
      if (res.ok && res.relPath) {
        addToast({ type: 'success', message: t('designer.canvas.addUnitDone') })
        setMenuOpen(false)
        setTemplateOpen(false)
        setSketchOpen(false)
        onAdded(res.relPath)
      } else {
        addToast({
          type: 'error',
          message: res.error ?? t('designer.canvas.addUnitFailed'),
        })
      }
    } catch (err) {
      addToast({
        type: 'error',
        message: err instanceof Error ? err.message : t('designer.canvas.addUnitFailed'),
      })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        onClick={() => {
          setTemplateOpen(false)
          setMenuOpen((v) => !v)
        }}
        title={t('designer.canvas.addUnit')}
        className={`flex h-6 w-6 items-center justify-center rounded ${
          menuOpen || templateOpen
            ? 'bg-[var(--color-accent)] text-[var(--color-on-accent)]'
            : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
        }`}
      >
        <span className="material-symbols-outlined text-[16px]">add_box</span>
      </button>

      {menuOpen && !templateOpen && style && createPortal(
        <div
          ref={menuRef}
          style={style}
          className="w-[176px] rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] py-1 shadow-[var(--shadow-dropdown)]"
        >
          <button
            type="button"
            onClick={() => setTemplateOpen(true)}
            className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-[12px] text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
          >
            <span className="material-symbols-outlined text-[15px] text-[var(--color-text-tertiary)]">
              dashboard_customize
            </span>
            <span>{t('designer.canvas.addFromTemplate')}</span>
          </button>
          <button
            type="button"
            onClick={() => {
              setMenuOpen(false)
              setSketchOpen(true)
            }}
            className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-[12px] text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
          >
            <span className="material-symbols-outlined text-[15px] text-[var(--color-text-tertiary)]">
              draw
            </span>
            <span>{t('designer.canvas.addSketch')}</span>
          </button>
        </div>,
        portalTarget,
      )}

      {templateOpen && style && createPortal(
        <div
          ref={menuRef}
          style={style}
          className="flex min-h-0 w-[280px] flex-col rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] shadow-[var(--shadow-dropdown)]"
        >
          <div className="border-b border-[var(--color-border)] p-2">
            <input
              autoFocus
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t('designer.htmlTemplate.searchPlaceholder')}
              className="w-full rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
            />
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto py-1">
            {filtered.length === 0 && (
              <div className="px-3 py-4 text-center text-[11px] text-[var(--color-text-tertiary)]">
                {t('designer.htmlTemplate.count').replace('{n}', '0')}
              </div>
            )}
            {filtered.map((tpl) => (
              <button
                key={tpl.id}
                type="button"
                disabled={busy}
                onClick={() => void submit({ source: 'template', templateId: tpl.id })}
                className="flex w-full flex-col gap-0.5 px-2.5 py-1.5 text-left hover:bg-[var(--color-surface-hover)] disabled:opacity-50"
              >
                <span className="text-[12px] text-[var(--color-text-primary)]">
                  {tpl.title}
                </span>
                <span className="text-[10px] text-[var(--color-text-tertiary)]">
                  {tpl.category}
                </span>
              </button>
            ))}
          </div>
        </div>,
        portalTarget,
      )}

      {sketchOpen && (
        <SketchDialog
          busy={busy}
          onCancel={() => setSketchOpen(false)}
          onSave={(name, strokes) =>
            void submit({ source: 'html', name, html: buildSketchHtml(name, strokes) })
          }
        />
      )}
    </div>
  )
}

function SketchDialog({
  busy,
  onCancel,
  onSave,
}: {
  busy: boolean
  onCancel: () => void
  onSave: (name: string, strokes: Stroke[]) => void
}) {
  const t = useTranslation()
  const [strokes, setStrokes] = useState<Stroke[]>([])
  const [draft, setDraft] = useState<Stroke | null>(null)
  const [color, setColor] = useState<string>('#1a1a1a')
  const [width, setWidth] = useState<number>(4)
  const [name, setName] = useState('')
  const boardRef = useRef<SVGSVGElement | null>(null)

  const toLocal = (e: React.PointerEvent): [number, number] => {
    const el = boardRef.current
    if (!el) return [0, 0]
    const rect = el.getBoundingClientRect()
    const x = ((e.clientX - rect.left) / rect.width) * SKETCH_W
    const y = ((e.clientY - rect.top) / rect.height) * SKETCH_H
    return [
      Math.max(0, Math.min(SKETCH_W, x)),
      Math.max(0, Math.min(SKETCH_H, y)),
    ]
  }

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return
    e.preventDefault()
    e.currentTarget.setPointerCapture(e.pointerId)
    setDraft({ color, width, points: [toLocal(e)] })
  }
  const onPointerMove = (e: React.PointerEvent) => {
    if (!draft) return
    e.preventDefault()
    const pt = toLocal(e)
    setDraft((prev) => (prev ? { ...prev, points: [...prev.points, pt] } : prev))
  }
  const onPointerUp = () => {
    if (!draft) return
    if (draft.points.length > 1) setStrokes((prev) => [...prev, draft])
    setDraft(null)
  }

  const all = draft ? [...strokes, draft] : strokes
  const canSave = strokes.length > 0 && !busy

  return (
    <div className="fixed inset-0 z-[10000] flex items-center justify-center bg-black/40 p-6">
      <div className="flex max-h-full w-[720px] max-w-full flex-col overflow-hidden rounded-xl border border-[var(--color-border)] bg-[var(--color-surface)] shadow-xl">
        <div className="flex items-center justify-between border-b border-[var(--color-border)] px-3 py-2">
          <span className="text-[13px] font-medium text-[var(--color-text-primary)]">
            {t('designer.canvas.addSketch')}
          </span>
          <button
            type="button"
            onClick={onCancel}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)]"
          >
            <span className="material-symbols-outlined text-[16px]">close</span>
          </button>
        </div>

        <div className="flex flex-wrap items-center gap-2 border-b border-[var(--color-border)] px-3 py-2">
          <div className="flex items-center gap-1">
            {SKETCH_COLORS.map((c) => (
              <button
                key={c}
                type="button"
                onClick={() => setColor(c)}
                className={`h-5 w-5 rounded-full border-2 ${
                  color === c ? 'border-[var(--color-accent)]' : 'border-transparent'
                }`}
                style={{ backgroundColor: c }}
                title={c}
              />
            ))}
          </div>
          <div className="flex items-center gap-1">
            {SKETCH_WIDTHS.map((w) => (
              <button
                key={w}
                type="button"
                onClick={() => setWidth(w)}
                className={`flex h-6 w-6 items-center justify-center rounded ${
                  width === w
                    ? 'bg-[var(--color-surface-selected)]'
                    : 'hover:bg-[var(--color-surface-hover)]'
                }`}
                title={`${w}px`}
              >
                <span
                  className="rounded-full bg-[var(--color-text-primary)]"
                  style={{ width: `${w + 2}px`, height: `${w + 2}px` }}
                />
              </button>
            ))}
          </div>
          <div className="flex-1" />
          <button
            type="button"
            disabled={strokes.length === 0}
            onClick={() => setStrokes((prev) => prev.slice(0, -1))}
            title={t('designer.canvas.sketchUndo')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-30"
          >
            <span className="material-symbols-outlined text-[16px]">undo</span>
          </button>
          <button
            type="button"
            disabled={strokes.length === 0}
            onClick={() => setStrokes([])}
            title={t('designer.canvas.sketchClear')}
            className="flex h-6 w-6 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] disabled:opacity-30"
          >
            <span className="material-symbols-outlined text-[16px]">mop</span>
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-hidden bg-[var(--color-surface-secondary)] p-3">
          <svg
            ref={boardRef}
            viewBox={`0 0 ${SKETCH_W} ${SKETCH_H}`}
            className="h-auto w-full cursor-crosshair touch-none rounded-lg border border-[var(--color-border)] bg-white"
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerLeave={onPointerUp}
          >
            {all.map((s, i) => (
              <path
                key={i}
                d={s.points
                  .map(([x, y], j) => `${j === 0 ? 'M' : 'L'}${x} ${y}`)
                  .join(' ')}
                fill="none"
                stroke={s.color}
                strokeWidth={s.width}
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            ))}
          </svg>
        </div>

        <div className="flex items-center gap-2 border-t border-[var(--color-border)] px-3 py-2">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t('designer.canvas.sketchNamePlaceholder')}
            className="min-w-0 flex-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] px-2 py-1 text-[12px] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-accent)]"
          />
          <button
            type="button"
            onClick={onCancel}
            className="rounded-md px-3 py-1 text-[12px] text-[var(--color-text-secondary)] hover:bg-[var(--color-surface-hover)]"
          >
            {t('common.cancel')}
          </button>
          <button
            type="button"
            disabled={!canSave}
            onClick={() => onSave(name.trim() || t('designer.canvas.sketchDefaultName'), strokes)}
            className="rounded-md bg-[var(--color-accent)] px-3 py-1 text-[12px] text-[var(--color-on-accent)] disabled:opacity-40"
          >
            {busy ? '…' : t('designer.canvas.sketchSave')}
          </button>
        </div>
      </div>
    </div>
  )
}
