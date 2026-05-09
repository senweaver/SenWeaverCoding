import { useEffect, useState } from 'react'
import { useTranslation } from '../i18n'
import { Input } from '../components/shared/Input'
import { useWebResearchStore } from '../stores/webResearchStore'
import type { WebFetchConfig, WebSearchConfig } from '../api/web'

const PROVIDERS = [
  { value: 'duckduckgo', label: 'DuckDuckGo', requiresKey: false },
  { value: 'baidu', label: 'Baidu (百度)', requiresKey: false },
  { value: 'brave', label: 'Brave', requiresKey: true, keyField: 'braveApiKey' },
  { value: 'searxng', label: 'SearxNG', requiresKey: false },
  { value: 'tavily', label: 'Tavily', requiresKey: true, keyField: 'tavilyApiKey' },
  { value: 'exa', label: 'Exa', requiresKey: true, keyField: 'exaApiKey' },
] as const

export function WebResearchSettings() {
  const t = useTranslation()
  const webSearch = useWebResearchStore((s) => s.webSearch)
  const webFetch = useWebResearchStore((s) => s.webFetch)
  const isLoading = useWebResearchStore((s) => s.isLoading)
  const isSaving = useWebResearchStore((s) => s.isSaving)
  const hasFetched = useWebResearchStore((s) => s.hasFetched)
  const error = useWebResearchStore((s) => s.error)
  const fetch = useWebResearchStore((s) => s.fetch)
  const updateWebSearch = useWebResearchStore((s) => s.updateWebSearch)
  const updateWebFetch = useWebResearchStore((s) => s.updateWebFetch)

  useEffect(() => {
    if (!hasFetched && !isLoading) void fetch()
  }, [hasFetched, isLoading, fetch])

  return (
    <div className="max-w-3xl flex flex-col gap-6">
      <div>
        <h2 className="text-base font-semibold text-[var(--color-text-primary)] mb-1">
          {t('settings.web.title')}
        </h2>
        <p className="text-xs text-[var(--color-text-tertiary)]">
          {t('settings.web.description')}
        </p>
      </div>

      {error && (
        <div className="rounded-md border border-[var(--color-error)]/40 bg-[var(--color-error)]/10 px-3 py-2 text-xs text-[var(--color-error)]">
          {error}
        </div>
      )}

      {!hasFetched && isLoading && (
        <div className="text-xs text-[var(--color-text-tertiary)]">
          {t('settings.web.loading')}
        </div>
      )}

      {webSearch && (
        <WebSearchCard
          config={webSearch}
          onUpdate={updateWebSearch}
          saving={isSaving}
        />
      )}

      {webFetch && (
        <WebFetchCard
          config={webFetch}
          onUpdate={updateWebFetch}
          saving={isSaving}
        />
      )}
    </div>
  )
}

type WebSearchCardProps = {
  config: WebSearchConfig
  onUpdate: (patch: Partial<WebSearchConfig>) => Promise<void>
  saving: boolean
}

