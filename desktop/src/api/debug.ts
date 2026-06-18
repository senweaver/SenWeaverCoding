// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { api } from './client'

export type DebugFieldOption = {
  value: string
  labelEn: string
  labelZh: string
}

export type DebugFieldType = 'text' | 'select' | 'multiselect' | 'toggle' | 'number'

export type DebugField = {
  key: string
  labelEn: string
  labelZh: string
  type: DebugFieldType
  options?: DebugFieldOption[]
  default?: unknown
  placeholderEn?: string
  placeholderZh?: string
}

export type DebugSubmode = {
  id: string
  labelEn: string
  labelZh: string
  icon: string
  mayWrite: boolean
  fields: DebugField[]
}

export type DebugSubmodesResponse = {
  submodes: DebugSubmode[]
}

export type DebugReportFinding = {
  id: string
  severity: string
  bucket: string
  category: string
  title: string
  description: string
}

export type DebugReportCase = {
  id: string
  title: string
  status: string
}

export type DebugReport = {
  runId: string
  title: string
  generatedAt: string
  sessionId?: string | null
  submode: string
  summary: {
    findings: { total: number; p0: number; p1: number; p2: number; p3: number }
    cases: {
      total: number
      passed: number
      failed: number
      blocked: number
      other: number
    }
    coverage: number
    analysisNotes: number
  }
  findings: DebugReportFinding[]
  cases: DebugReportCase[]
  artifacts: { report: string; analysis: string; runbook: string }
}

export type DebugReportResponse = {
  report: DebugReport | null
}

export const debugApi = {
  submodes() {
    return api.get<DebugSubmodesResponse>('/api/debug/submodes')
  },
  report(sessionId: string) {
    return api.get<DebugReportResponse>(
      `/api/sessions/${encodeURIComponent(sessionId)}/debug-report`,
    )
  },
}
