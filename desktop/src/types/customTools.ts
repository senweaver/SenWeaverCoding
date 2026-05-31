// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

export type CustomToolDef = {
  name: string
  description: string
  command: string
  args: string[]
  cwd: string | null
  env: Record<string, string>
  timeoutSecs: number
  schema: Record<string, unknown> | unknown
  enabled: boolean
}

export type CustomToolPatch = Partial<Omit<CustomToolDef, 'name'>> & {
  name?: string
}
