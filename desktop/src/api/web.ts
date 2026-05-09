import { api } from './client'

export type WebSearchConfig = {
  enabled: boolean
  provider: string
  braveApiKey?: string | null
  searxngInstanceUrl?: string | null
  tavilyApiKey?: string | null
  exaApiKey?: string | null
  maxResults: number
  timeoutSecs: number
}

export type FirecrawlConfig = {
  enabled: boolean
  apiKey?: string | null
  apiUrl?: string | null
  mode?: string
  formats?: string[]
  onlyMainContent?: boolean
  maxAge?: number | null
  timeoutSecs?: number
  limit?: number | null
  maxDepth?: number | null
}

export type WebFetchConfig = {
  enabled: boolean
  allowedDomains: string[]
  blockedDomains: string[]
  allowedPrivateHosts: string[]
  maxResponseSize: number
  timeoutSecs: number
  firecrawl: FirecrawlConfig
}

export const webApi = {
  getSearch() {
    return api.get<WebSearchConfig>('/api/web-search')
  },

  updateSearch(patch: Partial<WebSearchConfig>) {
    return api.put<WebSearchConfig>('/api/web-search', patch)
  },

  getFetch() {
    return api.get<WebFetchConfig>('/api/web-fetch')
  },

  updateFetch(patch: Partial<WebFetchConfig>) {
    return api.put<WebFetchConfig>('/api/web-fetch', patch)
  },
}
