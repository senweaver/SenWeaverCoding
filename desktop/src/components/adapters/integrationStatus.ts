// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { IntegrationInfo } from '../../api/integrations'
import { useTranslation } from '../../i18n'

export function integrationStatusLabel(
  t: ReturnType<typeof useTranslation>,
  status: IntegrationInfo['status'],
): string {
  switch (status) {
    case 'Active':
      return t('settings.integrations.statusActive')
    case 'ComingSoon':
      return t('settings.integrations.statusComingSoon')
    default:
      return t('settings.integrations.statusAvailable')
  }
}

export function integrationStatusClass(status: IntegrationInfo['status']): string {
  switch (status) {
    case 'Active':
      return 'border-[var(--color-success)]/30 bg-[var(--color-success)]/12 text-[var(--color-success)]'
    case 'ComingSoon':
      return 'border-[var(--color-border)] bg-[var(--color-surface-container-high)] text-[var(--color-text-tertiary)]'
    default:
      return 'border-[var(--color-border)] bg-[var(--color-surface-container-high)] text-[var(--color-text-secondary)]'
  }
}

export function humanizeCategory(raw: string): string {
  if (!raw) return ''
  return raw.replace(/([a-z0-9])([A-Z])/g, '$1 $2')
}
