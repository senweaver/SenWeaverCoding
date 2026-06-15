// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { workspaceFilesApi } from '../api/workspaceFiles'

const PRINT_W = 1024
const PRINT_H = 1448

export type MergedPrintUnit = {
  relPath: string
}

function escapeAttr(value: string): string {
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;')
}

function dirOf(path: string): string {
  const idx = path.lastIndexOf('/')
  return idx === -1 ? '' : path.slice(0, idx + 1)
}

function withBase(html: string, baseHref: string | null): string {
  if (!baseHref) return html
  const tag = `<base href="${escapeAttr(baseHref)}">`
  if (/<head[^>]*>/i.test(html)) {
    return html.replace(/<head([^>]*)>/i, `<head$1>${tag}`)
  }
  if (/<html[^>]*>/i.test(html)) {
    return html.replace(/<html([^>]*)>/i, `<html$1><head>${tag}</head>`)
  }
  return `${tag}${html}`
}

const HOST_SCRIPT = `(function(){
  var frames=Array.prototype.slice.call(document.querySelectorAll('.od-frame'));
  var pending=frames.length;var done=false;
  function size(f){try{var d=f.contentDocument;if(d){var b=d.body?d.body.scrollHeight:0;var h=Math.max(d.documentElement.scrollHeight,b);f.style.height=((h||600)+2)+'px';}}catch(e){f.style.height='600px';}}
  function fire(){try{window.focus();}catch(e){}window.print();}
  function ready(){if(done)return;done=true;setTimeout(function(){frames.forEach(size);setTimeout(fire,60);},250);}
  if(frames.length===0){ready();return;}
  frames.forEach(function(f){f.addEventListener('load',function(){size(f);pending--;if(pending<=0)ready();});});
  setTimeout(function(){frames.forEach(size);ready();},2500);
  window.addEventListener('afterprint',function(){parent.postMessage({__odprint:1,t:'done'},'*');});
})();`

function buildHostDoc(pages: string): string {
  return (
    `<!doctype html><html><head><meta charset="utf-8"><style>` +
    `@page{size:${PRINT_W}px ${PRINT_H}px;margin:0}` +
    `*{box-sizing:border-box}` +
    `html,body{margin:0;padding:0;background:#fff}` +
    `.od-page{width:${PRINT_W}px;page-break-after:always;break-after:page}` +
    `.od-page:last-child{page-break-after:auto;break-after:auto}` +
    `.od-frame{display:block;width:${PRINT_W}px;border:0;background:#fff}` +
    `</style></head><body>${pages}<script>${HOST_SCRIPT}</script></body></html>`
  )
}

export async function printUnitsMerged(opts: {
  root: string
  rawId: string | null
  units: MergedPrintUnit[]
  readHtml: (relPath: string) => Promise<string | null>
}): Promise<number> {
  const built: string[] = []
  for (const unit of opts.units) {
    const html = await opts.readHtml(unit.relPath)
    if (html == null) continue
    const baseHref = opts.rawId
      ? workspaceFilesApi.rawUrl(opts.rawId, dirOf(unit.relPath))
      : null
    const doc = withBase(html, baseHref)
    built.push(
      `<div class="od-page"><iframe class="od-frame" sandbox="allow-same-origin allow-scripts" srcdoc="${escapeAttr(
        doc,
      )}"></iframe></div>`,
    )
  }
  if (built.length === 0) return 0

  const hostDoc = buildHostDoc(built.join(''))

  await new Promise<void>((resolve) => {
    const host = document.createElement('iframe')
    host.setAttribute('aria-hidden', 'true')
    host.style.cssText = `position:fixed;left:-10000px;top:0;width:${PRINT_W}px;height:10px;border:0;opacity:0;pointer-events:none;`
    let settled = false
    const finish = () => {
      if (settled) return
      settled = true
      window.removeEventListener('message', onMessage)
      window.clearTimeout(safety)
      setTimeout(() => host.remove(), 200)
      resolve()
    }
    const onMessage = (event: MessageEvent) => {
      const data = event.data as { __odprint?: number } | null
      if (data && data.__odprint) finish()
    }
    const safety = window.setTimeout(finish, 60000)
    window.addEventListener('message', onMessage)
    document.body.appendChild(host)
    const cd = host.contentDocument
    if (!cd) {
      finish()
      return
    }
    cd.open()
    cd.write(hostDoc)
    cd.close()
  })

  return built.length
}
