// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useState } from 'react'

import { browserApi } from '../api/browser'
import { Button } from '../components/shared/Button'
import { Input } from '../components/shared/Input'
import { useTranslation } from '../i18n'
import { useUIStore } from '../stores/uiStore'

export function ComputerUseSettings() {
  const t = useTranslation()
  const addToast = useUIStore((s) => s.addToast)

  const [enabled, setEnabled] = useState(false)
  const [endpoint, setEndpoint] = useState('')
  const [timeoutSec, setTimeoutSec] = useState('15')
  const [allowRemote, setAllowRemote] = useState(false)
  const [windowAllowlistText, setWindowAllowlistText] = useState('')
  const [apiKeySet, setApiKeySet] = useState(false)
  const [isLoading, setIsLoading] = useState(false)
  const [isSaving, setIsSaving] = useState(false)

  useEffect(() => {
    let cancelled = false
    const load = async () => {
      setIsLoading(true)
      try {
        const cfg = await browserApi.getComputerUse()
        if (cancelled) return
        setEnabled(cfg.enabled)
        setEndpoint(cfg.endpoint)
        setTimeoutSec(String(Math.round(cfg.timeoutMs / 1000)))
        setAllowRemote(cfg.allowRemoteEndpoint)
        setWindowAllowlistText(cfg.windowAllowlist.join('\n'))
        setApiKeySet(cfg.apiKeySet)
      } catch (err) {
        if (cancelled) return
        addToast({
          type: 'error',
          message: `${t('settings.computerUse.loadFailed')}: ${err instanceof Error ? err.message : String(err)}`,
        })
      } finally {
        if (!cancelled) setIsLoading(false)
      }
    }
    void load()
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const save = async () => {
    setIsSaving(true)
    try {
      const cfg = await browserApi.updateComputerUse({
        enabled,
        endpoint: endpoint.trim(),
        timeoutMs: Math.max(1, Number(timeoutSec) || 0) * 1000,
        allowRemoteEndpoint: allowRemote,
        windowAllowlist: windowAllowlistText
          .split('\n')
          .map((s) => s.trim())
          .filter(Boolean),
      })
      setEnabled(cfg.enabled)
      setEndpoint(cfg.endpoint)
      setTimeoutSec(String(Math.round(cfg.timeoutMs / 1000)))
      setAllowRemote(cfg.allowRemoteEndpoint)
      setWindowAllowlistText(cfg.windowAllowlist.join('\n'))
      setApiKeySet(cfg.apiKeySet)
      addToast({ type: 'success', message: t('settings.computerUse.savedToast') })
    } catch (err) {
      addToast({
        type: 'error',
        message: `${t('settings.computerUse.saveFailed')}: ${err instanceof Error ? err.message : String(err)}`,
      })
    } finally {
      setIsSaving(false)
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <div>
        <h2 className="text-lg font-semibold">{t('settings.computerUse.title')}</h2>
        <p className="mt-1 text-sm text-[var(--color-text-secondary)]">
          {t('settings.computerUse.description')}
        </p>
      </div>

      <label className="flex items-center gap-3">
        <input
          type="checkbox"
          checked={enabled}
          disabled={isLoading}
          onChange={(e) => setEnabled(e.target.checked)}
        />
        <span className="text-sm font-medium">{t('settings.computerUse.enableLabel')}</span>
      </label>
      <p className="-mt-4 text-xs text-[var(--color-text-secondary)]">
        {t('settings.computerUse.enableHint')}
      </p>

      <Input
        label={t('settings.computerUse.endpoint')}
        value={endpoint}
        disabled={isLoading}
        onChange={(e) => setEndpoint(e.target.value)}
      />
      <p className="-mt-4 text-xs text-[var(--color-text-secondary)]">
        {t('settings.computerUse.endpointHint')}
      </p>

      <Input
        label={t('settings.computerUse.timeoutSec')}
        type="number"
        min={1}
        value={timeoutSec}
        disabled={isLoading}
        onChange={(e) => setTimeoutSec(e.target.value)}
      />

      <label className="flex items-center gap-3">
        <input
          type="checkbox"
          checked={allowRemote}
          disabled={isLoading}
          onChange={(e) => setAllowRemote(e.target.checked)}
        />
        <span className="text-sm font-medium">{t('settings.computerUse.allowRemote')}</span>
      </label>
      <p className="-mt-4 text-xs text-[var(--color-text-secondary)]">
        {t('settings.computerUse.allowRemoteHint')}
      </p>

      <div>
        <label className="mb-1 block text-sm font-medium">
          {t('settings.computerUse.windowAllowlist')}
        </label>
        <textarea
          className="w-full rounded-md border border-[var(--color-border)] bg-[var(--color-bg)] p-2 text-sm"
          rows={4}
          value={windowAllowlistText}
          disabled={isLoading}
          onChange={(e) => setWindowAllowlistText(e.target.value)}
        />
        <p className="mt-1 text-xs text-[var(--color-text-secondary)]">
          {t('settings.computerUse.windowAllowlistHint')}
        </p>
      </div>

      <p className="text-xs text-[var(--color-text-secondary)]">
        {apiKeySet
          ? t('settings.computerUse.apiKeyManagedSet')
          : t('settings.computerUse.apiKeyManagedUnset')}
      </p>

      <div>
        <Button onClick={() => void save()} disabled={isSaving || isLoading} loading={isSaving}>
          {isSaving ? t('common.saving') : t('common.save')}
        </Button>
      </div>
    </div>
  )
}
