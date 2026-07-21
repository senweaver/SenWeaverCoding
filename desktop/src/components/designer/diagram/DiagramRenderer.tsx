// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useRef, useState } from 'react'
import DOMPurify from 'dompurify'
import { useTranslation } from '../../../i18n'
import { workspaceFilesApi } from '../../../api/workspaceFiles'
import { useUIStore } from '../../../stores/uiStore'

export type DiagramEngine = 'mermaid' | 'echarts' | 'markmap'

type Props = {
  root: string
  relPath: string
  refreshToken: number
  contentW: number
  contentH: number
  onTitleResolved: (title: string) => void
}

const CONTROLS_H = 30

export function diagramEngineForPath(path: string): DiagramEngine | null {
  const lower = path.toLowerCase()
  if (lower.endsWith('.mmd')) return 'mermaid'
  if (lower.endsWith('.echarts.json')) return 'echarts'
  if (lower.endsWith('.mindmap.md')) return 'markmap'
  return null
}

type MermaidTheme = 'default' | 'dark'

// mermaid is ~1.5MB; load it lazily only when a mermaid diagram is opened.
type MermaidApi = typeof import('mermaid')['default']
let diagramMermaidModulePromise: Promise<MermaidApi> | null = null
function loadDiagramMermaid(): Promise<MermaidApi> {
  if (!diagramMermaidModulePromise) {
    diagramMermaidModulePromise = import('mermaid').then((m) => m.default)
  }
  return diagramMermaidModulePromise
}

let diagramMermaidTheme: MermaidTheme | null = null

function initDiagramMermaid(mermaid: MermaidApi, theme: MermaidTheme) {
  if (diagramMermaidTheme === theme) return
  mermaid.initialize({
    startOnLoad: false,
    theme,
    securityLevel: 'strict',
    suppressErrorRendering: true,
    fontFamily: 'var(--font-sans)',
    htmlLabels: false,
    flowchart: { useMaxWidth: true, curve: 'basis' },
    class: { useMaxWidth: true },
    sequence: { useMaxWidth: true },
    state: { useMaxWidth: true },
  })
  diagramMermaidTheme = theme
}

let diagramIdCounter = 0

function sanitizeSvg(svg: string): string {
  return DOMPurify.sanitize(svg, {
    USE_PROFILES: { svg: true, svgFilters: true },
    ADD_TAGS: ['foreignObject', 'div', 'span', 'p', 'br', 'em', 'strong', 'i', 'b', 'code'],
    ADD_ATTR: ['xmlns', 'xmlns:xhtml', 'class', 'style'],
  })
}

function extractTitle(engine: DiagramEngine, source: string): string | null {
  if (engine === 'echarts') {
    try {
      const option = JSON.parse(source) as { title?: { text?: string } }
      const text = option.title?.text?.trim()
      return text || null
    } catch {
      return null
    }
  }
  if (engine === 'markmap') {
    for (const line of source.split('\n')) {
      const trimmed = line.trim()
      if (trimmed.startsWith('- ') || trimmed.startsWith('* ')) {
        return trimmed.slice(2).trim() || null
      }
    }
    return null
  }
  for (const line of source.split('\n')) {
    const trimmed = line.trim()
    const m = trimmed.match(/^title[:\s]+(.+)$/i)
    if (m?.[1]) return m[1].trim()
  }
  const head = source
    .split('\n')
    .map((l) => l.trim())
    .find((l) => l.length > 0 && !l.startsWith('%%'))
  return head?.split(/\s+/)[0] ?? null
}

function downloadDataUrl(dataUrl: string, filename: string) {
  const anchor = document.createElement('a')
  anchor.href = dataUrl
  anchor.download = filename
  document.body.appendChild(anchor)
  anchor.click()
  anchor.remove()
}

function svgElementToString(svg: SVGSVGElement): string {
  const clone = svg.cloneNode(true) as SVGSVGElement
  if (!clone.getAttribute('xmlns')) {
    clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg')
  }
  return new XMLSerializer().serializeToString(clone)
}

