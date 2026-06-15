// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { isTauriRuntime } from './desktopRuntime'
import { extractMermaidBlocks } from './mermaidDetect'

export { extractMermaidBlocks } from './mermaidDetect'

export type CuratorDiagram = {
  code: string
  png_base64: string
}

type SvgSize = { width: number; height: number }

function parseSvgSize(svg: string): SvgSize {
  const viewBox = svg.match(/viewBox="([^"]+)"/i)
  if (viewBox?.[1]) {
    const parts = viewBox[1].split(/[\s,]+/).map((v) => Number.parseFloat(v))
    if (parts.length === 4 && parts.every((v) => Number.isFinite(v))) {
      const w = parts[2] ?? 0
      const h = parts[3] ?? 0
      if (w > 0 && h > 0) return { width: w, height: h }
    }
  }
  const w = svg.match(/\bwidth="([0-9.]+)(?:px)?"/i)
  const h = svg.match(/\bheight="([0-9.]+)(?:px)?"/i)
  const width = w?.[1] ? Number.parseFloat(w[1]) : NaN
  const height = h?.[1] ? Number.parseFloat(h[1]) : NaN
  if (Number.isFinite(width) && Number.isFinite(height) && width > 0 && height > 0) {
    return { width, height }
  }
  return { width: 800, height: 600 }
}

async function svgToPngBase64(svg: string, scale = 2): Promise<string | null> {
  if (typeof document === 'undefined') return null
  const { width, height } = parseSvgSize(svg)
  const withNs = svg.includes('xmlns=')
    ? svg
    : svg.replace('<svg', '<svg xmlns="http://www.w3.org/2000/svg"')
  const blob = new Blob([withNs], { type: 'image/svg+xml;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  try {
    const img = new Image()
    img.width = width
    img.height = height
    await new Promise<void>((resolve, reject) => {
      img.onload = () => resolve()
      img.onerror = () => reject(new Error('svg image load failed'))
      img.src = url
    })
    const canvas = document.createElement('canvas')
    canvas.width = Math.max(1, Math.round(width * scale))
    canvas.height = Math.max(1, Math.round(height * scale))
    const ctx = canvas.getContext('2d')
    if (!ctx) return null
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, canvas.width, canvas.height)
    ctx.drawImage(img, 0, 0, canvas.width, canvas.height)
    const dataUrl = canvas.toDataURL('image/png')
    return dataUrl.split(',')[1] ?? null
  } catch (err) {
    console.warn('[mermaidToImage] svg->png failed', err)
    return null
  } finally {
    URL.revokeObjectURL(url)
  }
}

let mermaidInitialized = false
let mermaidIdCounter = 0

async function renderMermaidToPngBase64(code: string): Promise<string | null> {
  try {
    const mermaidMod = (await import('mermaid')) as unknown as {
      default: {
        initialize: (config: Record<string, unknown>) => void
        render: (id: string, code: string) => Promise<{ svg: string }>
      }
    }
    const mermaid = mermaidMod.default
    if (!mermaidInitialized) {
      mermaid.initialize({
        startOnLoad: false,
        theme: 'default',
        securityLevel: 'strict',
        suppressErrorRendering: true,
        htmlLabels: false,
        flowchart: { useMaxWidth: false, curve: 'basis' },
        class: { useMaxWidth: false },
        sequence: { useMaxWidth: false },
        state: { useMaxWidth: false },
      })
      mermaidInitialized = true
    }
    const id = `curator-mermaid-${++mermaidIdCounter}`
    const { svg } = await mermaid.render(id, code)
    return await svgToPngBase64(svg)
  } catch (err) {
    console.warn('[mermaidToImage] mermaid render failed', err)
    return null
  }
}

export async function renderCuratorDiagrams(markdown: string): Promise<CuratorDiagram[]> {
  const blocks = extractMermaidBlocks(markdown)
  if (blocks.length === 0) return []
  const out: CuratorDiagram[] = []
  for (const code of blocks) {
    const png = await renderMermaidToPngBase64(code)
    if (png) {
      out.push({ code, png_base64: png })
    }
  }
  return out
}

type InvokeFn = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>

let invokeRef: InvokeFn | null = null

async function ensureInvoke(): Promise<InvokeFn | null> {
  if (!isTauriRuntime()) return null
  if (invokeRef) return invokeRef
  const core = (await import(/* @vite-ignore */ '@tauri-apps/api/core')) as {
    invoke: InvokeFn
  }
  invokeRef = core.invoke
  return invokeRef
}

export async function regenerateCuratorDocxWithDiagrams(args: {
  finalMdPath: string
  template: string
  diagrams: CuratorDiagram[]
}): Promise<string | null> {
  const invoke = await ensureInvoke()
  if (!invoke) return null
  try {
    return await invoke<string>('curator_render_docx_with_diagrams', {
      finalMdPath: args.finalMdPath,
      template: args.template,
      diagrams: args.diagrams,
    })
  } catch (err) {
    console.warn('[mermaidToImage] regenerate docx failed', err)
    return null
  }
}
