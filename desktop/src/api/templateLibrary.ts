// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type TemplateFileStatus = 'builtin' | 'user' | 'customized' | 'stale'
export type TemplateSource = 'builtin' | 'user'

export type TemplateKindId =
  | 'design-system'
  | 'designer-template'
  | 'prompt-template'
  | 'curator-template'

export type TemplateFile = {
  path: string
  file: string
  status: TemplateFileStatus
}

export type TemplateItem = {
  kind: TemplateKindId
  id: string
  name: string
  category: string
  description: string
  surface?: string
  tags?: string[]
  model?: string | null
  aspect?: string | null
  previewImageUrl?: string | null
  previewVideoUrl?: string | null
  source: TemplateSource
  customized: boolean
  stale: boolean
  files: TemplateFile[]
}

export type TemplateLibraryCatalog = {
  designSystems: TemplateItem[]
  designerTemplates: TemplateItem[]
  promptTemplates: TemplateItem[]
  curatorTemplates: TemplateItem[]
}

export type TemplateFileResponse = {
  ok: boolean
  path: string
  status?: TemplateFileStatus
  content: string
}

export type TemplateCreateBody = {
  kind: TemplateKindId
  id: string
  name?: string
  description?: string
  category?: string
  surface?: string
  base_kind?: string
}

export const templateLibraryApi = {
  catalog() {
    return api.get<TemplateLibraryCatalog>('/api/template-library/catalog')
  },
  file(path: string) {
    return api.get<TemplateFileResponse>(
      `/api/template-library/file?path=${encodeURIComponent(path)}`,
    )
  },
  builtinFile(path: string) {
    return api.get<TemplateFileResponse>(
      `/api/template-library/builtin-file?path=${encodeURIComponent(path)}`,
    )
  },
  save(path: string, content: string) {
    return api.put<{ ok: boolean; path: string; status: TemplateFileStatus }>(
      '/api/template-library/file',
      { path, content },
    )
  },
  reset(path: string) {
    return api.post<{ ok: boolean; path: string; status: TemplateFileStatus }>(
      '/api/template-library/reset',
      { path },
    )
  },
  create(body: TemplateCreateBody) {
    return api.post<{ ok: boolean; id: string; kind: string }>(
      '/api/template-library/create',
      body,
    )
  },
  remove(kind: TemplateKindId, id: string, surface?: string) {
    const surfaceQuery = surface ? `&surface=${encodeURIComponent(surface)}` : ''
    return api.delete<{ ok: boolean; id: string }>(
      `/api/template-library/entry?kind=${encodeURIComponent(kind)}&id=${encodeURIComponent(id)}${surfaceQuery}`,
    )
  },
}
