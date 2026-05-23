// SPDX-License-Identifier: MIT

export const engineIcons = {
  duckduckgo: '/engine-icons/duckduckgo.svg',
  brave: '/engine-icons/brave.svg',
  bing: '/engine-icons/bing.ico',
  baidu: '/engine-icons/baidu.ico',
  csdn: '/engine-icons/csdn.ico',
  juejin: '/engine-icons/juejin.png',
  zhihu: '/engine-icons/zhihu.ico',
  jina: '/engine-icons/jina.svg',
  weixin: '/engine-icons/weixin.ico',
  wechat: '/engine-icons/weixin.ico',
  github: '/engine-icons/github.svg',
  arxiv: '/engine-icons/arxiv.png',
  semanticscholar: '/engine-icons/semanticscholar.png',
  dblp: '/engine-icons/dblp.png',
  pubmed: '/engine-icons/pubmed.ico',
  googlescholar: '/engine-icons/googlescholar.svg',
  searxng: '/engine-icons/duckduckgo.svg',
  sogou: '/engine-icons/sogou.ico',
  people: '/engine-icons/people.com.cn.ico',
  xinhuanet: '/engine-icons/xinhuanet.com.ico',
} as const

export type EngineId = keyof typeof engineIcons

export const engineLabels: Record<EngineId, string> = {
  duckduckgo: 'DuckDuckGo',
  brave: 'Brave',
  bing: 'Bing',
  baidu: 'Baidu',
  csdn: 'CSDN',
  juejin: '掘金',
  zhihu: '知乎',
  jina: 'Jina Reader',
  weixin: '微信',
  wechat: '微信',
  github: 'GitHub',
  arxiv: 'arXiv',
  semanticscholar: 'Semantic Scholar',
  dblp: 'DBLP',
  pubmed: 'PubMed',
  googlescholar: 'Google Scholar',
  searxng: 'SearXNG',
  sogou: 'Sogou',
  people: '人民网',
  xinhuanet: '新华网',
}

export function isEngineId(value: string | null | undefined): value is EngineId {
  if (!value) return false
  return Object.prototype.hasOwnProperty.call(engineIcons, value)
}

export function engineIconFor(id: string | null | undefined): string | null {
  if (!isEngineId(id)) return null
  return engineIcons[id]
}

export function engineLabelFor(id: string | null | undefined): string {
  if (!isEngineId(id)) return id ?? ''
  return engineLabels[id] ?? id
}

const HOST_TO_ENGINE_RAW: Record<string, EngineId> = {
  'duckduckgo.com': 'duckduckgo',
  'duckduckgo.io': 'duckduckgo',
  'brave.com': 'brave',
  'search.brave.com': 'brave',
  'bing.com': 'bing',
  'cn.bing.com': 'bing',
  'baidu.com': 'baidu',
  'tieba.baidu.com': 'baidu',
  'baike.baidu.com': 'baidu',
  'wenku.baidu.com': 'baidu',
  'csdn.net': 'csdn',
  'blog.csdn.net': 'csdn',
  'juejin.cn': 'juejin',
  'juejin.im': 'juejin',
  'zhihu.com': 'zhihu',
  'zhuanlan.zhihu.com': 'zhihu',
  'jina.ai': 'jina',
  'r.jina.ai': 'jina',
  'mp.weixin.qq.com': 'weixin',
  'weixin.qq.com': 'weixin',
  'github.com': 'github',
  'gist.github.com': 'github',
  'github.io': 'github',
  'arxiv.org': 'arxiv',
  'semanticscholar.org': 'semanticscholar',
  'dblp.org': 'dblp',
  'ncbi.nlm.nih.gov': 'pubmed',
  'pubmed.ncbi.nlm.nih.gov': 'pubmed',
  'scholar.google.com': 'googlescholar',
  'scholar.google.cn': 'googlescholar',
  'sogou.com': 'sogou',
  'www.sogou.com': 'sogou',
  'people.com.cn': 'people',
  'xinhuanet.com': 'xinhuanet',
  'news.cn': 'xinhuanet',
}

export const hostToEngine: Readonly<Record<string, EngineId>> = HOST_TO_ENGINE_RAW

function normalizeHost(host: string | null | undefined): string {
  if (!host) return ''
  return host.toLowerCase().replace(/^www\./i, '')
}

export function engineIdForHost(host: string | null | undefined): EngineId | null {
  const norm = normalizeHost(host)
  if (!norm) return null
  if (HOST_TO_ENGINE_RAW[norm]) return HOST_TO_ENGINE_RAW[norm]
  const parts = norm.split('.')
  for (let i = 1; i < parts.length - 1; i += 1) {
    const suffix = parts.slice(i).join('.')
    if (HOST_TO_ENGINE_RAW[suffix]) return HOST_TO_ENGINE_RAW[suffix]
  }
  return null
}

export function engineIconForHost(host: string | null | undefined): string | null {
  const id = engineIdForHost(host)
  return id ? engineIcons[id] : null
}
