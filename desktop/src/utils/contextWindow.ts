// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.



export const DEFAULT_CONTEXT_WINDOW = 1_000_000

const MIN_DISPLAYED_CONTEXT = 1_000_000

type Rule = {

  match: readonly string[]

  context: number
}

const RULES: readonly Rule[] = [

  { match: ['claude', 'opus'], context: 200_000 },
  { match: ['claude', 'sonnet'], context: 200_000 },
  { match: ['claude', 'haiku'], context: 200_000 },
  { match: ['claude'], context: 200_000 },

  { match: ['deepseek', 'r1'], context: 64_000 },
  { match: ['deepseek', 'v4'], context: 128_000 },
  { match: ['deepseek', 'v3'], context: 128_000 },
  { match: ['deepseek', 'coder'], context: 128_000 },
  { match: ['deepseek', 'chat'], context: 128_000 },
  { match: ['deepseek'], context: 128_000 },

  { match: ['gpt-5'], context: 300_000 },

  { match: ['gpt-4.1'], context: 1_000_000 },
  { match: ['gpt-4o'], context: 128_000 },
  { match: ['gpt-4'], context: 128_000 },
  { match: ['o4'], context: 200_000 },
  { match: ['o3'], context: 200_000 },
  { match: ['o1'], context: 200_000 },

  { match: ['gemini', '2.5', 'pro'], context: 2_000_000 },
  { match: ['gemini', 'pro'], context: 1_000_000 },
  { match: ['gemini'], context: 1_000_000 },

  { match: ['qwen'], context: 128_000 },
  { match: ['llama'], context: 128_000 },
  { match: ['mistral'], context: 128_000 },
  { match: ['mixtral'], context: 64_000 },

]

export function inferContextWindow(modelId: string | null | undefined): number {
  if (!modelId) return DEFAULT_CONTEXT_WINDOW
  const lower = modelId.toLowerCase()
  for (const rule of RULES) {
    if (rule.match.every((needle) => lower.includes(needle))) {
      return Math.max(rule.context, MIN_DISPLAYED_CONTEXT)
    }
  }
  return DEFAULT_CONTEXT_WINDOW
}

export function resolveContextWindow(
  modelId: string | null | undefined,
  override: number | null | undefined,
): number {
  if (typeof override === 'number' && Number.isFinite(override) && override > 0) {
    return Math.floor(override)
  }
  return inferContextWindow(modelId)
}

export function formatTokenCount(tokens: number): string {
  if (!Number.isFinite(tokens) || tokens <= 0) return '0'
  if (tokens >= 1_000_000) {
    const mil = tokens / 1_000_000
    return `${mil >= 10 ? mil.toFixed(0) : mil.toFixed(1)}M`
  }
  if (tokens >= 10_000) {
    return `${(tokens / 1_000).toFixed(1)}K`
  }
  if (tokens >= 1_000) {
    return `${(tokens / 1_000).toFixed(1)}K`
  }
  return tokens.toLocaleString()
}
