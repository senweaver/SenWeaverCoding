// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useState } from 'react'
import { useTranslation } from '../i18n'
import { CredentialsSettings } from './CredentialsSettings'
import { HooksSettings } from './HooksSettings'

type SubTab = 'credentials' | 'hooks'

export function SecuritySettings() {
  const t = useTranslation()
  const [tab, setTab] = useState<SubTab>('credentials')

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-xs font-semibold text-[var(--color-text-primary)]">
          {t('settings.securityTab.title')}
        </h2>
        <p className="text-xs text-[var(--color-text-secondary)] mt-1">
          {t('settings.securityTab.description')}
        </p>
      </div>

      <div className="flex items-center gap-1 border-b border-[var(--color-border)]">
        <SubTabButton
          active={tab === 'credentials'}
          onClick={() => setTab('credentials')}
          icon="key"
          label={t('settings.securityTab.subtabCredentials')}
        />
        <SubTabButton
          active={tab === 'hooks'}
          onClick={() => setTab('hooks')}
          icon="webhook"
          label={t('settings.securityTab.subtabHooks')}
        />
      </div>

      {tab === 'credentials' && <CredentialsSettings />}
      {tab === 'hooks' && <HooksSettings />}
    </div>
  )
}

function SubTabButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean
  onClick: () => void
  icon: string
  label: string
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-1.5 px-2.5 py-1.5 text-xs transition-colors border-b-2 -mb-[1px] ${
        active
          ? 'border-[var(--color-brand)] text-[var(--color-text-primary)] font-medium'
          : 'border-transparent text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]'
      }`}
    >
      <span className="material-symbols-outlined text-[14px]">{icon}</span>
      {label}
    </button>
  )
}
