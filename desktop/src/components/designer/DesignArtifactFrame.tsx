// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from '../../i18n'
import { useAnchoredDropdown } from '../../hooks/useAnchoredDropdown'
import { workspaceFilesApi } from '../../api/workspaceFiles'
import type { FileContent } from '../../types/workspaceFile'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { useUIStore } from '../../stores/uiStore'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import { revealInExplorer } from '../../lib/revealInExplorer'
import { joinWorkspaceAbsPath } from '../../lib/workspacePath'
import { DeckSpecRenderer } from './deck/DeckSpecRenderer'
import { deckPptxPath } from './deck/deckRenderModel'
import { ImageRegionSelector, type ImageRegionPick } from './image/ImageRegionSelector'
import { DiagramRenderer } from './diagram/DiagramRenderer'
import {
  DEVICE_PRESETS,
  unitDisplayName,
  type CanvasTweaks,
  type DesignUnit,
  type UnitDevice,
} from '../../stores/designerCanvasStore'

export const DESIGN_UNIT_DND_MIME = 'application/x-sen-design-unit'

const LIVE_ARTIFACT_STAGE_W = 1920
const LIVE_ARTIFACT_STAGE_H = 1080

export type ElementPick = {
  odId: string
  cssPath?: string
  label: string
  tag: string
  color?: string
  bg?: string
  fontSize?: string
  fontFamily?: string
}

type Props = {
  root: string
  sessionId: string
  unit: DesignUnit
  selected: boolean
  selectMode: boolean
  tweaks: CanvasTweaks
  viewportEl: HTMLElement | null
  refreshToken: number
  onSelect: (unitId: string) => void
  onEdit: (relPath: string) => void
  onDragStartUnit: (relPath: string) => void
  onSendToComposer: (relPath: string) => void
  onPickElement: (relPath: string, pick: ElementPick) => void
  onSetDevice: (unitId: string, device: UnitDevice) => void
  onLayoutDrag: (unitId: string, next: { x: number; y: number }) => void
  onTitleResolved: (unitId: string, title: string) => void
  onRename: (unitId: string, name: string) => void
  onNaturalSize: (unitId: string, naturalW: number, naturalH: number) => void
  zoom: number
}

function extractHtmlTitle(html: string): string | null {
  const titleMatch = html.match(/<title[^>]*>([\s\S]*?)<\/title>/i)
  const title = titleMatch?.[1]?.replace(/\s+/g, ' ').trim()
  if (title) return title
  const h1 = html.match(/<h1[^>]*>([\s\S]*?)<\/h1>/i)
  const heading = h1?.[1]?.replace(/<[^>]+>/g, '').replace(/\s+/g, ' ').trim()
  return heading || null
}

function extOf(path: string): string {
  const idx = path.lastIndexOf('.')
  return idx === -1 ? '' : path.slice(idx + 1).toLowerCase()
}

function dirOf(path: string): string {
  const idx = path.lastIndexOf('/')
  return idx === -1 ? '' : path.slice(0, idx + 1)
}

const IFRAME_SANDBOX = 'allow-scripts allow-forms allow-popups allow-modals allow-downloads'

const rawIdCache = new Map<string, Promise<string | null>>()

function resolveRawId(root: string): Promise<string | null> {
  if (!root) return Promise.resolve(null)
  let pending = rawIdCache.get(root)
  if (!pending) {
    pending = workspaceFilesApi
      .rawHandle({ root })
      .then((res) => res.rawId ?? null)
      .catch(() => {
        rawIdCache.delete(root)
        return null
      })
    rawIdCache.set(root, pending)
  }
  return pending
}

function useRawId(root: string): string | null {
  const [rawId, setRawId] = useState<string | null>(null)
  useEffect(() => {
    let cancelled = false
    setRawId(null)
    void resolveRawId(root).then((id) => {
      if (!cancelled) setRawId(id)
    })
    return () => {
      cancelled = true
    }
  }, [root])
  return rawId
}

