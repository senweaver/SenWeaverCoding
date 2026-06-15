// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { SavedProvider } from '../types/provider'

export const MODEL_TYPES = [
  'text',
  'image-generation',
  'video-generation',
  'audio-generation',
  'image-understanding',
  'video-understanding',
  'speech-recognition',
  'embedding',
  'rerank',
  'music-generation',
] as const

export type ModelType = (typeof MODEL_TYPES)[number]

export const DEFAULT_MODEL_TYPE: ModelType = 'text'

const MODEL_TYPE_SET = new Set<string>(MODEL_TYPES)

export function isKnownModelType(value: string): value is ModelType {
  return MODEL_TYPE_SET.has(value)
}

export function sanitizeModelTypes(values: readonly string[] | undefined): ModelType[] {
  const out: ModelType[] = []
  const seen = new Set<string>()
  for (const raw of values ?? []) {
    const trimmed = raw.trim()
    if (!isKnownModelType(trimmed) || seen.has(trimmed)) continue
    seen.add(trimmed)
    out.push(trimmed)
  }
  return out
}

export function effectiveModelTypes(values: readonly string[] | undefined): ModelType[] {
  const sanitized = sanitizeModelTypes(values)
  return sanitized.length > 0 ? sanitized : [DEFAULT_MODEL_TYPE]
}

export function modelTypeLabelKey(type: string): string {
  return `settings.modelTypes.${type}`
}

export function surfaceToModelType(surface: string | null | undefined): ModelType | null {
  switch (surface) {
    case 'image':
      return 'image-generation'
    case 'video':
      return 'video-generation'
    case 'audio':
      return 'audio-generation'
    default:
      return null
  }
}

export function modelTypesForId(provider: SavedProvider, modelId: string): ModelType[] {
  return effectiveModelTypes(provider.modelTypes?.[modelId])
}

export function buildModelTypeLookup(providers: SavedProvider[]): Map<string, ModelType[]> {
  const lookup = new Map<string, ModelType[]>()
  for (const provider of providers) {
    for (const raw of provider.models ?? []) {
      const id = raw.trim()
      if (!id) continue
      const types = modelTypesForId(provider, id)
      const existing = lookup.get(id)
      if (existing) {
        for (const type of types) {
          if (!existing.includes(type)) existing.push(type)
        }
      } else {
        lookup.set(id, [...types])
      }
    }
  }
  return lookup
}

export function modelMatchesType(
  lookup: Map<string, ModelType[]>,
  modelId: string,
  required: string,
): boolean {
  const types = lookup.get(modelId)
  if (!types) {

    return required === DEFAULT_MODEL_TYPE
  }
  return types.includes(required as ModelType)
}
