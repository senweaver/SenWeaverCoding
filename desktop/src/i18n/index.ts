// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useCallback } from 'react'
import { useSettingsStore } from '../stores/settingsStore'
import type { TranslationKey } from './locales/en'

export type Locale = 'en' | 'zh'

const translations: Partial<Record<Locale, Record<string, string>>> = {}

export async function ensureLocaleLoaded(locale: Locale): Promise<void> {
  if (!translations.en) {
    const m = await import('./locales/en')
    translations.en = m.en
  }
  if (locale === 'zh' && !translations.zh) {
    const m = await import('./locales/zh')
    translations.zh = m.zh
  }
}

export function translate(
  locale: Locale,
  key: TranslationKey,
  params?: Record<string, string | number>,
): string {
  let text = translations[locale]?.[key] ?? translations.en?.[key] ?? key
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      text = text.replace(new RegExp(`\\{${k}\\}`, 'g'), String(v))
    }
  }
  return text
}

export function useTranslation() {
  const locale = useSettingsStore((s) => s.locale)
  return useCallback(
    (key: TranslationKey, params?: Record<string, string | number>) =>
      translate(locale, key, params),
    [locale],
  )
}

export function t(key: TranslationKey, params?: Record<string, string | number>): string {
  const locale = useSettingsStore.getState().locale
  return translate(locale, key, params)
}

export function translateCodingMode(
  locale: Locale,
  modeId: string,
  kind: 'label' | 'description',
  fallback: string,
): string {
  const key = `codingMode.${modeId}.${kind}` as TranslationKey
  const value = translations[locale]?.[key] ?? translations.en?.[key]
  return value && value !== key ? value : fallback
}

export function useCodingModeText() {
  const locale = useSettingsStore((s) => s.locale)
  return useCallback(
    (modeId: string, kind: 'label' | 'description', fallback: string) =>
      translateCodingMode(locale, modeId, kind, fallback),
    [locale],
  )
}

export type { TranslationKey }