const BRIDGE_SCRIPT = `<script>(function(){
  var selectMode=false;
  function applyTweaks(tw){
    if(!tw) return;var root=document.documentElement;
    if(tw.accent){root.style.setProperty('--od-accent',tw.accent);}else{root.style.removeProperty('--od-accent');}
    root.style.setProperty('--od-scale',String(tw.scale||1));
    root.style.setProperty('--od-density',String(tw.density||1));
    root.style.setProperty('--od-motion',String(tw.motion||1));
    if(tw.mode&&tw.mode!=='auto'){root.setAttribute('data-od-mode',tw.mode);}else{root.removeAttribute('data-od-mode');}
  }
  var hl=null;
  function ensureHl(){if(!hl){hl=document.createElement('div');hl.style.cssText='position:fixed;pointer-events:none;z-index:2147483646;border:2px solid #3b82f6;background:rgba(59,130,246,.12);border-radius:4px;display:none;transition:none';document.body.appendChild(hl);}return hl;}
  function placeHl(el){var h=ensureHl();var r=el.getBoundingClientRect();h.style.display='block';h.style.left=r.left+'px';h.style.top=r.top+'px';h.style.width=r.width+'px';h.style.height=r.height+'px';return h;}
  var MEANINGFUL=/^(section|header|footer|nav|main|article|aside|form|table|ul|ol|figure|dialog|fieldset|button|a|input|select|textarea|label|h1|h2|h3|h4|img|picture|video|audio|svg|canvas)$/;
  function annotated(el){var node=el;while(node&&node!==document.body){if(node.hasAttribute&&node.hasAttribute('data-od-id'))return node;node=node.parentElement;}return null;}
  function fallback(el){
    var node=el;
    while(node&&node!==document.body&&node.nodeType===1){
      var tag=node.tagName.toLowerCase();
      if(MEANINGFUL.test(tag))return node;
      var cs;try{cs=getComputedStyle(node);}catch(err){cs=null;}
      if(cs&&cs.display!=='inline'&&cs.display!=='contents')return node;
      node=node.parentElement;
    }
    return (el&&el.nodeType===1)?el:null;
  }
  function target(e){var el=e.target;if(!el||el.nodeType!==1)return null;if(hl&&el===hl)return null;return annotated(el)||fallback(el);}
  function cssPath(el){
    var parts=[];var cur=el;var depth=0;
    while(cur&&cur!==document.body&&cur.nodeType===1&&depth<8){
      var tag=cur.tagName.toLowerCase();
      if(cur.id){parts.unshift(tag+'#'+cur.id);return parts.join(' > ');}
      var idx=1;var sib=cur.previousElementSibling;
      while(sib){if(sib.tagName===cur.tagName)idx++;sib=sib.previousElementSibling;}
      parts.unshift(tag+':nth-of-type('+idx+')');
      cur=cur.parentElement;depth++;
    }
    return parts.join(' > ');
  }
  function labelFor(el){
    var l=el.getAttribute('data-od-label')||el.getAttribute('data-od-id')||el.getAttribute('aria-label');
    if(l)return l;
    var h=el.querySelector('h1,h2,h3,h4,legend,caption');
    var txt=((h?h.textContent:el.textContent)||'').replace(/\\s+/g,' ').trim();
    if(txt.length>40)txt=txt.slice(0,40)+'…';
    return txt||el.tagName.toLowerCase();
  }
  document.addEventListener('mousemove',function(e){if(!selectMode){if(hl)hl.style.display='none';return;}var el=target(e);if(!el){if(hl)hl.style.display='none';return;}placeHl(el);},true);
  document.addEventListener('click',function(e){
    if(!selectMode)return;
    e.preventDefault();e.stopPropagation();
    var el=target(e);if(!el)return;
    var h=placeHl(el);
    h.style.background='rgba(59,130,246,.32)';
    setTimeout(function(){if(hl)hl.style.background='rgba(59,130,246,.12)';},180);
    var cs=getComputedStyle(el);
    parent.postMessage({__od:1,t:'od:pick',odId:el.getAttribute('data-od-id')||'',cssPath:cssPath(el),label:labelFor(el),tag:el.tagName.toLowerCase(),color:cs.color,bg:cs.backgroundColor,fontSize:cs.fontSize,fontFamily:cs.fontFamily},'*');
  },true);
  window.addEventListener('message',function(e){var d=e.data||{};if(!d.__od)return;if(d.t==='od:tweaks'){applyTweaks(d.tweaks);}else if(d.t==='od:select-mode'){selectMode=!!d.on;if(!selectMode&&hl)hl.style.display='none';}});
  function postSize(){
    var de=document.documentElement;var b=document.body;
    var w=Math.max(de?de.scrollWidth:0,b?b.scrollWidth:0);
    var h=Math.max(de?de.scrollHeight:0,b?b.scrollHeight:0);
    if(w>0&&h>0){parent.postMessage({__od:1,t:'od:size',w:w,h:h},'*');}
  }
  window.addEventListener('load',function(){postSize();setTimeout(postSize,200);});
  window.addEventListener('resize',postSize);
  parent.postMessage({__od:1,t:'od:ready'},'*');
  postSize();
})();</script>`

