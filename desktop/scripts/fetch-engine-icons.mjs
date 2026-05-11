// SPDX-License-Identifier: MIT
// One-shot downloader for the search engine and common-host favicons used
// by the WebSearch tool result UI.  Run with `bun desktop/scripts/fetch-engine-icons.mjs`
// or `node desktop/scripts/fetch-engine-icons.mjs`.  Files that already exist
// in `desktop/public/engine-icons/` are skipped, so the script is safe to
// re-run.

import { mkdir, writeFile, stat } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import path from 'node:path'

const targets = [
  { id: 'duckduckgo', file: 'duckduckgo.ico', url: 'https://duckduckgo.com/favicon.ico' },
  { id: 'brave', file: 'brave.png', url: 'https://brave.com/static-assets/images/brave-favicon.png' },
  { id: 'bing', file: 'bing.ico', url: 'https://www.bing.com/favicon.ico' },
  { id: 'baidu', file: 'baidu.ico', url: 'https://www.baidu.com/favicon.ico' },
  { id: 'csdn', file: 'csdn.ico', url: 'https://g.csdnimg.cn/static/logo/favicon32.ico' },
  { id: 'juejin', file: 'juejin.png', url: 'https://lf-web-assets.juejin.cn/obj/juejin-web/xitu_juejin_web/static/favicons/favicon-32x32.png' },
  { id: 'zhihu', file: 'zhihu.ico', url: 'https://static.zhihu.com/heifetz/favicon.ico' },
  { id: 'jina', file: 'jina.ico', url: 'https://jina.ai/favicon.ico' },
  { id: 'weixin', file: 'weixin.ico', url: 'https://res.wx.qq.com/a/wx_fed/assets/res/NTI4MWU5.ico' },
  { id: 'github', file: 'github.svg', url: 'https://github.githubassets.com/favicons/favicon.svg' },
  { id: 'arxiv', file: 'arxiv.png', url: 'https://static.arxiv.org/static/browse/0.3.4/images/icons/favicon-32x32.png' },
  { id: 'semanticscholar', file: 'semanticscholar.png', url: 'https://cdn.semanticscholar.org/d5a7fc2a8d2c90b9/img/favicon-32x32.png' },
  { id: 'dblp', file: 'dblp.png', url: 'https://dblp.org/img/dblp.icon.192x192.png' },
  { id: 'pubmed', file: 'pubmed.ico', url: 'https://www.ncbi.nlm.nih.gov/favicon.ico' },
  { id: 'googlescholar', file: 'googlescholar.ico', url: 'https://scholar.google.com/favicon.ico' },
  // Frequent hit-side hosts in mainland China search results
  { id: 'sogou', file: 'sogou.ico', url: 'https://www.sogou.com/favicon.ico' },
  { id: 'people', file: 'people.com.cn.ico', url: 'https://www.people.com.cn/favicon.ico' },
  { id: 'xinhuanet', file: 'xinhuanet.com.ico', url: 'https://www.xinhuanet.com/favicon.ico' },
]

const here = path.dirname(fileURLToPath(import.meta.url))
const outDir = path.resolve(here, '..', 'public', 'engine-icons')

async function exists(p) {
  try {
    const s = await stat(p)
    return s.isFile() && s.size > 0
  } catch {
    return false
  }
}

async function download(url) {
  const res = await fetch(url, {
    redirect: 'follow',
    headers: {
      'User-Agent':
        'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/127.0.0.0 Safari/537.36',
      Accept: 'image/avif,image/webp,*/*;q=0.8',
    },
  })
  if (!res.ok) throw new Error(`HTTP ${res.status} ${res.statusText}`)
  const buf = new Uint8Array(await res.arrayBuffer())
  if (buf.byteLength === 0) throw new Error('empty body')
  return buf
}

async function main() {
  await mkdir(outDir, { recursive: true })
  let okCount = 0
  let skipCount = 0
  const failures = []
  for (const t of targets) {
    const dest = path.join(outDir, t.file)
    if (await exists(dest)) {
      skipCount += 1
      continue
    }
    try {
      const bytes = await download(t.url)
      await writeFile(dest, bytes)
      console.log(`[ok] ${t.id} -> ${t.file} (${bytes.byteLength}B)`)
      okCount += 1
    } catch (err) {
      console.warn(`[warn] ${t.id} (${t.url}) failed: ${err.message || err}`)
      failures.push(t.id)
    }
  }
  console.log(
    `\nDone. downloaded=${okCount} skipped=${skipCount} failed=${failures.length}`,
  )
  if (failures.length) {
    console.log('failed ids:', failures.join(', '))
    process.exitCode = 1
  }
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