function ensureSvgSize(svgText: string): string {
  try {
    const doc = new DOMParser().parseFromString(svgText, 'image/svg+xml')
    const svg = doc.documentElement
    if (svg.tagName.toLowerCase() !== 'svg') return svgText
    const hasW = svg.hasAttribute('width') && !svg.getAttribute('width')?.includes('%')
    const hasH = svg.hasAttribute('height') && !svg.getAttribute('height')?.includes('%')
    if (hasW && hasH) return svgText
    const viewBox = svg.getAttribute('viewBox')
    if (!viewBox) return svgText
    const parts = viewBox.split(/[\s,]+/).map(Number)
    const w = parts[2]
    const h = parts[3]
    if (!w || !h || Number.isNaN(w) || Number.isNaN(h)) return svgText
    svg.setAttribute('width', String(Math.ceil(w)))
    svg.setAttribute('height', String(Math.ceil(h)))
    svg.style.maxWidth = ''
    return new XMLSerializer().serializeToString(svg)
  } catch {
    return svgText
  }
}

async function svgStringToPngDataUrl(rawSvgText: string, scale = 2): Promise<string> {
  const svgText = ensureSvgSize(rawSvgText)
  const blob = new Blob([svgText], { type: 'image/svg+xml;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  try {
    const img = new Image()
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve()
      img.onerror = () => reject(new Error('svg load failed'))
      img.src = url
    })
    const w = Math.max(1, img.naturalWidth || 1200)
    const h = Math.max(1, img.naturalHeight || 800)
    const canvas = document.createElement('canvas')
    canvas.width = w * scale
    canvas.height = h * scale
    const ctx = canvas.getContext('2d')
    if (!ctx) throw new Error('canvas 2d unavailable')
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, canvas.width, canvas.height)
    ctx.scale(scale, scale)
    ctx.drawImage(img, 0, 0, w, h)
    return canvas.toDataURL('image/png')
  } finally {
    URL.revokeObjectURL(url)
  }
}

function stemOf(relPath: string): string {
  const base = relPath.split('/').pop() ?? 'diagram'
  const dot = base.indexOf('.')
  return dot === -1 ? base : base.slice(0, dot)
}

type EChartsApi = {
  init: (
    el: HTMLElement,
    theme?: string | null,
    opts?: { renderer?: 'canvas' | 'svg' },
  ) => {
    setOption: (option: Record<string, unknown>, opts?: { notMerge?: boolean }) => void
    getDataURL: (opts?: { pixelRatio?: number; backgroundColor?: string }) => string
    resize: () => void
    dispose: () => void
  }
}

type EChartsInstance = ReturnType<EChartsApi['init']>

type MarkmapHandle = {
  setData: (root: unknown) => void
  fit: () => void
  destroy: () => void
}

function enhanceEChartsOption(option: Record<string, unknown>): Record<string, unknown> {
  const out = { ...option }
  if (out.tooltip === undefined) out.tooltip = {}
  const series = Array.isArray(out.series) ? out.series : out.series ? [out.series] : []
  out.series = series.map((s) => {
    if (s && typeof s === 'object') {
      const rec = s as Record<string, unknown>
      const type = typeof rec.type === 'string' ? rec.type : ''
      if (['graph', 'tree', 'sankey', 'map'].includes(type) && rec.roam === undefined) {
        return { ...rec, roam: true }
      }
    }
    return s
  })
  return out
}

