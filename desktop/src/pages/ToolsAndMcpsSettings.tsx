import { useState } from 'react'
import { useTranslation } from '../i18n'
import { McpSettings } from './McpSettings'
import { CustomToolList } from '../components/tools/CustomToolList'

type SubTab = 'tools' | 'mcps'

export function ToolsAndMcpsSettings() {
  const t = useTranslation()
  const [tab, setTab] = useState<SubTab>('tools')

  return (
    <div className="space-y-4">
      <div>
        <h2 className="text-lg font-semibold text-[var(--color-text-primary)]">
          {t('settings.toolsAndMcps.title')}
        </h2>
        <p className="text-xs text-[var(--color-text-secondary)] mt-1">
          {t('settings.toolsAndMcps.description')}
        </p>
      </div>

      <div className="flex items-center gap-1 border-b border-[var(--color-border)]">
        <SubTabButton
          active={tab === 'tools'}
          onClick={() => setTab('tools')}
          icon="extension"
          label={t('settings.toolsAndMcps.subtabTools')}
        />
        <SubTabButton
          active={tab === 'mcps'}
          onClick={() => setTab('mcps')}
          icon="hub"
          label={t('settings.toolsAndMcps.subtabMcps')}
        />
      </div>

      {tab === 'tools' && <CustomToolList />}
      {tab === 'mcps' && <McpSettings />}
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
      className={`flex items-center gap-1.5 px-3 py-2 text-xs transition-colors border-b-2 -mb-[1px] ${
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
