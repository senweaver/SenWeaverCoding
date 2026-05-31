// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ApiFormat } from './provider'

export type ProviderPreset = {
  id: string
  name: string
  baseUrl: string
  apiFormat: ApiFormat

  defaultModels: string[]
  needsApiKey: boolean
  websiteUrl: string
}
