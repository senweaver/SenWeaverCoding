// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'
import { useTranslation } from '../../i18n'
import { Button } from '../shared/Button'
import { Input } from '../shared/Input'
import {
  systemSettingsApi,
  type ProxyScope,
  type ServiceTokensStatus,
} from '../../api/systemSettings'
import {
  SettingsSection,
  SettingsSectionStatus,
  type SettingsSectionStatusValue,
} from './SettingsSection'

function toErrorText(err: unknown, fallback: string) {
  return err instanceof Error && err.message ? err.message : fallback
}

function ToggleRow({
  label,
  hint,
  checked,
  onChange,
  disabled,
}: {
  label: string
  hint?: string
  checked: boolean
  onChange: (next: boolean) => void
  disabled?: boolean
}) {
  return (
    <div className="flex items-center justify-between gap-3 rounded-lg border border-[var(--color-border)] px-3 py-2.5">
      <div className="min-w-0 flex-1">
        <div className="text-xs font-medium text-[var(--color-text-primary)]">{label}</div>
        {hint && (
          <div className="text-xs text-[var(--color-text-tertiary)] mt-0.5">{hint}</div>
        )}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors disabled:opacity-50 ${
          checked ? 'bg-[var(--color-brand)]' : 'bg-[var(--color-surface-hover)]'
        }`}
      >
        <span
          className={`inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ${
            checked ? 'translate-x-5' : 'translate-x-0.5'
          }`}
        />
      </button>
    </div>
  )
}

function FieldLabel({ text }: { text: string }) {
  return (
    <label className="text-xs font-medium text-[var(--color-text-secondary)] mb-1 block">
      {text}
    </label>
  )
}

export function NetworkProxySection() {
  const t = useTranslation()
  const [loaded, setLoaded] = useState(false)
  const [enabled, setEnabled] = useState(false)
  const [httpProxy, setHttpProxy] = useState('')
  const [httpsProxy, setHttpsProxy] = useState('')
  const [allProxy, setAllProxy] = useState('')
  const [noProxy, setNoProxy] = useState('')
  const [scope, setScope] = useState<ProxyScope>('environment')
  const [systemDetect, setSystemDetect] = useState(false)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsSectionStatusValue>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const res = await systemSettingsApi.getNetworkSettings()
        if (cancelled) return
        const p = res.proxy
        setEnabled(p.enabled)
        setHttpProxy(p.httpProxy ?? '')
        setHttpsProxy(p.httpsProxy ?? '')
        setAllProxy(p.allProxy ?? '')
        setNoProxy((p.noProxy ?? []).join(', '))
        setScope(p.scope)
        setSystemDetect(p.systemDetect)
        setLoaded(true)
      } catch (err) {
        if (!cancelled) {
          setMessage({ kind: 'error', text: toErrorText(err, t('settings.system.loadFailed')) })
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [t])

  async function save() {
    setSaving(true)
    setMessage(null)
    try {
      await systemSettingsApi.updateNetworkSettings({
        proxy: {
          enabled,
          httpProxy: httpProxy.trim() ? httpProxy.trim() : null,
          httpsProxy: httpsProxy.trim() ? httpsProxy.trim() : null,
          allProxy: allProxy.trim() ? allProxy.trim() : null,
          noProxy: noProxy
            .split(',')
            .map((s) => s.trim())
            .filter(Boolean),
          scope,
          systemDetect,
        },
      })
      setMessage({ kind: 'ok', text: t('settings.system.saved') })
    } catch (err) {
      setMessage({ kind: 'error', text: toErrorText(err, t('settings.system.saveFailed')) })
    } finally {
      setSaving(false)
    }
  }

  const SCOPES: Array<{ value: ProxyScope; label: string }> = [
    { value: 'environment', label: t('settings.network.scope.environment') },
    { value: 'internal', label: t('settings.network.scope.internal') },
    { value: 'services', label: t('settings.network.scope.services') },
  ]

  return (
    <SettingsSection
      title={t('settings.network.title')}
      description={t('settings.network.description')}
      footer={
        <>
          <SettingsSectionStatus status={message} />
          <Button onClick={() => void save()} disabled={!loaded || saving} size="sm">
            {saving ? t('common.saving') : t('common.save')}
          </Button>
        </>
      }
    >
      <ToggleRow
        label={t('settings.network.enabled')}
        hint={t('settings.network.enabledHint')}
        checked={enabled}
        onChange={setEnabled}
        disabled={!loaded}
      />

      <div>
        <FieldLabel text={t('settings.network.httpProxy')} />
        <Input
          value={httpProxy}
          onChange={(e) => setHttpProxy(e.target.value)}
          placeholder="http://127.0.0.1:7890"
        />
      </div>

      <div>
        <FieldLabel text={t('settings.network.httpsProxy')} />
        <Input
          value={httpsProxy}
          onChange={(e) => setHttpsProxy(e.target.value)}
          placeholder="http://127.0.0.1:7890"
        />
      </div>

      <div>
        <FieldLabel text={t('settings.network.allProxy')} />
        <Input
          value={allProxy}
          onChange={(e) => setAllProxy(e.target.value)}
          placeholder="http://127.0.0.1:7890"
        />
      </div>

      <div>
        <FieldLabel text={t('settings.network.noProxy')} />
        <Input
          value={noProxy}
          onChange={(e) => setNoProxy(e.target.value)}
          placeholder="localhost, 127.0.0.1, .internal"
        />
        <p className="text-xs text-[var(--color-text-tertiary)] mt-1">
          {t('settings.network.noProxyHint')}
        </p>
      </div>

      <div>
        <FieldLabel text={t('settings.network.scope')} />
        <select
          value={scope}
          onChange={(e) => setScope(e.target.value as ProxyScope)}
          className="w-full h-10 px-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)]"
        >
          {SCOPES.map(({ value, label }) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </div>

      <ToggleRow
        label={t('settings.network.systemDetect')}
        hint={t('settings.network.systemDetectHint')}
        checked={systemDetect}
        onChange={setSystemDetect}
        disabled={!loaded}
      />
    </SettingsSection>
  )
}

export function AutomationSection() {
  const t = useTranslation()
  const [loaded, setLoaded] = useState(false)
  const [enabled, setEnabled] = useState(false)
  const [catchUp, setCatchUp] = useState(false)
  const [maxRunHistory, setMaxRunHistory] = useState(0)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsSectionStatusValue>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const res = await systemSettingsApi.getCronSettings()
        if (cancelled) return
        setEnabled(res.enabled)
        setCatchUp(res.catch_up_on_startup)
        setMaxRunHistory(res.max_run_history)
        setLoaded(true)
      } catch (err) {
        if (!cancelled) {
          setMessage({ kind: 'error', text: toErrorText(err, t('settings.system.loadFailed')) })
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [t])

  async function save() {
    setSaving(true)
    setMessage(null)
    try {
      const res = await systemSettingsApi.updateCronSettings({
        enabled,
        catch_up_on_startup: catchUp,
        max_run_history: maxRunHistory,
      })
      setEnabled(res.enabled)
      setCatchUp(res.catch_up_on_startup)
      setMaxRunHistory(res.max_run_history)
      setMessage({ kind: 'ok', text: t('settings.system.saved') })
    } catch (err) {
      setMessage({ kind: 'error', text: toErrorText(err, t('settings.system.saveFailed')) })
    } finally {
      setSaving(false)
    }
  }

  return (
    <SettingsSection
      title={t('settings.automation.title')}
      description={t('settings.automation.description')}
      footer={
        <>
          <SettingsSectionStatus status={message} />
          <Button onClick={() => void save()} disabled={!loaded || saving} size="sm">
            {saving ? t('common.saving') : t('common.save')}
          </Button>
        </>
      }
    >
      <ToggleRow
        label={t('settings.automation.enabled')}
        hint={t('settings.automation.enabledHint')}
        checked={enabled}
        onChange={setEnabled}
        disabled={!loaded}
      />
      <ToggleRow
        label={t('settings.automation.catchUp')}
        hint={t('settings.automation.catchUpHint')}
        checked={catchUp}
        onChange={setCatchUp}
        disabled={!loaded}
      />

      <div>
        <FieldLabel text={t('settings.automation.maxRunHistory')} />
        <Input
          type="number"
          min={0}
          value={Number.isFinite(maxRunHistory) ? maxRunHistory : 0}
          onChange={(e) => setMaxRunHistory(Number.parseInt(e.target.value || '0', 10))}
        />
        <p className="text-xs text-[var(--color-text-tertiary)] mt-1">
          {t('settings.automation.maxRunHistoryHint')}
        </p>
      </div>
    </SettingsSection>
  )
}

function parseLimit(raw: string): number | null {
  const trimmed = raw.trim()
  if (!trimmed) return null
  const n = Number.parseInt(trimmed, 10)
  return Number.isFinite(n) && n >= 0 ? n : null
}

export function SecuritySandboxSection() {
  const t = useTranslation()
  const [loaded, setLoaded] = useState(false)
  const [sandboxEnabled, setSandboxEnabled] = useState(false)
  const [backend, setBackend] = useState('')
  const [confineFilesystem, setConfineFilesystem] = useState(false)
  const [availableBackends, setAvailableBackends] = useState<string[]>([])
  const [maxMemoryMb, setMaxMemoryMb] = useState('')
  const [maxCpuTimeSeconds, setMaxCpuTimeSeconds] = useState('')
  const [maxSubprocesses, setMaxSubprocesses] = useState('')
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<SettingsSectionStatusValue>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const res = await systemSettingsApi.getSecuritySettings()
        if (cancelled) return
        setSandboxEnabled(res.sandbox.enabled)
        setBackend(res.sandbox.backend)
        setConfineFilesystem(res.sandbox.confineFilesystem)
        setAvailableBackends(res.sandbox.availableBackends ?? [])
        setMaxMemoryMb(res.resources.maxMemoryMb != null ? String(res.resources.maxMemoryMb) : '')
        setMaxCpuTimeSeconds(
          res.resources.maxCpuTimeSeconds != null ? String(res.resources.maxCpuTimeSeconds) : '',
        )
        setMaxSubprocesses(
          res.resources.maxSubprocesses != null ? String(res.resources.maxSubprocesses) : '',
        )
        setLoaded(true)
      } catch (err) {
        if (!cancelled) {
          setMessage({ kind: 'error', text: toErrorText(err, t('settings.system.loadFailed')) })
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [t])

  async function save() {
    setSaving(true)
    setMessage(null)
    try {
      await systemSettingsApi.updateSecuritySettings({
        sandbox: {
          enabled: sandboxEnabled,
          backend,
          confineFilesystem,
        },
        resources: {
          maxMemoryMb: parseLimit(maxMemoryMb),
          maxCpuTimeSeconds: parseLimit(maxCpuTimeSeconds),
          maxSubprocesses: parseLimit(maxSubprocesses),
        },
      })
      setMessage({ kind: 'ok', text: t('settings.system.saved') })
    } catch (err) {
      setMessage({ kind: 'error', text: toErrorText(err, t('settings.system.saveFailed')) })
    } finally {
      setSaving(false)
    }
  }

  const backendOptions = availableBackends.includes(backend) || backend.length === 0
    ? availableBackends
    : [backend, ...availableBackends]

  return (
    <SettingsSection
      title={t('settings.security.title')}
      description={t('settings.security.description')}
      footer={
        <>
          <SettingsSectionStatus status={message} />
          <Button onClick={() => void save()} disabled={!loaded || saving} size="sm">
            {saving ? t('common.saving') : t('common.save')}
          </Button>
        </>
      }
    >
      <ToggleRow
        label={t('settings.security.sandboxEnabled')}
        hint={t('settings.security.sandboxEnabledHint')}
        checked={sandboxEnabled}
        onChange={setSandboxEnabled}
        disabled={!loaded}
      />

      <div>
        <FieldLabel text={t('settings.security.backend')} />
        <select
          value={backend}
          onChange={(e) => setBackend(e.target.value)}
          disabled={!loaded}
          className="w-full h-10 px-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] text-sm text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] disabled:opacity-50"
        >
          {backendOptions.map((b) => (
            <option key={b} value={b}>
              {b}
            </option>
          ))}
        </select>
      </div>

      <ToggleRow
        label={t('settings.security.confineFilesystem')}
        hint={t('settings.security.confineFilesystemHint')}
        checked={confineFilesystem}
        onChange={setConfineFilesystem}
        disabled={!loaded}
      />

      <div>
        <div className="text-xs font-medium text-[var(--color-text-primary)] mb-1">
          {t('settings.security.resourcesTitle')}
        </div>
        <p className="text-xs text-[var(--color-text-tertiary)] mb-2">
          {t('settings.security.resourcesHint')}
        </p>
        <div className="grid grid-cols-3 gap-3">
          <div>
            <FieldLabel text={t('settings.security.maxMemoryMb')} />
            <Input
              type="number"
              min={0}
              value={maxMemoryMb}
              onChange={(e) => setMaxMemoryMb(e.target.value)}
            />
          </div>
          <div>
            <FieldLabel text={t('settings.security.maxCpuTimeSeconds')} />
            <Input
              type="number"
              min={0}
              value={maxCpuTimeSeconds}
              onChange={(e) => setMaxCpuTimeSeconds(e.target.value)}
            />
          </div>
          <div>
            <FieldLabel text={t('settings.security.maxSubprocesses')} />
            <Input
              type="number"
              min={0}
              value={maxSubprocesses}
              onChange={(e) => setMaxSubprocesses(e.target.value)}
            />
          </div>
        </div>
      </div>
    </SettingsSection>
  )
}

export function ServiceTokensSection() {
  const t = useTranslation()
  const [tokens, setTokens] = useState<ServiceTokensStatus | null>(null)
  const [rpcInput, setRpcInput] = useState('')
  const [mcpInput, setMcpInput] = useState('')
  const [tokenBusy, setTokenBusy] = useState(false)
  const [tokenMessage, setTokenMessage] = useState<SettingsSectionStatusValue>(null)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const res = await systemSettingsApi.getServiceTokens()
        if (!cancelled) setTokens(res)
      } catch (err) {
        if (!cancelled) {
          setTokenMessage({ kind: 'error', text: toErrorText(err, t('settings.system.loadFailed')) })
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [t])

  async function saveTokens() {
    const payload: { rpcToken?: string | null; mcpSseToken?: string | null } = {}
    if (rpcInput.trim()) payload.rpcToken = rpcInput.trim()
    if (mcpInput.trim()) payload.mcpSseToken = mcpInput.trim()
    if (payload.rpcToken === undefined && payload.mcpSseToken === undefined) return
    setTokenBusy(true)
    setTokenMessage(null)
    try {
      const res = await systemSettingsApi.updateServiceTokens(payload)
      setTokens(res)
      setRpcInput('')
      setMcpInput('')
      setTokenMessage({ kind: 'ok', text: t('settings.system.saved') })
    } catch (err) {
      setTokenMessage({ kind: 'error', text: toErrorText(err, t('settings.system.saveFailed')) })
    } finally {
      setTokenBusy(false)
    }
  }

  async function clearToken(kind: 'rpc' | 'mcp') {
    setTokenBusy(true)
    setTokenMessage(null)
    try {
      const res = await systemSettingsApi.updateServiceTokens(
        kind === 'rpc' ? { rpcToken: null } : { mcpSseToken: null },
      )
      setTokens(res)
      if (kind === 'rpc') setRpcInput('')
      else setMcpInput('')
      setTokenMessage({ kind: 'ok', text: t('settings.system.saved') })
    } catch (err) {
      setTokenMessage({ kind: 'error', text: toErrorText(err, t('settings.system.saveFailed')) })
    } finally {
      setTokenBusy(false)
    }
  }

  return (
    <SettingsSection
      title={t('settings.security.tokensTitle')}
      description={t('settings.security.tokensDescription')}
      footer={
        <>
          <SettingsSectionStatus status={tokenMessage} />
          <Button
            onClick={() => void saveTokens()}
            disabled={tokenBusy || !tokens || (!rpcInput.trim() && !mcpInput.trim())}
            size="sm"
          >
            {tokenBusy ? t('common.saving') : t('common.save')}
          </Button>
        </>
      }
    >
      <div className="space-y-2">
        <TokenRow
          label={t('settings.security.rpcToken')}
          isSet={tokens?.rpcTokenSet ?? false}
          value={rpcInput}
          onChange={setRpcInput}
          onClear={() => void clearToken('rpc')}
          busy={tokenBusy || !tokens}
        />
        <TokenRow
          label={t('settings.security.mcpSseToken')}
          isSet={tokens?.mcpSseTokenSet ?? false}
          value={mcpInput}
          onChange={setMcpInput}
          onClear={() => void clearToken('mcp')}
          busy={tokenBusy || !tokens}
        />
      </div>
    </SettingsSection>
  )
}

function TokenRow({
  label,
  isSet,
  value,
  onChange,
  onClear,
  busy,
}: {
  label: string
  isSet: boolean
  value: string
  onChange: (next: string) => void
  onClear: () => void
  busy: boolean
}) {
  const t = useTranslation()
  return (
    <div className="rounded-lg border border-[var(--color-border)] px-3 py-2.5">
      <div className="flex items-center justify-between gap-2 mb-1.5">
        <span className="text-xs font-medium text-[var(--color-text-primary)]">{label}</span>
        <span
          className={`text-[10px] font-semibold px-2 py-0.5 rounded-full ${
            isSet
              ? 'bg-[var(--color-success)]/12 text-[var(--color-success)]'
              : 'bg-[var(--color-surface-hover)] text-[var(--color-text-tertiary)]'
          }`}
        >
          {isSet ? t('settings.security.tokenSet') : t('settings.security.tokenUnset')}
        </span>
      </div>
      <div className="flex items-center gap-2">
        <div className="flex-1">
          <Input
            type="password"
            value={value}
            onChange={(e) => onChange(e.target.value)}
            placeholder={
              isSet
                ? t('settings.security.tokenPlaceholderReplace')
                : t('settings.security.tokenPlaceholderSet')
            }
            className="h-8 text-xs"
            disabled={busy}
          />
        </div>
        <Button variant="ghost" size="sm" onClick={onClear} disabled={busy || !isSet}>
          {t('settings.security.tokenClear')}
        </Button>
      </div>
    </div>
  )
}