function WebSearchCard({ config, onUpdate, saving }: WebSearchCardProps) {
  const t = useTranslation()
  const [maxResults, setMaxResults] = useState(String(config.maxResults))
  const [timeoutSecs, setTimeoutSecs] = useState(String(config.timeoutSecs))

  useEffect(() => {
    setMaxResults(String(config.maxResults))
  }, [config.maxResults])
  useEffect(() => {
    setTimeoutSecs(String(config.timeoutSecs))
  }, [config.timeoutSecs])

  const provider = PROVIDERS.find((p) => p.value === config.provider) ?? PROVIDERS[0]
  const keyMissing =
    provider.requiresKey &&
    'keyField' in provider &&
    !((config[provider.keyField as keyof typeof config] as string | null | undefined) ?? '')

  const commitMaxResults = async () => {
    const n = Number.parseInt(maxResults, 10)
    if (!Number.isFinite(n)) {
      setMaxResults(String(config.maxResults))
      return
    }
    const clamped = Math.max(1, Math.min(10, n))
    if (clamped === config.maxResults) {
      setMaxResults(String(clamped))
      return
    }
    await onUpdate({ maxResults: clamped })
  }

  const commitTimeout = async () => {
    const n = Number.parseInt(timeoutSecs, 10)
    if (!Number.isFinite(n)) {
      setTimeoutSecs(String(config.timeoutSecs))
      return
    }
    const clamped = Math.max(1, Math.min(120, n))
    if (clamped === config.timeoutSecs) {
      setTimeoutSecs(String(clamped))
      return
    }
    await onUpdate({ timeoutSecs: clamped })
  }

  return (
    <SectionCard
      icon="travel_explore"
      title={t('settings.web.search.title')}
      description={t('settings.web.search.description')}
      enabled={config.enabled}
      onToggle={(next) => void onUpdate({ enabled: next })}
      saving={saving}
      disabledHint={t('settings.web.disabledHint')}
    >
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
        <FieldShell label={t('settings.web.search.provider')}>
          <select
            value={config.provider}
            onChange={(e) => void onUpdate({ provider: e.target.value })}
            disabled={!config.enabled || saving}
            className="h-10 rounded-md border border-[var(--color-border)] bg-[var(--color-surface)] px-2 text-sm text-[var(--color-text-primary)] disabled:opacity-50"
          >
            {PROVIDERS.map((p) => (
              <option key={p.value} value={p.value}>
                {p.label}
              </option>
            ))}
          </select>
        </FieldShell>
        <FieldShell label={t('settings.web.search.maxResults')}>
          <Input
            type="number"
            min={1}
            max={10}
            step={1}
            value={maxResults}
            onChange={(e) => setMaxResults(e.target.value)}
            onBlur={() => void commitMaxResults()}
            disabled={!config.enabled || saving}
          />
        </FieldShell>
        <FieldShell label={t('settings.web.search.timeout')}>
          <Input
            type="number"
            min={1}
            max={120}
            step={1}
            value={timeoutSecs}
            onChange={(e) => setTimeoutSecs(e.target.value)}
            onBlur={() => void commitTimeout()}
            disabled={!config.enabled || saving}
          />
        </FieldShell>
      </div>
      {config.enabled && keyMissing && (
        <div className="mt-3 rounded-md border border-[var(--color-warning)]/40 bg-[var(--color-warning)]/10 px-3 py-2 text-[11px] text-[var(--color-warning)]">
          {t('settings.web.providerHintNoKey')}
        </div>
      )}
    </SectionCard>
  )
}

type WebFetchCardProps = {
  config: WebFetchConfig
  onUpdate: (patch: Partial<WebFetchConfig>) => Promise<void>
  saving: boolean
}