function escapeAttr(value: string): string {
  return value
    .replace(/&/g, '&amp;')
    .replace(/"/g, '&quot;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
}

function buildSrcDoc(html: string, baseHref: string | null): string {
  const head = baseHref ? `<base href="${escapeAttr(baseHref)}">` : ''
  let out = html
  if (/<head[^>]*>/i.test(out)) {
    out = out.replace(/<head([^>]*)>/i, `<head$1>${head}`)
  } else if (/<html[^>]*>/i.test(out)) {
    out = out.replace(/<html([^>]*)>/i, `<html$1><head>${head}</head>`)
  } else {
    out = `${head}${out}`
  }
  if (/<\/body>/i.test(out)) {
    out = out.replace(/<\/body>/i, `${BRIDGE_SCRIPT}</body>`)
  } else {
    out = `${out}${BRIDGE_SCRIPT}`
  }
  return out
}

function HtmlBridgeFrame({
  doc,
  interactive,
  selectMode,
  contentW,
  contentH,
  deviceW,
  naturalW,
  naturalH,
  selfFit,
  scrollable,
  onReady,
  iframeRef,
}: {
  doc: string
  interactive: boolean
  selectMode: boolean
  contentW: number
  contentH: number
  deviceW: number | null
  naturalW: number | null
  naturalH: number | null
  selfFit: boolean
  scrollable: boolean
  onReady: () => void
  iframeRef: React.MutableRefObject<HTMLIFrameElement | null>
}) {
  const pointerEvents = interactive || selectMode ? 'auto' : 'none'
  if (selfFit) {
    const stageW = LIVE_ARTIFACT_STAGE_W
    const stageH = LIVE_ARTIFACT_STAGE_H
    const fit = Math.min(contentW / stageW, contentH / stageH)
    const visW = Math.round(stageW * fit)
    const visH = Math.round(stageH * fit)
    const left = Math.round((contentW - visW) / 2)
    const top = Math.round((contentH - visH) / 2)
    return (
      <div className="relative h-full w-full overflow-hidden bg-white">
        <div
          style={{
            position: 'absolute',
            left: `${left}px`,
            top: `${top}px`,
            width: `${visW}px`,
            height: `${visH}px`,
            overflow: 'hidden',
          }}
        >
          <iframe
            ref={iframeRef}
            title="design-unit-preview"
            sandbox={IFRAME_SANDBOX}
            srcDoc={doc}
            onLoad={onReady}
            style={{
              position: 'absolute',
              left: 0,
              top: 0,
              width: `${stageW}px`,
              height: `${stageH}px`,
              transform: `scale(${fit})`,
              transformOrigin: 'top left',
              pointerEvents,
            }}
            className="border-0 bg-white"
          />
        </div>
      </div>
    )
  }
  const logicalW = deviceW ?? (naturalW && naturalW > 0 ? naturalW : contentW)
  const scale = logicalW > 0 ? contentW / logicalW : 1
  const logicalH =
    naturalH && naturalH > 0 ? naturalH : scale > 0 ? contentH / scale : contentH
  const scaledW = Math.round(logicalW * scale)
  const scaledH = Math.round(logicalH * scale)
  const overflows = scaledH > contentH + 1
  const offsetY = overflows ? 0 : Math.round((contentH - scaledH) / 2)
  return (
    <div
      className={
        scrollable && overflows
          ? 'relative h-full w-full overflow-y-auto overflow-x-hidden bg-white'
          : 'relative h-full w-full overflow-hidden bg-white'
      }
    >
      <div
        style={{
          width: `${scaledW}px`,
          height: `${scaledH}px`,
          marginTop: `${offsetY}px`,
          position: 'relative',
          overflow: 'hidden',
        }}
      >
        <iframe
          ref={iframeRef}
          title="design-unit-preview"
          sandbox={IFRAME_SANDBOX}
          srcDoc={doc}
          onLoad={onReady}
          style={{
            position: 'absolute',
            left: 0,
            top: 0,
            width: `${logicalW}px`,
            height: `${logicalH}px`,
            transform: scale !== 1 ? `scale(${scale})` : undefined,
            transformOrigin: 'top left',
            pointerEvents,
          }}
          className="border-0 bg-white"
        />
      </div>
    </div>
  )
}

const DEVICE_ORDER: UnitDevice[] = ['auto', 'desktop', 'tablet', 'mobile']
const DEVICE_ICON: Record<UnitDevice, string> = {
  auto: 'fit_screen',
  desktop: 'desktop_windows',
  tablet: 'tablet_mac',
  mobile: 'smartphone',
}

export function DesignArtifactFrame({
  root,
  sessionId,
  unit,
  selected,
  selectMode,
  tweaks,
  viewportEl,
  refreshToken,
  onSelect,
  onEdit,
  onDragStartUnit,
  onSendToComposer,
  onPickElement,
  onSetDevice,
  onLayoutDrag,
  onTitleResolved,
  onRename,
  onNaturalSize,
  zoom,
}: Props) {
  const t = useTranslation()
  const frameRef = useRef<HTMLDivElement | null>(null)
  const iframeRef = useRef<HTMLIFrameElement | null>(null)
  const [inView, setInView] = useState(false)
  const [naturalSize, setNaturalSize] = useState<{ w: number; h: number } | null>(null)
  const [content, setContent] = useState<FileContent | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [renaming, setRenaming] = useState(false)
  const [nameDraft, setNameDraft] = useState('')
  const [deviceOpen, setDeviceOpen] = useState(false)
  const deviceMenu = useAnchoredDropdown<HTMLDivElement>(deviceOpen, () => setDeviceOpen(false), {
    align: 'right',
    estimatedHeight: 180,
  })
  const [fullscreen, setFullscreen] = useState(false)
  const fullscreenIframeRef = useRef<HTMLIFrameElement | null>(null)
  const rawId = useRawId(root)
  const addToast = useUIStore((s) => s.addToast)

  const isSvg = extOf(unit.relPath) === 'svg'
  const isHtml = unit.surface === 'html' && !isSvg
  const isDeckUnit = unit.surface === 'deck'
  const isDiagramUnit = unit.surface === 'diagram'
  const mediaSurface =
    unit.surface === 'image' || unit.surface === 'video' || unit.surface === 'audio'
  const rawPreviewable = mediaSurface || isSvg
  const needsContent =
    isDeckUnit || isDiagramUnit ? false : rawPreviewable ? !rawId : true

  useEffect(() => {
    const el = frameRef.current
    if (!el || !viewportEl) {
      setInView(true)
      return
    }
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setInView(true)
            observer.disconnect()
            break
          }
        }
      },
      { root: viewportEl, rootMargin: '400px' },
    )
    observer.observe(el)
    return () => observer.disconnect()
  }, [viewportEl])

  useEffect(() => {
    if (!inView || !root || !needsContent) return
    let cancelled = false
    setLoading(true)
    setError(null)
    workspaceFilesApi
      .readFile({ root, path: unit.relPath })
      .then((res) => {
        if (!cancelled) setContent(res)
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setError(err instanceof Error ? err.message : String(err))
          setContent(null)
        }
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [inView, root, unit.relPath, refreshToken, needsContent])

  const rawSrc = rawId
    ? workspaceFilesApi.rawUrl(rawId, unit.relPath, refreshToken || undefined)
    : null

  const dataUrl = useMemo(() => {
    if (!content) return null
    if (content.encoding === 'base64') {
      const mime = content.mimeType ?? 'application/octet-stream'
      return `data:${mime};base64,${content.content}`
    }
    if (isSvg && content.encoding === 'utf8') {
      return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(content.content)}`
    }
    return null
  }, [content, isSvg])

  const htmlTitle = useMemo(() => {
    if (!content || content.encoding !== 'utf8' || unit.surface !== 'html') return null
    return extractHtmlTitle(content.content)
  }, [content, unit.surface])

  useEffect(() => {
    if (htmlTitle) onTitleResolved(unit.id, htmlTitle)
  }, [htmlTitle, unit.id, onTitleResolved])

  const baseHref = useMemo(() => {
    if (!rawId) return null
    const dir = dirOf(unit.relPath)
    return workspaceFilesApi.rawUrl(rawId, dir)
  }, [rawId, unit.relPath])

  const srcDoc = useMemo(() => {
    if (!isHtml || content?.encoding !== 'utf8') return null
    return buildSrcDoc(content.content, baseHref)
  }, [isHtml, content, baseHref])

  useEffect(() => {
    const win = iframeRef.current?.contentWindow
    if (!win) return
    win.postMessage({ __od: 1, t: 'od:select-mode', on: selectMode }, '*')
  }, [selectMode, srcDoc])

  useEffect(() => {
    const win = iframeRef.current?.contentWindow
    if (!win) return
    win.postMessage({ __od: 1, t: 'od:tweaks', tweaks }, '*')
  }, [tweaks, srcDoc])

  useEffect(() => {
    const onMessage = (event: MessageEvent) => {
      const data = event.data as { __od?: number; t?: string } | null
      if (!data || data.__od !== 1) return
      if (event.source !== iframeRef.current?.contentWindow) return
      if (data.t === 'od:ready') {
        const win = iframeRef.current?.contentWindow
        win?.postMessage({ __od: 1, t: 'od:tweaks', tweaks }, '*')
        win?.postMessage({ __od: 1, t: 'od:select-mode', on: selectMode }, '*')
      } else if (data.t === 'od:pick') {
        const pick = data as unknown as ElementPick
        onPickElement(unit.relPath, pick)
      } else if (data.t === 'od:size') {
        const size = data as unknown as { w?: number; h?: number }
        const w = typeof size.w === 'number' && Number.isFinite(size.w) ? size.w : 0
        const h = typeof size.h === 'number' && Number.isFinite(size.h) ? size.h : 0
        if (w > 0 && h > 0) {
          setNaturalSize((prev) => (prev && prev.w === w && prev.h === h ? prev : { w, h }))
          onNaturalSize(unit.id, w, h)
        }
      }
    }
    window.addEventListener('message', onMessage)
    return () => window.removeEventListener('message', onMessage)
  }, [tweaks, selectMode, unit.relPath, unit.id, onPickElement, onNaturalSize])

  const name = unitDisplayName(unit)
  const ext = extOf(unit.relPath)

  const canSaveMedia = mediaSurface || isSvg

  const handleSaveMedia = async () => {
    if (isTauriRuntime()) {
      const absPath = joinWorkspaceAbsPath(root, unit.relPath)
      try {
        await revealInExplorer(absPath)
        return
      } catch {
      }
    }
    const src = rawSrc ?? dataUrl
    if (!src) return
    const anchor = document.createElement('a')
    anchor.href = src
    anchor.download = unit.relPath.split('/').pop() ?? 'asset'
    document.body.appendChild(anchor)
    anchor.click()
    anchor.remove()
  }

  const handleOpenPptx = async () => {
    const pptxRel = deckPptxPath(unit.relPath)
    if (isTauriRuntime()) {
      try {
        await revealInExplorer(joinWorkspaceAbsPath(root, pptxRel))
        return
      } catch {
        addToast({
          type: 'error',
          message: t('designer.deck.openPptxFailed'),
          duration: 6000,
        })
        return
      }
    }
    if (!rawId) {
      addToast({
        type: 'error',
        message: t('designer.deck.openPptxFailed'),
        duration: 6000,
      })
      return
    }
    const anchor = document.createElement('a')
    anchor.href = workspaceFilesApi.rawUrl(rawId, pptxRel)
    anchor.download = pptxRel.split('/').pop() ?? 'deck.pptx'
    document.body.appendChild(anchor)
    anchor.click()
    anchor.remove()
  }

  const commitRename = () => {
    setRenaming(false)
    onRename(unit.id, nameDraft)
  }

  const onHeaderPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    event.stopPropagation()
    onSelect(unit.id)
    const handle = event.currentTarget
    const pointerId = event.pointerId
    try {
      window.getSelection()?.removeAllRanges()
    } catch {
    }
    const startX = event.clientX
    const startY = event.clientY
    const originX = unit.x
    const originY = unit.y
    const z = zoom > 0 ? zoom : 1
    let moved = false
    let prevUserSelect = ''
    const onMove = (ev: PointerEvent) => {
      const dx = (ev.clientX - startX) / z
      const dy = (ev.clientY - startY) / z
      if (!moved && (Math.abs(dx) > 2 || Math.abs(dy) > 2)) {
        moved = true
        prevUserSelect = document.body.style.userSelect
        document.body.style.userSelect = 'none'
        try {
          handle.setPointerCapture(pointerId)
        } catch {
        }
      }
      if (moved) onLayoutDrag(unit.id, { x: originX + dx, y: originY + dy })
    }
    const onUp = () => {
      if (moved) {
        document.body.style.userSelect = prevUserSelect
        try {
          handle.releasePointerCapture(pointerId)
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
  }

  const deviceW =
    unit.device && unit.device !== 'auto' ? DEVICE_PRESETS[unit.device].w : null
  const contentW = unit.width
  const contentH = Math.max(1, unit.height - 28)

  const renderPreviewContent = (params: {
    width: number
    height: number
    interactive: boolean
    selectModeOn: boolean
    htmlIframeRef: React.MutableRefObject<HTMLIFrameElement | null>
    allowPick: boolean
    scrollable?: boolean
  }) => {
    const {
      width,
      height,
      interactive,
      selectModeOn,
      htmlIframeRef,
      allowPick,
      scrollable = false,
    } = params
    if (isDeckUnit) {
      return (
        <DeckSpecRenderer
          root={root}
          sessionId={sessionId}
          manifestRelPath={unit.relPath}
          refreshToken={refreshToken}
          contentW={width}
          contentH={height}
          selectMode={allowPick && selectModeOn}
          rawId={rawId}
          onPickBlock={(pick) => {
            if (!allowPick) return
            onPickElement(unit.relPath, {
              odId: `deck:${pick.slideId}:${pick.blockId}`,
              label: pick.label,
              tag: 'deck',
            })
          }}
          onTitleResolved={(title) => onTitleResolved(unit.id, title)}
        />
      )
    }
    if (isDiagramUnit) {
      return (
        <DiagramRenderer
          root={root}
          relPath={unit.relPath}
          refreshToken={refreshToken}
          contentW={width}
          contentH={height}
          onTitleResolved={(title) => onTitleResolved(unit.id, title)}
        />
      )
    }
    if (isHtml && srcDoc) {
      return (
        <HtmlBridgeFrame
          doc={srcDoc}
          interactive={interactive}
          selectMode={allowPick && selectModeOn}
          contentW={width}
          contentH={height}
          deviceW={deviceW}
          naturalW={naturalSize?.w ?? null}
          naturalH={naturalSize?.h ?? null}
          selfFit={unit.submode === 'live-artifact'}
          scrollable={scrollable}
          onReady={() => {
            const win = htmlIframeRef.current?.contentWindow
            win?.postMessage({ __od: 1, t: 'od:tweaks', tweaks }, '*')
            win?.postMessage(
              { __od: 1, t: 'od:select-mode', on: allowPick && selectModeOn },
              '*',
            )
          }}
          iframeRef={htmlIframeRef}
        />
      )
    }
    if (rawSrc || content) {
      return (
        <MediaBody
          ext={ext}
          surface={isSvg ? 'image' : unit.surface}
          content={content}
          dataUrl={dataUrl}
          rawSrc={rawSrc}
          loadFailedText={t('designer.canvas.mediaLoadFailed')}
          regionSelect={
            allowPick && unit.surface === 'image' && !isSvg
              ? {
                  root,
                  relPath: unit.relPath,
                  selectMode: selectModeOn,
                  onPicked: (pick: ImageRegionPick) =>
                    onPickElement(unit.relPath, {
                      odId: pick.odId,
                      label: pick.label,
                      tag: 'image',
                    }),
                }
              : null
          }
        />
      )
    }
    return null
  }

  return (
    <div
      ref={frameRef}
      data-testid="design-unit"
      className={`absolute flex flex-col overflow-hidden rounded-lg border bg-[var(--color-surface)] shadow-sm transition-shadow ${
        selected
          ? 'border-[var(--color-accent)] shadow-md ring-2 ring-[var(--color-accent)]/40'
          : 'border-[var(--color-border)] hover:shadow-md'
      }`}
      style={{ left: unit.x, top: unit.y, width: unit.width, height: unit.height }}
      onPointerDown={(event) => {
        event.stopPropagation()
        onSelect(unit.id)
      }}
    >
      <div
        className="flex h-7 flex-shrink-0 cursor-grab select-none items-center justify-between gap-1 border-b border-[var(--color-border)] bg-[var(--color-surface-secondary)] px-2 active:cursor-grabbing"
        title={unit.relPath}
        onPointerDown={onHeaderPointerDown}
      >
        <div className="flex min-w-0 items-center gap-1">
          <span
            className="material-symbols-outlined text-[13px] text-[var(--color-text-tertiary)]"
            title={t('designer.canvas.moveUnit')}
          >
            drag_indicator
          </span>
          {renaming ? (
            <input
              autoFocus
              value={nameDraft}
              onChange={(e) => setNameDraft(e.target.value)}
              onBlur={commitRename}
              onKeyDown={(e) => {
                if (e.key === 'Enter') commitRename()
                else if (e.key === 'Escape') setRenaming(false)
              }}
              onPointerDown={(e) => e.stopPropagation()}
              onClick={(e) => e.stopPropagation()}
              className="min-w-0 flex-1 select-text rounded border border-[var(--color-accent)] bg-[var(--color-surface)] px-1 py-0 text-[11px] text-[var(--color-text-primary)] outline-none"
            />
          ) : (
            <span
              className="cursor-text truncate text-[11px] font-medium text-[var(--color-text-secondary)]"
              title={`${name}\n${unit.relPath}`}
              onDoubleClick={(e) => {
                e.stopPropagation()
                setNameDraft(unit.customName ?? name)
                setRenaming(true)
              }}
            >
              {name}
            </span>
          )}
        </div>
        <div className="flex flex-shrink-0 items-center gap-1.5">
          {isDeckUnit && (
            <button
              type="button"
              onPointerDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation()
                void handleOpenPptx()
              }}
              title={t('designer.deck.openPptx')}
              className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-accent)]"
            >
              <span className="material-symbols-outlined text-[13px]">co_present</span>
            </button>
          )}
          {canSaveMedia && (
            <button
              type="button"
              onPointerDown={(e) => e.stopPropagation()}
              onClick={(e) => {
                e.stopPropagation()
                void handleSaveMedia()
              }}
              title={
                isTauriRuntime()
                  ? t('files.tree.reveal')
                  : t('designer.canvas.downloadFile')
              }
              className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-accent)]"
            >
              <span className="material-symbols-outlined text-[13px]">download</span>
            </button>
          )}
          {isHtml && (
            <div className="relative" ref={deviceMenu.triggerRef}>
              <button
                type="button"
                onPointerDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation()
                  setDeviceOpen((v) => !v)
                }}
                title={t('designer.canvas.device')}
                className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
              >
                <span className="material-symbols-outlined text-[13px]">
                  {DEVICE_ICON[unit.device ?? 'auto']}
                </span>
              </button>
              {deviceOpen && deviceMenu.style && createPortal(
                <div
                  ref={deviceMenu.menuRef}
                  style={deviceMenu.style}
                  className="w-[120px] rounded-lg border border-[var(--color-border)] bg-[var(--color-surface-container-lowest)] py-1 shadow-[var(--shadow-dropdown)]"
                  onPointerDown={(e) => e.stopPropagation()}
                >
                  {DEVICE_ORDER.map((d) => (
                    <button
                      key={d}
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation()
                        onSetDevice(unit.id, d)
                        setDeviceOpen(false)
                      }}
                      className={`flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-[12px] transition-colors ${
                        (unit.device ?? 'auto') === d
                          ? 'bg-[var(--color-surface-selected)] text-[var(--color-accent)]'
                          : 'text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]'
                      }`}
                    >
                      <span className="material-symbols-outlined text-[14px]">
                        {DEVICE_ICON[d]}
                      </span>
                      <span>
                        {d === 'auto'
                          ? t('designer.canvas.deviceAuto')
                          : DEVICE_PRESETS[d].label}
                      </span>
                    </button>
                  ))}
                </div>,
                deviceMenu.portalTarget,
              )}
            </div>
          )}
          <button
            type="button"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation()
              setFullscreen(true)
            }}
            title={t('designer.canvas.fullscreenPreview')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-accent)]"
          >
            <span className="material-symbols-outlined text-[13px]">fullscreen</span>
          </button>
          <button
            type="button"
            draggable
            onPointerDown={(e) => e.stopPropagation()}
            onDragStart={(event) => {
              const payload = JSON.stringify({ relPath: unit.relPath, sessionId })
              event.dataTransfer.setData(DESIGN_UNIT_DND_MIME, payload)
              event.dataTransfer.setData('text/plain', unit.relPath)
              event.dataTransfer.effectAllowed = 'copy'
              onDragStartUnit(unit.relPath)
            }}
            onClick={(e) => {
              e.stopPropagation()
              onSendToComposer(unit.relPath)
            }}
            title={t('designer.canvas.sendToComposer')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-accent)]"
          >
            <span className="material-symbols-outlined text-[13px]">north_east</span>
          </button>
          <button
            type="button"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation()
              setNameDraft(unit.customName ?? name)
              setRenaming(true)
            }}
            title={t('designer.canvas.renameUnit')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[13px]">edit_note</span>
          </button>
          <button
            type="button"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={(e) => {
              e.stopPropagation()
              onEdit(unit.relPath)
            }}
            title={t('designer.canvas.editUnit')}
            className="flex h-5 w-5 items-center justify-center rounded text-[var(--color-text-tertiary)] hover:bg-[var(--color-surface-hover)] hover:text-[var(--color-text-primary)]"
          >
            <span className="material-symbols-outlined text-[13px]">edit</span>
          </button>
        </div>
      </div>

      <div className="relative min-h-0 flex-1 overflow-hidden bg-white">
        {loading && !srcDoc && !rawSrc && !content && (
          <div className="flex h-full items-center justify-center text-[11px] text-[var(--color-text-tertiary)]">
            …
          </div>
        )}
        {!loading && error && !content && (
          <div className="flex h-full items-center justify-center px-3 text-center text-[11px] text-[var(--color-danger)]">
            {error}
          </div>
        )}
        {renderPreviewContent({
          width: contentW,
          height: contentH,
          interactive: selected,
          selectModeOn: selectMode,
          htmlIframeRef: iframeRef,
          allowPick: true,
        })}
      </div>
      {fullscreen &&
        createPortal(
          <FullscreenPreviewOverlay
            title={name}
            closeLabel={t('designer.canvas.exitFullscreen')}
            onClose={() => setFullscreen(false)}
            renderBody={(w, h) =>
              renderPreviewContent({
                width: w,
                height: h,
                interactive: true,
                selectModeOn: false,
                htmlIframeRef: fullscreenIframeRef,
                allowPick: false,
                scrollable: true,
              })
            }
          />,
          document.body,
        )}
    </div>
  )
}

function FullscreenPreviewOverlay({
  title,
  closeLabel,
  onClose,
  renderBody,
}: {
  title: string
  closeLabel: string
  onClose: () => void
  renderBody: (width: number, height: number) => React.ReactNode
}) {
  const bodyRef = useRef<HTMLDivElement | null>(null)
  const [size, setSize] = useState<{ w: number; h: number } | null>(null)

  useEffect(() => {
    const el = bodyRef.current
    if (!el) return
    const update = () => setSize({ w: el.clientWidth, h: el.clientHeight })
    update()
    const observer = new ResizeObserver(update)
    observer.observe(el)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', onKey)
    const prevOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    return () => {
      document.removeEventListener('keydown', onKey)
      document.body.style.overflow = prevOverflow
    }
  }, [onClose])

  return (
    <div
      className="fixed inset-0 z-[10000] flex flex-col bg-black/80 backdrop-blur-sm"
      onPointerDown={(e) => {
        if (e.target === e.currentTarget) onClose()
      }}
    >
      <div className="flex flex-shrink-0 items-center justify-between gap-2 px-4 py-2">
        <span className="truncate text-[13px] font-medium text-white/90" title={title}>
          {title}
        </span>
        <button
          type="button"
          onClick={onClose}
          title={closeLabel}
          className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-full bg-white/10 text-white transition-colors hover:bg-white/20"
        >
          <span className="material-symbols-outlined text-[18px]">close</span>
        </button>
      </div>
      <div className="relative mx-4 mb-4 flex-1 overflow-hidden rounded-lg bg-white shadow-2xl">
        <div ref={bodyRef} className="absolute inset-0">
          {size && renderBody(size.w, size.h)}
        </div>
      </div>
    </div>
  )
}

function MediaBody({
  ext,
  surface,
  content,
  dataUrl,
  rawSrc,
  loadFailedText,
  regionSelect,
}: {
  ext: string
  surface: DesignUnit['surface']
  content: FileContent | null
  dataUrl: string | null
  rawSrc: string | null
  loadFailedText: string
  regionSelect?: {
    root: string
    relPath: string
    selectMode: boolean
    onPicked: (pick: ImageRegionPick) => void
  } | null
}) {
  const [failedSrc, setFailedSrc] = useState<string | null>(null)
  const preferred = rawSrc ?? dataUrl
  const src =
    preferred && failedSrc === preferred && dataUrl && dataUrl !== preferred
      ? dataUrl
      : preferred
  const failed = src !== null && failedSrc === src
  const markFailed = () => setFailedSrc(src)

  if (failed) {
    return (
      <div className="flex h-full items-center justify-center px-3 text-center text-[11px] text-[var(--color-danger)]">
        {loadFailedText}
      </div>
    )
  }
  if (surface === 'image') {
    if (src) {
      if (regionSelect) {
        return (
          <ImageRegionSelector
            root={regionSelect.root}
            relPath={regionSelect.relPath}
            src={src}
            selectMode={regionSelect.selectMode}
            onPicked={regionSelect.onPicked}
            onLoadFailed={markFailed}
          />
        )
      }
      return (
        <div className="flex h-full items-center justify-center bg-[var(--color-surface)] p-2">
          <img
            src={src}
            alt=""
            onError={markFailed}
            className="max-h-full max-w-full object-contain"
          />
        </div>
      )
    }
  }
  if (surface === 'video') {
    if (src) {
      return (
        <div className="flex h-full items-center justify-center bg-black p-1">
          <video src={src} controls onError={markFailed} className="max-h-full max-w-full" />
        </div>
      )
    }
  }
  if (surface === 'audio') {
    if (src) {
      return (
        <div className="flex h-full items-center justify-center bg-[var(--color-surface)] p-3">
          <audio src={src} controls onError={markFailed} className="w-full" />
        </div>
      )
    }
  }
  if ((ext === 'md' || ext === 'markdown') && content?.encoding === 'utf8') {
    return (
      <div className="h-full overflow-auto p-3">
        <MarkdownRenderer content={content.content} variant="document" />
      </div>
    )
  }
  if (!content) return null
  return (
    <pre className="h-full overflow-auto whitespace-pre-wrap p-2 text-[10px] text-[var(--color-text-secondary)]">
      {content.encoding === 'utf8' ? content.content : '[binary]'}
    </pre>
  )
}