export function DiagramRenderer({
  root,
  relPath,
  refreshToken,
  contentW,
  contentH,
  onTitleResolved,
}: Props) {
  const t = useTranslation()
  const themeMode = useUIStore((s) => s.theme)
  const addToast = useUIStore((s) => s.addToast)
  const engine = diagramEngineForPath(relPath)
  const [source, setSource] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [mermaidSvg, setMermaidSvg] = useState<string | null>(null)
  const stageRef = useRef<HTMLDivElement | null>(null)
  const echartsRef = useRef<EChartsInstance | null>(null)
  const markmapSvgRef = useRef<SVGSVGElement | null>(null)
  const markmapRef = useRef<MarkmapHandle | null>(null)

  useEffect(() => {
    if (!root) return
    let cancelled = false
    workspaceFilesApi
      .readFile({ root, path: relPath })
      .then((res) => {
        if (cancelled) return
        if (res.encoding === 'utf8') {
          setSource(res.content)
        } else {
          setError('binary file')
        }
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      })
    return () => {
      cancelled = true
    }
  }, [root, relPath, refreshToken])

  useEffect(() => {
    if (source === null || !engine) return
    const title = extractTitle(engine, source)
    if (title) onTitleResolved(title)
  }, [source, engine, onTitleResolved])

  useEffect(() => {
    if (source === null || engine !== 'mermaid') return
    let cancelled = false
    setError(null)
    const id = `designer-diagram-${++diagramIdCounter}`
    loadDiagramMermaid()
      .then((mermaid) => {
        if (cancelled) return
        initDiagramMermaid(mermaid, themeMode === 'dark' ? 'dark' : 'default')
        return mermaid
          .parse(source)
          .then(() => mermaid.render(id, source))
          .then(({ svg }) => {
            if (!cancelled) setMermaidSvg(svg)
          })
      })
      .catch((e: unknown) => {
        if (!cancelled) {
          setMermaidSvg(null)
          setError(e instanceof Error ? e.message : String(e))
        }
      })
    return () => {
      cancelled = true
    }
  }, [source, engine, themeMode])

  useEffect(() => {
    if (source === null || engine !== 'echarts') return
    const el = stageRef.current
    if (!el) return
    let cancelled = false
    setError(null)
    let parsed: Record<string, unknown>
    try {
      parsed = JSON.parse(source) as Record<string, unknown>
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
      return
    }
    void import('echarts').then((echartsModule) => {
      if (cancelled) return
      const echarts = echartsModule as unknown as EChartsApi
      try {
        echartsRef.current?.dispose()
        const chart = echarts.init(el, themeMode === 'dark' ? 'dark' : null, {
          renderer: 'canvas',
        })
        chart.setOption(enhanceEChartsOption(parsed), { notMerge: true })
        echartsRef.current = chart
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e))
      }
    })
    return () => {
      cancelled = true
    }
  }, [source, engine, themeMode])

  useEffect(() => {
    if (source === null || engine !== 'markmap') return
    const svgEl = markmapSvgRef.current
    if (!svgEl) return
    let cancelled = false
    setError(null)
    void Promise.all([import('markmap-lib'), import('markmap-view')]).then(
      ([libModule, viewModule]) => {
        if (cancelled || !svgEl.isConnected) return
        try {
          const transformer = new libModule.Transformer()
          const { root: treeRoot } = transformer.transform(source)
          if (markmapRef.current) {
            markmapRef.current.setData(treeRoot)
            markmapRef.current.fit()
          } else {
            while (svgEl.firstChild) svgEl.removeChild(svgEl.firstChild)
            const mm = viewModule.Markmap.create(
              svgEl,
              { autoFit: true, duration: 200 },
              treeRoot,
            )
            markmapRef.current = mm as unknown as MarkmapHandle
          }
        } catch (e) {
          setError(e instanceof Error ? e.message : String(e))
        }
      },
    )
    return () => {
      cancelled = true
    }
  }, [source, engine])

  useEffect(() => {
    if (stageRef.current?.isConnected) {
      echartsRef.current?.resize()
    }
    if (markmapSvgRef.current?.isConnected && markmapRef.current) {
      try {
        markmapRef.current.fit()
      } catch {
        /* svg may be mid-detach during canvas close */
      }
    }
  }, [contentW, contentH])

  useEffect(
    () => () => {
      echartsRef.current?.dispose()
      echartsRef.current = null
      try {
        markmapRef.current?.destroy()
      } catch {
        /* listeners may already be gone */
      }
      markmapRef.current = null
    },
    [],
  )

  const exportPng = useCallback(async () => {
    try {
      const stem = stemOf(relPath)
      if (engine === 'echarts' && echartsRef.current) {
        downloadDataUrl(
          echartsRef.current.getDataURL({ pixelRatio: 2, backgroundColor: '#ffffff' }),
          `${stem}.png`,
        )
        return
      }
      let svgText: string | null = null
      if (engine === 'mermaid' && mermaidSvg) {
        svgText = mermaidSvg
      } else if (engine === 'markmap' && markmapSvgRef.current) {
        svgText = svgElementToString(markmapSvgRef.current)
      }
      if (!svgText) return
      downloadDataUrl(await svgStringToPngDataUrl(svgText), `${stem}.png`)
    } catch {
      addToast({ type: 'error', message: t('designer.diagram.exportFailed'), duration: 4000 })
    }
  }, [engine, mermaidSvg, relPath, addToast, t])

  const exportSvg = useCallback(() => {
    const stem = stemOf(relPath)
    let svgText: string | null = null
    if (engine === 'mermaid' && mermaidSvg) {
      svgText = mermaidSvg
    } else if (engine === 'markmap' && markmapSvgRef.current) {
      svgText = svgElementToString(markmapSvgRef.current)
    }
    if (!svgText) return
    const dataUrl = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svgText)}`
    downloadDataUrl(dataUrl, `${stem}.svg`)
  }, [engine, mermaidSvg, relPath])

  const copySource = useCallback(() => {
    if (source === null) return
    void navigator.clipboard
      .writeText(source)
      .then(() =>
        addToast({ type: 'success', message: t('designer.diagram.sourceCopied'), duration: 2500 }),
      )
      .catch(() => {
        /* clipboard unavailable */
      })
  }, [source, addToast, t])

  if (!engine) return null

  const stageH = Math.max(1, contentH - CONTROLS_H)

  return (
    <div className="flex h-full w-full flex-col bg-[var(--color-surface)]">
      <div
        className="relative min-h-0 flex-1 overflow-auto"
        style={{ height: stageH }}
      >
        {engine === 'mermaid' ? (
          mermaidSvg ? (
            <div
              className="flex h-full w-full items-center justify-center p-2 [&_svg]:max-h-full [&_svg]:max-w-full"
              dangerouslySetInnerHTML={{ __html: sanitizeSvg(mermaidSvg) }}
            />
          ) : (
            <div className="flex h-full items-center justify-center text-[11px] text-[var(--color-text-tertiary)]">
              …
            </div>
          )
        ) : engine === 'echarts' ? (
          <div ref={stageRef} className="h-full w-full" />
        ) : (
          <svg
            ref={markmapSvgRef}
            className="h-full w-full text-[var(--color-text-primary)]"
            style={{ color: 'var(--color-text-primary)' }}
          />
        )}
        {error && (
          <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 bg-[var(--color-surface)] px-4 text-center">
            <span className="material-symbols-outlined text-[20px] text-[var(--color-danger)]">
              error
            </span>
            <div className="text-[11px] font-medium text-[var(--color-danger)]">
              {t('designer.diagram.renderError')}
            </div>
            <pre className="max-h-[45%] max-w-full select-text overflow-auto whitespace-pre-wrap rounded border border-[var(--color-border)] bg-[var(--color-surface-secondary)] p-2 text-left text-[10px] text-[var(--color-text-secondary)]">
              {error}
            </pre>
          </div>
        )}
      </div>
      <div
        className="flex flex-shrink-0 items-center justify-end gap-0.5 border-t border-[var(--color-border)] bg-[var(--color-surface)] px-1.5"
        style={{ height: CONTROLS_H }}
      >
        <button
          type="button"
          onClick={copySource}
          title={t('designer.diagram.copySource')}
          className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
        >
          <span className="material-symbols-outlined text-[13px]">content_copy</span>
        </button>
        {(engine === 'mermaid' || engine === 'markmap') && (
          <button
            type="button"
            onClick={exportSvg}
            disabled={!!error}
            title={t('designer.diagram.exportSvg')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-30"
          >
            <span className="material-symbols-outlined text-[13px]">polyline</span>
          </button>
        )}
        <button
          type="button"
          onClick={() => void exportPng()}
          disabled={!!error}
          title={t('designer.diagram.exportPng')}
          className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-30"
        >
          <span className="material-symbols-outlined text-[13px]">image</span>
        </button>
      </div>
    </div>
  )
}