function WebFetchCard({ config, onUpdate, saving }: WebFetchCardProps) {
  const t = useTranslation()
  const [domains, setDomains] = useState(config.allowedDomains.join(', '))
  const [maxSize, setMaxSize] = useState(String(config.maxResponseSize))
  const [timeoutSecs, setTimeoutSecs] = useState(String(config.timeoutSecs))

  useEffect(() => {
    setDomains(config.allowedDomains.join(', '))
  }, [config.allowedDomains])
  useEffect(() => {
    setMaxSize(String(config.maxResponseSize))
  }, [config.maxResponseSize])
  useEffect(() => {
    setTimeoutSecs(String(config.timeoutSecs))
  }, [config.timeoutSecs])

  const commitDomains = async () => {
    const list = domains
      .split(/[,\n]/g)
      .map((s) => s.trim())
      .filter(Boolean)
    if (
      list.length === config.allowedDomains.length &&
      list.every((v, i) => v === config.allowedDomains[i])
    ) {
      return
    }
    await onUpdate({ allowedDomains: list })
  }

  const commitMaxSize = async () => {
    const n = Number.parseInt(maxSize, 10)
    if (!Number.isFinite(n)) {
      setMaxSize(String(config.maxResponseSize))
      return
    }
    const clamped = Math.max(1024, n)
    if (clamped === config.maxResponseSize) {
      setMaxSize(String(clamped))
      return
    }
    await onUpdate({ maxResponseSize: clamped })
  }

  const commitTimeout = async () => {
    const n = Number.parseInt(timeoutSecs, 10)
    if (!Number.isFinite(n)) {
      setTimeoutSecs(String(config.timeoutSecs))
      return
    }
    const clamped = Math.max(1, Math.min(120, n))
    if (clamped === config.timeoutSecs) {
      setTimeoutSecs(String(clamped))
      return
    }
    await onUpdate({ timeoutSecs: clamped })
  }

  return (
    <SectionCard
      icon="downloading"
      title={t('settings.web.fetch.title')}
      description={t('settings.web.fetch.description')}
      enabled={config.enabled}
      onToggle={(next) => void onUpdate({ enabled: next })}
      saving={saving}
      disabledHint={t('settings.web.disabledHint')}
    >
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 mb-3">
        <FieldShell
          label={t('settings.web.fetch.allowedDomains')}
          className="sm:col-span-3"
        >
          <Input
            type="text"
            value={domains}
            onChange={(e) => setDomains(e.target.value)}
            onBlur={() => void commitDomains()}
            placeholder="*"
            disabled={!config.enabled || saving}
          />
        </FieldShell>
        <FieldShell label={t('settings.web.fetch.maxSize')}>
          <Input
            type="number"
            min={1024}
            step={1024}
            value={maxSize}
            onChange={(e) => setMaxSize(e.target.value)}
            onBlur={() => void commitMaxSize()}
            disabled={!config.enabled || saving}
          />
        </FieldShell>
        <FieldShell label={t('settings.web.fetch.timeout')}>
          <Input
            type="number"
            min={1}
            max={120}
            step={1}
            value={timeoutSecs}
            onChange={(e) => setTimeoutSecs(e.target.value)}
            onBlur={() => void commitTimeout()}
            disabled={!config.enabled || saving}
          />
        </FieldShell>
      </div>
      <p className="text-[11px] text-[var(--color-text-tertiary)]">
        {t('settings.web.fetch.allowedDomainsHint')}
      </p>
    </SectionCard>
  )
}

type SectionCardProps = {
  icon: string
  title: string
  description: string
  enabled: boolean
  onToggle: (next: boolean) => void
  saving: boolean
  disabledHint: string
  children: React.ReactNode
}

function SectionCard({
  icon,
  title,
  description,
  enabled,
  onToggle,
  saving,
  disabledHint,
  children,
}: SectionCardProps) {
  return (
    <section className="rounded-xl border border-[var(--color-border)] bg-[var(--color-surface-container-low)] p-4">
      <div className="flex items-start justify-between gap-3 mb-3">
        <div className="flex items-start gap-3 min-w-0">
          <span className="material-symbols-outlined shrink-0 text-[20px] text-[var(--color-brand)]">
            {icon}
          </span>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
              {title}
            </h3>
            <p className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
              {description}
            </p>
          </div>
        </div>
        <ToggleSwitch checked={enabled} onChange={onToggle} disabled={saving} />
      </div>
      {!enabled && (
        <div className="mb-3 rounded-md border border-[var(--color-border)]/60 bg-[var(--color-surface-container-lowest)] px-3 py-2 text-[11px] text-[var(--color-text-tertiary)]">
          {disabledHint}
        </div>
      )}
      <div aria-hidden={!enabled} className={enabled ? '' : 'opacity-60'}>
        {children}
      </div>
    </section>
  )
}

function FieldShell({
  label,
  children,
  className = '',
}: {
  label: string
  children: React.ReactNode
  className?: string
}) {
  return (
    <div className={`flex flex-col gap-1 ${className}`}>
      <span className="text-[11px] font-medium uppercase tracking-wide text-[var(--color-text-tertiary)]">
        {label}
      </span>
      {children}
    </div>
  )
}

function ToggleSwitch({
  checked,
  onChange,
  disabled = false,
}: {
  checked: boolean
  onChange: (next: boolean) => void
  disabled?: boolean
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
        checked
          ? 'bg-[var(--color-brand)]'
          : 'bg-[var(--color-surface-container-high)]'
      }`}
    >
      <span
        className={`inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ${
          checked ? 'translate-x-5' : 'translate-x-0.5'
        }`}
      />
    </button>
  )
}
