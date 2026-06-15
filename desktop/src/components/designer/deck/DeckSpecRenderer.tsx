// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useTranslation } from '../../../i18n'
import { useSettingsStore } from '../../../stores/settingsStore'
import { workspaceFilesApi } from '../../../api/workspaceFiles'
import { designerApi } from '../../../api/designer'
import {
  DECK_THEME_OPTIONS,
  deckRenderPath,
  hexWithAlpha,
  parseDeckRenderModel,
  type DeckRenderBlock,
  type DeckRenderModel,
  type DeckRenderSlide,
  type DeckRenderTextBlock,
} from './deckRenderModel'

export type DeckBlockPick = {
  slideId: string
  blockId: string
  label: string
}

type Props = {
  root: string
  sessionId: string
  manifestRelPath: string
  refreshToken: number
  contentW: number
  contentH: number
  selectMode: boolean
  rawId: string | null
  onPickBlock: (pick: DeckBlockPick) => void
  onTitleResolved: (title: string) => void
}

const CONTROLS_H = 30
const THUMBS_H = 76

function blockLabel(block: DeckRenderBlock): string {
  if (block.kind === 'text') {
    const text = block.paragraphs
      .flatMap((p) => p.runs.map((r) => r.text))
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim()
    if (text) return text.length > 40 ? `${text.slice(0, 40)}…` : text
  }
  return block.id
}

function backgroundStyle(
  slide: DeckRenderSlide,
  assetUrl: (src: string) => string | null,
): React.CSSProperties {
  const bg = slide.background
  if (bg.kind === 'gradient') {
    return { background: `linear-gradient(${bg.angle}deg, ${bg.from}, ${bg.to})` }
  }
  if (bg.kind === 'image') {
    const url = assetUrl(bg.src)
    if (url) {
      return {
        backgroundImage: `url("${url}")`,
        backgroundSize: 'cover',
        backgroundPosition: 'center',
      }
    }
    return { backgroundColor: '#111111' }
  }
  return { backgroundColor: bg.color }
}

function TextBlockView({
  block,
  model,
}: {
  block: DeckRenderTextBlock
  model: DeckRenderModel
}) {
  const justify =
    block.valign === 'middle' ? 'center' : block.valign === 'bottom' ? 'flex-end' : 'flex-start'
  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        justifyContent: justify,
        overflow: 'hidden',
      }}
    >
      {block.paragraphs.map((para, pi) => {
        const runs = para.runs.map((run, ri) => (
          <span
            key={ri}
            style={{
              fontFamily:
                run.font === 'heading' ? model.fonts.headingCss : model.fonts.bodyCss,
              fontSize: `${run.size}px`,
              fontWeight: run.bold ? 700 : 400,
              fontStyle: run.italic ? 'italic' : 'normal',
              color: run.color,
              whiteSpace: 'pre-wrap',
              overflowWrap: 'anywhere',
            }}
          >
            {run.text}
          </span>
        ))
        const firstSize = para.runs[0]?.size ?? 24
        if (para.bullet) {
          return (
            <div
              key={pi}
              style={{
                display: 'flex',
                alignItems: 'baseline',
                gap: `${Math.round(firstSize * 0.45)}px`,
                marginTop: `${para.spaceBefore}px`,
                paddingLeft: `${para.level * firstSize * 1.4}px`,
                textAlign: block.align as React.CSSProperties['textAlign'],
                lineHeight: block.lineSpacing,
              }}
            >
              <span
                style={{
                  color: model.accent,
                  fontSize: `${Math.round(firstSize * 0.85)}px`,
                  fontFamily: model.fonts.bodyCss,
                  flexShrink: 0,
                }}
              >
                {para.bulletChar ?? '•'}
              </span>
              <span style={{ flex: 1, minWidth: 0 }}>{runs}</span>
            </div>
          )
        }
        return (
          <div
            key={pi}
            style={{
              marginTop: `${para.spaceBefore}px`,
              textAlign: block.align as React.CSSProperties['textAlign'],
              lineHeight: block.lineSpacing,
            }}
          >
            {runs}
          </div>
        )
      })}
    </div>
  )
}

