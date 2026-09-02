// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type DesignerFieldOption = {
  value: string
  labelEn: string
  labelZh: string
}

export type DesignerFieldType =
  | 'text'
  | 'select'
  | 'multiselect'
  | 'toggle'
  | 'number'
  | 'designSystem'
  | 'promptTemplate'
  | 'htmlTemplate'

export type DesignerField = {
  key: string
  labelEn: string
  labelZh: string
  type: DesignerFieldType
  options?: DesignerFieldOption[]
  default?: unknown
  surface?: 'image' | 'video' | 'audio'
  required?: boolean
  min?: number
  max?: number
  placeholderEn?: string
  placeholderZh?: string
}

export type DesignerSubmode = {
  id: string
  labelEn: string
  labelZh: string
  icon: string
  surface?: 'image' | 'video' | 'audio' | null
  modelPicker?: boolean
  fields: DesignerField[]
}

export type DesignerSubmodesResponse = {
  submodes: DesignerSubmode[]
}

export type DesignSystemMeta = {
  id: string
  name: string
  category: string
  description: string
}

export type DesignSystemsResponse = {
  designSystems: DesignSystemMeta[]
}

export type PromptTemplateMeta = {
  id: string
  surface: 'image' | 'video'
  title: string
  summary: string
  category: string
  tags: string[]
  model?: string
  aspect?: string
  previewImageUrl?: string
  previewVideoUrl?: string
}

export type PromptTemplatesResponse = {
  promptTemplates: PromptTemplateMeta[]
}

export type HtmlTemplateMeta = {
  id: string
  title: string
  category: string
  tags: string[]
  summary: string
}

export type HtmlTemplatesResponse = {
  htmlTemplates: HtmlTemplateMeta[]
}

export type DesignArtifactRecord = {
  relPath: string
  submode: string | null
  surface: string
  createdAt: number
  updatedAt: number
}

export type DesignArtifactsResponse = {
  artifacts: DesignArtifactRecord[]
}

export type DesignHandoffResult = {
  zipPath: string
  manifestPath: string
  handoffPath: string
  reactPaths: string[]
  fileCount: number
}

export type DesignHandoffResponse = {
  ok: boolean
  error?: string
  handoff?: DesignHandoffResult
}

export type DesignLintFinding = {
  severity: string
  rule: string
  message: string
  line?: number
}

export type DesignLintReport = {
  findings: DesignLintFinding[]
  p0: number
  p1: number
  p2: number
}

export type DesignLintResponse = {
  ok: boolean
  error?: string
  report?: DesignLintReport
}

export type DesignUnitAddResponse = {
  ok: boolean
  error?: string
  relPath?: string
}

export const designerApi = {
  submodes() {
    return api.get<DesignerSubmodesResponse>('/api/designer/submodes')
  },
  designSystems() {
    return api.get<DesignSystemsResponse>('/api/designer/design-systems')
  },
  promptTemplates() {
    return api.get<PromptTemplatesResponse>('/api/designer/prompt-templates')
  },
  htmlTemplates() {
    return api.get<HtmlTemplatesResponse>('/api/designer/html-templates')
  },
  designArtifacts(sessionId: string) {
    return api.get<DesignArtifactsResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/design-artifacts`,
    )
  },
  deleteArtifact(sessionId: string, relPath: string) {
    return api.post<{ ok: boolean; error?: string }>(
      `/api/sessions/${encodeURIComponent(sessionId)}/design-artifacts/delete`,
      { relPath },
    )
  },
  exportHandoff(sessionId: string) {
    return api.post<DesignHandoffResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/design-handoff`,
      {},
    )
  },
  lintArtifact(sessionId: string, relPath: string) {
    return api.post<DesignLintResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/design-lint`,
      { relPath },
    )
  },
  addUnit(
    sessionId: string,
    body: { source: 'template' | 'html'; templateId?: string; name?: string; html?: string },
  ) {
    return api.post<DesignUnitAddResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/design-units`,
      body,
    )
  },
}
