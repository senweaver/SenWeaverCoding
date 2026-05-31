// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



export type ApiFormat = 'anthropic' | 'openai_chat' | 'openai_responses'

export type ModelMapping = {
  main: string
  haiku: string
  sonnet: string
  opus: string
}

export type CustomHttpHeader = {
  name: string
  value: string
  enabled: boolean
}

export type SavedProvider = {
  id: string
  presetId: string
  name: string
  apiKey: string
  baseUrl: string
  apiFormat: ApiFormat

  models: string[]

  modelEnabled?: Record<string, boolean>
  modelContextWindows?: Record<string, number>
  customHeaders?: CustomHttpHeader[]
  notes?: string
}

export type CreateProviderInput = {
  id?: string
  presetId: string
  name: string
  apiKey: string
  baseUrl: string
  apiFormat?: ApiFormat
  models: string[]
  modelEnabled?: Record<string, boolean>
  modelContextWindows?: Record<string, number>
  customHeaders?: CustomHttpHeader[]
  notes?: string
}

export type UpdateProviderInput = {
  name?: string
  apiKey?: string
  baseUrl?: string
  apiFormat?: ApiFormat
  models?: string[]
  modelEnabled?: Record<string, boolean>
  modelContextWindows?: Record<string, number>
  customHeaders?: CustomHttpHeader[]
  notes?: string
}

export type TestProviderConfigInput = {
  baseUrl: string
  apiKey: string
  modelId: string
  apiFormat?: ApiFormat
}

export type ProviderTestStepResult = {
  success: boolean
  latencyMs: number
  error?: string
  modelUsed?: string
  httpStatus?: number
}

export type ProviderTestResult = {

  connectivity: ProviderTestStepResult

  proxy?: ProviderTestStepResult
}