function BlockView({
  block,
  model,
  assetUrl,
}: {
  block: DeckRenderBlock
  model: DeckRenderModel
  assetUrl: (src: string) => string | null
}) {
  if (block.kind === 'text') {
    return <TextBlockView block={block} model={model} />
  }
  if (block.kind === 'image') {
    const url = assetUrl(block.src)
    if (!url) return null
    return (
      <img
        src={url}
        alt=""
        draggable={false}
        style={{
          width: '100%',
          height: '100%',
          objectFit: block.fit === 'contain' ? 'contain' : 'cover',
          borderRadius: `${block.radius}px`,
          display: 'block',
        }}
      />
    )
  }
  if (block.kind === 'shape') {
    if (block.shape === 'line') {
      const horizontal = block.w >= block.h
      const stroke = block.stroke
      if (!stroke) return null
      const color = hexWithAlpha(stroke.color, stroke.alpha)
      return (
        <div
          style={{
            position: 'absolute',
            left: 0,
            top: horizontal ? `calc(50% - ${stroke.width / 2}px)` : 0,
            width: horizontal ? '100%' : `${stroke.width}px`,
            height: horizontal ? `${stroke.width}px` : '100%',
            backgroundColor: color,
          }}
        />
      )
    }
    const radius =
      block.shape === 'ellipse'
        ? '50%'
        : block.shape === 'roundRect' || block.radius > 0
          ? `${block.radius}px`
          : '0'
    return (
      <div
        style={{
          width: '100%',
          height: '100%',
          backgroundColor: block.fill
            ? hexWithAlpha(block.fill.color, block.fill.alpha)
            : 'transparent',
          border: block.stroke
            ? `${block.stroke.width}px solid ${hexWithAlpha(block.stroke.color, block.stroke.alpha)}`
            : 'none',
          borderRadius: radius,
          boxSizing: 'border-box',
        }}
      />
    )
  }
  if (block.kind === 'table') {
    const total = block.colFracs.reduce((a, b) => a + b, 0) || 1
    return (
      <table
        style={{
          width: '100%',
          height: '100%',
          borderCollapse: 'collapse',
          tableLayout: 'fixed',
          fontFamily: block.fontCss || model.fonts.bodyCss,
          fontSize: `${block.size}px`,
        }}
      >
        <colgroup>
          {block.colFracs.map((f, i) => (
            <col key={i} style={{ width: `${((f / total) * 100).toFixed(2)}%` }} />
          ))}
        </colgroup>
        <tbody>
          {block.rows.map((row, ri) => {
            const isHeader = block.headerRow && ri === 0
            return (
              <tr key={ri}>
                {row.map((cell, ci) => (
                  <td
                    key={ci}
                    style={{
                      padding: `${Math.round(block.size * 0.35)}px ${Math.round(block.size * 0.5)}px`,
                      backgroundColor: isHeader
                        ? block.headerFill
                        : hexWithAlpha(block.rowFill, 0.6),
                      color: isHeader ? block.headerText : block.textColor,
                      fontWeight: isHeader ? 700 : 400,
                      borderBottom: `1px solid ${hexWithAlpha(block.hairline, 0.7)}`,
                      verticalAlign: 'middle',
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                    }}
                  >
                    {cell}
                  </td>
                ))}
              </tr>
            )
          })}
        </tbody>
      </table>
    )
  }
  return null
}

function SlideView({
  slide,
  model,
  scale,
  assetUrl,
  selectMode,
  onPickBlock,
}: {
  slide: DeckRenderSlide
  model: DeckRenderModel
  scale: number
  assetUrl: (src: string) => string | null
  selectMode: boolean
  onPickBlock?: (pick: DeckBlockPick) => void
}) {
  return (
    <div
      onClick={
        selectMode && onPickBlock
          ? () => {
              onPickBlock({
                slideId: slide.id,
                blockId: '',
                label: slide.id,
              })
            }
          : undefined
      }
      style={{
        width: model.stageW * scale,
        height: model.stageH * scale,
        position: 'relative',
        overflow: 'hidden',
        flexShrink: 0,
        cursor: selectMode ? 'crosshair' : undefined,
      }}
    >
      <div
        style={{
          width: model.stageW,
          height: model.stageH,
          transform: `scale(${scale})`,
          transformOrigin: 'top left',
          position: 'absolute',
          left: 0,
          top: 0,
          ...backgroundStyle(slide, assetUrl),
        }}
      >
        {slide.blocks.map((block) => (
          <div
            key={block.id}
            onClick={
              selectMode && onPickBlock
                ? (e) => {
                    e.stopPropagation()
                    onPickBlock({
                      slideId: slide.id,
                      blockId: block.id,
                      label: blockLabel(block),
                    })
                  }
                : undefined
            }
            style={{
              position: 'absolute',
              left: block.x,
              top: block.y,
              width: block.w,
              height: block.h,
              cursor: selectMode ? 'crosshair' : undefined,
              outline: 'none',
            }}
            className={selectMode ? 'sen-deck-pickable' : undefined}
          >
            <BlockView block={block} model={model} assetUrl={assetUrl} />
          </div>
        ))}
      </div>
      {selectMode && (
        <style>{`.sen-deck-pickable:hover { box-shadow: inset 0 0 0 2px #3b82f6; background: rgba(59,130,246,.08); }`}</style>
      )}
    </div>
  )
}

