// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



export type ApiFormat = 'anthropic' | 'openai_chat' | 'openai_responses'

export const API_FORMATS: readonly ApiFormat[] = [
  'openai_chat',
  'openai_responses',
  'anthropic',
] as const

export function normalizeApiFormat(raw: string | null | undefined): ApiFormat {
  const value = (raw ?? '').trim().toLowerCase().replace(/-/g, '_')
  switch (value) {
    case 'anthropic':
    case 'anthropic_messages':
    case 'anthropic_chat':
      return 'anthropic'
    case 'openai_responses':
    case 'responses':
    case 'openai_responses_api':
      return 'openai_responses'
    case 'openai_chat':
    case 'chat_completions':
    case 'chat':
    case 'chatcompletions':
      return 'openai_chat'
    default:
      return 'openai_chat'
  }
}

export function apiFormatLabel(format: string | null | undefined): string {
  switch (normalizeApiFormat(format)) {
    case 'anthropic':
      return 'Anthropic Messages'
    case 'openai_responses':
      return 'OpenAI Responses'
    case 'openai_chat':
      return 'OpenAI Chat'
  }
}

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

export type ModelPricingEntry = {
  input: number
  output: number
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
  modelTypes?: Record<string, string[]>
  modelPricing?: Record<string, ModelPricingEntry>
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
  modelTypes?: Record<string, string[]>
  modelPricing?: Record<string, ModelPricingEntry>
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
  modelTypes?: Record<string, string[]>
  modelPricing?: Record<string, ModelPricingEntry>
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

export type DiscoveredModel = {
  id: string
  types: string[]
}

export type DiscoverModelsInput = {
  baseUrl: string
  apiFormat: ApiFormat
  apiKey?: string
  presetId?: string
  providerId?: string
}

export type DiscoverModelsResult = {
  source: string
  count: number
  models: DiscoveredModel[]
}
