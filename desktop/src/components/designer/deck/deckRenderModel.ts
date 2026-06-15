// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

export type DeckRenderRun = {
  text: string
  bold: boolean
  italic: boolean
  color: string
  size: number
  font: 'heading' | 'body' | string
}

export type DeckRenderParagraph = {
  bullet: boolean
  level: number
  bulletChar?: string
  spaceBefore: number
  runs: DeckRenderRun[]
}

export type DeckRenderTextBlock = {
  kind: 'text'
  id: string
  x: number
  y: number
  w: number
  h: number
  align: string
  valign: string
  lineSpacing: number
  paragraphs: DeckRenderParagraph[]
}

export type DeckRenderImageBlock = {
  kind: 'image'
  id: string
  x: number
  y: number
  w: number
  h: number
  src: string
  fit: 'cover' | 'contain' | string
  radius: number
}

export type DeckRenderPaint = { color: string; alpha: number }
export type DeckRenderStroke = { color: string; width: number; alpha: number }

export type DeckRenderShapeBlock = {
  kind: 'shape'
  id: string
  x: number
  y: number
  w: number
  h: number
  shape: 'rect' | 'roundRect' | 'ellipse' | 'line' | string
  radius: number
  fill?: DeckRenderPaint
  stroke?: DeckRenderStroke
}

export type DeckRenderTableBlock = {
  kind: 'table'
  id: string
  x: number
  y: number
  w: number
  h: number
  colFracs: number[]
  headerRow: boolean
  rows: string[][]
  size: number
  textColor: string
  headerFill: string
  headerText: string
  rowFill: string
  hairline: string
  fontCss: string
}

export type DeckRenderBlock =
  | DeckRenderTextBlock
  | DeckRenderImageBlock
  | DeckRenderShapeBlock
  | DeckRenderTableBlock

export type DeckRenderBackground =
  | { kind: 'color'; color: string }
  | { kind: 'gradient'; from: string; to: string; angle: number }
  | { kind: 'image'; src: string }

export type DeckRenderSlide = {
  id: string
  layout: string
  background: DeckRenderBackground
  notes?: string
  blocks: DeckRenderBlock[]
}

export type DeckRenderModel = {
  version: number
  title: string
  theme: string
  stageW: number
  stageH: number
  transition: string
  accent: string
  fonts: {
    headingLatin: string
    headingEa: string
    bodyLatin: string
    bodyEa: string
    headingCss: string
    bodyCss: string
  }
  slides: DeckRenderSlide[]
}

export const DECK_THEME_OPTIONS: { id: string; labelZh: string; labelEn: string }[] = [
  { id: 'business-simple', labelZh: '简约商务', labelEn: 'Business simple' },
  { id: 'tech-modern', labelZh: '现代科技', labelEn: 'Tech modern' },
  { id: 'academic-formal', labelZh: '严谨学术', labelEn: 'Academic formal' },
  { id: 'creative-fun', labelZh: '活泼创意', labelEn: 'Creative fun' },
  { id: 'minimalist-clean', labelZh: '极简清爽', labelEn: 'Minimalist clean' },
  { id: 'luxury-premium', labelZh: '高端奢华', labelEn: 'Luxury premium' },
  { id: 'nature-fresh', labelZh: '自然清新', labelEn: 'Nature fresh' },
  { id: 'gradient-vibrant', labelZh: '渐变活力', labelEn: 'Gradient vibrant' },
  { id: 'swiss-editorial', labelZh: '瑞士国际主义', labelEn: 'Swiss editorial' },
  { id: 'dark-keynote', labelZh: '暗色主题演讲', labelEn: 'Dark keynote' },
  { id: 'ink-wash', labelZh: '墨韵东方', labelEn: 'Ink wash' },
  { id: 'china-red', labelZh: '中国红', labelEn: 'China red' },
  { id: 'magazine-editorial', labelZh: '杂志编辑', labelEn: 'Magazine editorial' },
  { id: 'data-insight', labelZh: '数据洞察', labelEn: 'Data insight' },
  { id: 'sunset-warm', labelZh: '暮色暖阳', labelEn: 'Sunset warm' },
  { id: 'mono-noir', labelZh: '黑白极简', labelEn: 'Mono noir' },
  { id: 'bento-grid', labelZh: '便当栅格', labelEn: 'Bento grid' },
  { id: 'neo-brutalist', labelZh: '新粗野主义', labelEn: 'Neo brutalist' },
  { id: 'crimson-report', labelZh: '红韵报告', labelEn: 'Crimson report' },
  { id: 'teal-breeze', labelZh: '青澜清风', labelEn: 'Teal breeze' },
  { id: 'violet-haze', labelZh: '紫霭雅集', labelEn: 'Violet haze' },
  { id: 'morandi-duotone', labelZh: '莫兰迪双色', labelEn: 'Morandi duotone' },
  { id: 'jade-serif', labelZh: '松石宋韵', labelEn: 'Jade serif' },
  { id: 'cocoa-gold', labelZh: '可可鎏金', labelEn: 'Cocoa gold' },
  { id: 'scroll-antique', labelZh: '缃帙古卷', labelEn: 'Scroll antique' },
  { id: 'powder-azure', labelZh: '天青文苑', labelEn: 'Powder azure' },
]

export function deckDirOf(manifestRelPath: string): string {
  const idx = manifestRelPath.lastIndexOf('/')
  return idx === -1 ? '' : manifestRelPath.slice(0, idx)
}

export function deckRenderPath(manifestRelPath: string): string {
  const dir = deckDirOf(manifestRelPath)
  return dir ? `${dir}/render.json` : 'render.json'
}

export function deckPptxPath(manifestRelPath: string): string {
  const dir = deckDirOf(manifestRelPath)
  return dir ? `${dir}/deck.pptx` : 'deck.pptx'
}

export function hexWithAlpha(hex: string, alpha: number): string {
  const clean = hex.trim().replace('#', '')
  if (clean.length !== 6 || alpha >= 0.995) return `#${clean}`
  const r = parseInt(clean.slice(0, 2), 16)
  const g = parseInt(clean.slice(2, 4), 16)
  const b = parseInt(clean.slice(4, 6), 16)
  if ([r, g, b].some((v) => Number.isNaN(v))) return `#${clean}`
  return `rgba(${r}, ${g}, ${b}, ${Math.max(0, Math.min(1, alpha)).toFixed(3)})`
}

export function parseDeckRenderModel(raw: string): DeckRenderModel | null {
  try {
    const parsed = JSON.parse(raw) as DeckRenderModel
    if (!parsed || !Array.isArray(parsed.slides) || !parsed.stageW || !parsed.stageH) {
      return null
    }
    return parsed
  } catch {
    return null
  }
}