export function DeckSpecRenderer({
  root,
  sessionId,
  manifestRelPath,
  refreshToken,
  contentW,
  contentH,
  selectMode,
  rawId,
  onPickBlock,
  onTitleResolved,
}: Props) {
  const t = useTranslation()
  const locale = useSettingsStore((s) => s.locale)
  const [model, setModel] = useState<DeckRenderModel | null>(null)
  const [loadFailed, setLoadFailed] = useState(false)
  const [index, setIndex] = useState(0)
  const [showThumbs, setShowThumbs] = useState(false)
  const [showNotes, setShowNotes] = useState(false)
  const [themeOpen, setThemeOpen] = useState(false)
  const [switchingTheme, setSwitchingTheme] = useState(false)
  const [isFullscreen, setIsFullscreen] = useState(false)
  const [viewport, setViewport] = useState<{ w: number; h: number }>({ w: 0, h: 0 })
  const themeMenuRef = useRef<HTMLDivElement | null>(null)
  const containerRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    const sync = () => {
      const active =
        !!document.fullscreenElement && document.fullscreenElement === containerRef.current
      setIsFullscreen(active)
      if (active) {
        setViewport({ w: window.innerWidth, h: window.innerHeight })
        containerRef.current?.focus()
      }
    }
    const onResize = () => {
      if (document.fullscreenElement === containerRef.current) {
        setViewport({ w: window.innerWidth, h: window.innerHeight })
      }
    }
    document.addEventListener('fullscreenchange', sync)
    window.addEventListener('resize', onResize)
    return () => {
      document.removeEventListener('fullscreenchange', sync)
      window.removeEventListener('resize', onResize)
    }
  }, [])

  const togglePresent = useCallback(() => {
    if (document.fullscreenElement === containerRef.current) {
      void document.exitFullscreen().catch(() => {})
    } else {
      void containerRef.current?.requestFullscreen().catch(() => {})
    }
  }, [])

  useEffect(() => {
    if (!themeOpen) return
    const onDoc = (e: PointerEvent) => {
      if (themeMenuRef.current && !themeMenuRef.current.contains(e.target as Node)) {
        setThemeOpen(false)
      }
    }
    document.addEventListener('pointerdown', onDoc, true)
    return () => document.removeEventListener('pointerdown', onDoc, true)
  }, [themeOpen])

  const switchTheme = useCallback(
    async (themeId: string) => {
      if (switchingTheme || !root) return
      setSwitchingTheme(true)
      try {
        const res = await workspaceFilesApi.readFile({ root, path: manifestRelPath })
        if (res.encoding !== 'utf8') return
        const manifest = JSON.parse(res.content) as Record<string, unknown>
        manifest.theme = themeId
        await workspaceFilesApi.writeFile({
          root,
          path: manifestRelPath,
          content: `${JSON.stringify(manifest, null, 2)}\n`,
        })
        await designerApi.lintArtifact(sessionId, manifestRelPath)
      } catch {
        /* compile feedback surfaces through the canvas refresh */
      } finally {
        setSwitchingTheme(false)
        setThemeOpen(false)
      }
    },
    [switchingTheme, root, manifestRelPath, sessionId],
  )

  useEffect(() => {
    if (!root) return
    let cancelled = false
    workspaceFilesApi
      .readFile({ root, path: deckRenderPath(manifestRelPath) })
      .then((res) => {
        if (cancelled) return
        const parsed = res.encoding === 'utf8' ? parseDeckRenderModel(res.content) : null
        if (parsed) {
          setModel(parsed)
          setLoadFailed(false)
        } else {
          setLoadFailed(true)
        }
      })
      .catch(() => {
        if (!cancelled) setLoadFailed(true)
      })
    return () => {
      cancelled = true
    }
  }, [root, manifestRelPath, refreshToken])

  useEffect(() => {
    if (model?.title) onTitleResolved(model.title)
  }, [model?.title, onTitleResolved])

  const slideCount = model?.slides.length ?? 0
  useEffect(() => {
    setIndex((prev) => Math.min(prev, Math.max(0, slideCount - 1)))
  }, [slideCount])

  const assetUrl = useCallback(
    (src: string): string | null => {
      if (!rawId || !src) return null
      return `${workspaceFilesApi.rawUrl(rawId, src)}${refreshToken ? `?v=${refreshToken}` : ''}`
    },
    [rawId, refreshToken],
  )

  const goTo = useCallback(
    (next: number) => {
      setIndex(Math.max(0, Math.min(next, Math.max(0, slideCount - 1))))
    },
    [slideCount],
  )

  const onKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === 'ArrowLeft') {
        e.preventDefault()
        goTo(index - 1)
      } else if (e.key === 'ArrowRight' || e.key === ' ') {
        e.preventDefault()
        goTo(index + 1)
      } else if (e.key === 'Home') {
        e.preventDefault()
        goTo(0)
      } else if (e.key === 'End') {
        e.preventDefault()
        goTo(slideCount - 1)
      }
    },
    [goTo, index, slideCount],
  )

  const hasNotes = useMemo(
    () => (model?.slides ?? []).some((s) => (s.notes ?? '').trim().length > 0),
    [model],
  )

  if (!model) {
    return (
      <div className="flex h-full w-full items-center justify-center bg-[var(--color-surface-secondary)] px-4 text-center text-[11px] text-[var(--color-text-tertiary)]">
        {loadFailed ? t('designer.deck.generating') : '…'}
      </div>
    )
  }
  if (slideCount === 0) {
    return (
      <div className="flex h-full w-full items-center justify-center px-4 text-center text-[11px] text-[var(--color-text-tertiary)]">
        {t('designer.deck.generating')}
      </div>
    )
  }

  const thumbsVisible = !isFullscreen && showThumbs && slideCount > 1
  const effW = isFullscreen && viewport.w > 0 ? viewport.w : contentW
  const effH = isFullscreen && viewport.h > 0 ? viewport.h : contentH
  const stageAreaH = Math.max(
    1,
    effH - (isFullscreen ? 0 : CONTROLS_H) - (thumbsVisible ? THUMBS_H : 0),
  )
  const scale = Math.min(effW / model.stageW, stageAreaH / model.stageH)
  const slide = model.slides[Math.min(index, slideCount - 1)] ?? model.slides[0]
  if (!slide) {
    return (
      <div className="flex h-full w-full items-center justify-center px-4 text-center text-[11px] text-[var(--color-text-tertiary)]">
        {t('designer.deck.generating')}
      </div>
    )
  }
  const thumbScale = (THUMBS_H - 20) / model.stageH
  const currentNotes = (slide.notes ?? '').trim()

  return (
    <div
      ref={containerRef}
      className={`flex h-full w-full flex-col outline-none ${
        isFullscreen ? 'bg-black' : 'bg-[var(--color-surface-secondary)]'
      }`}
      tabIndex={0}
      onKeyDown={onKeyDown}
    >
      <div
        className="relative flex min-h-0 flex-1 items-center justify-center overflow-hidden"
        style={{ height: stageAreaH }}
        onClick={isFullscreen && !selectMode ? () => goTo(index + 1) : undefined}
      >
        <SlideView
          slide={slide}
          model={model}
          scale={scale}
          assetUrl={assetUrl}
          selectMode={selectMode}
          onPickBlock={onPickBlock}
        />
        {showNotes && currentNotes && (
          <div className="absolute bottom-1 left-1 right-1 max-h-[45%] overflow-auto rounded-md border border-[var(--color-border)] bg-[var(--color-surface)]/95 p-2 text-[11px] leading-relaxed text-[var(--color-text-primary)] shadow-md">
            {currentNotes}
          </div>
        )}
        {isFullscreen && (
          <div className="pointer-events-none absolute bottom-3 right-4 rounded bg-black/45 px-2 py-0.5 text-[12px] tabular-nums text-white/80">
            {index + 1} / {slideCount}
          </div>
        )}
      </div>

      {thumbsVisible && (
        <div
          className="flex flex-shrink-0 items-center gap-1.5 overflow-x-auto border-t border-[var(--color-border)] bg-[var(--color-surface)] px-1.5"
          style={{ height: THUMBS_H }}
        >
          {model.slides.map((s, i) => (
            <button
              key={s.id}
              type="button"
              onClick={() => goTo(i)}
              title={`${i + 1} · ${s.id}`}
              className={`relative flex-shrink-0 overflow-hidden rounded border ${
                i === index
                  ? 'border-[var(--color-accent)] ring-1 ring-[var(--color-accent)]/50'
                  : 'border-[var(--color-border)] opacity-75 hover:opacity-100'
              }`}
            >
              <SlideView
                slide={s}
                model={model}
                scale={thumbScale}
                assetUrl={assetUrl}
                selectMode={false}
              />
              <span className="absolute bottom-0 right-0 rounded-tl bg-black/55 px-1 text-[9px] leading-3 text-white">
                {i + 1}
              </span>
            </button>
          ))}
        </div>
      )}

      {!isFullscreen && (
      <div
        className="flex flex-shrink-0 items-center justify-between border-t border-[var(--color-border)] bg-[var(--color-surface)] px-1.5"
        style={{ height: CONTROLS_H }}
      >
        <div className="flex items-center gap-0.5">
          <button
            type="button"
            disabled={index <= 0}
            onClick={() => goTo(index - 1)}
            title={t('designer.deck.prev')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-30"
          >
            <span className="material-symbols-outlined text-[14px]">chevron_left</span>
          </button>
          <span className="min-w-[44px] text-center text-[10px] tabular-nums text-[var(--color-text-secondary)]">
            {index + 1} / {slideCount}
          </span>
          <button
            type="button"
            disabled={index >= slideCount - 1}
            onClick={() => goTo(index + 1)}
            title={t('designer.deck.next')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-30"
          >
            <span className="material-symbols-outlined text-[14px]">chevron_right</span>
          </button>
        </div>
        <div className="flex items-center gap-0.5">
          <div className="relative" ref={themeMenuRef}>
            <button
              type="button"
              disabled={switchingTheme}
              onClick={() => setThemeOpen((v) => !v)}
              title={t('designer.deck.theme')}
              className={`flex h-5 w-5 items-center justify-center rounded ${
                themeOpen
                  ? 'bg-[var(--color-accent)] text-[var(--color-on-accent)]'
                  : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
              } disabled:opacity-50`}
            >
              <span
                className={`material-symbols-outlined text-[13px]${
                  switchingTheme ? ' animate-spin' : ''
                }`}
              >
                {switchingTheme ? 'progress_activity' : 'palette'}
              </span>
            </button>
            {themeOpen && (
              <div className="absolute bottom-full right-0 z-[9999] mb-1 max-h-[260px] w-[180px] overflow-y-auto rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] py-1 shadow-[var(--shadow-dropdown)]">
                {DECK_THEME_OPTIONS.map((opt) => (
                  <button
                    key={opt.id}
                    type="button"
                    onClick={() => void switchTheme(opt.id)}
                    className={`flex w-full items-center justify-between gap-2 px-2.5 py-1 text-left text-[11px] transition-colors ${
                      model.theme === opt.id
                        ? 'bg-[var(--color-surface-selected)] text-[var(--color-accent)]'
                        : 'text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]'
                    }`}
                  >
                    <span className="truncate">
                      {locale === 'zh' ? opt.labelZh : opt.labelEn}
                    </span>
                    {model.theme === opt.id && (
                      <span className="material-symbols-outlined text-[12px]">check</span>
                    )}
                  </button>
                ))}
              </div>
            )}
          </div>
          {hasNotes && (
            <button
              type="button"
              onClick={() => setShowNotes((v) => !v)}
              title={t('designer.deck.notes')}
              className={`flex h-5 w-5 items-center justify-center rounded ${
                showNotes
                  ? 'bg-[var(--color-accent)] text-[var(--color-on-accent)]'
                  : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
              }`}
            >
              <span className="material-symbols-outlined text-[13px]">sticky_note_2</span>
            </button>
          )}
          <button
            type="button"
            onClick={() => setShowThumbs((v) => !v)}
            title={t('designer.deck.thumbnails')}
            className={`flex h-5 w-5 items-center justify-center rounded ${
              thumbsVisible
                ? 'bg-[var(--color-accent)] text-[var(--color-on-accent)]'
                : 'text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]'
            }`}
          >
            <span className="material-symbols-outlined text-[13px]">grid_view</span>
          </button>
          <button
            type="button"
            onClick={togglePresent}
            title={t('designer.deck.present')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[13px]">slideshow</span>
          </button>
        </div>
      </div>
      )}
    </div>
  )
}
