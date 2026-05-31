// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'

import { Modal } from '../shared/Modal'
import { Button } from '../shared/Button'
import { useTranslation } from '../../i18n'
import { usePythonEnvStore } from '../../stores/pythonEnvStore'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import { useTerminalPanelStore } from '../../stores/terminalPanelStore'
import { useUIStore } from '../../stores/uiStore'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import type { InstallStrategy } from '../../api/python'

type PythonEnvPickerProps = {
  open: boolean
  onClose: () => void
}

const STRATEGY_LABEL: Record<InstallStrategy, string> = {
  uv_sync: 'uv sync',
  uv_pip_editable: 'uv pip install -e .',
  pip_editable: 'pip install -e .',
  uv_pip_requirements: 'uv pip install -r ...',
  pip_requirements: 'pip install -r ...',
  none: '',
}

export function PythonEnvPicker({ open, onClose }: PythonEnvPickerProps) {
  const t = useTranslation()
  const workspaceRoot = useWorkspaceFilesStore((s) => s.root)
  const status = usePythonEnvStore((s) => (workspaceRoot ? s.statusByRoot[workspaceRoot] : undefined))
  const discovery = usePythonEnvStore((s) => (workspaceRoot ? s.discoveryByRoot[workspaceRoot] : undefined))
  const job = usePythonEnvStore((s) => (workspaceRoot ? s.jobsByRoot[workspaceRoot] : undefined))
  const lastError = usePythonEnvStore((s) => (workspaceRoot ? s.errorByRoot[workspaceRoot] : undefined))
  const refresh = usePythonEnvStore((s) => s.refresh)
  const discover = usePythonEnvStore((s) => s.discover)
  const createVenv = usePythonEnvStore((s) => s.createVenv)
  const selectInterpreter = usePythonEnvStore((s) => s.selectInterpreter)
  const installSmart = usePythonEnvStore((s) => s.installSmart)
  const purge = usePythonEnvStore((s) => s.purge)
  const subscribe = usePythonEnvStore((s) => s.subscribe)
  const openTerminalPanel = useTerminalPanelStore((s) => s.setOpen)
  const addToast = useUIStore((s) => s.addToast)

  const [busy, setBusy] = useState(false)
  const [showLogs, setShowLogs] = useState(true)
  const [showErrorDetails, setShowErrorDetails] = useState(false)

  useEffect(() => {
    if (!open || !workspaceRoot) return
    subscribe(workspaceRoot)
    void refresh(workspaceRoot)
    void discover(workspaceRoot)
  }, [open, workspaceRoot, refresh, discover, subscribe])

  useEffect(() => {
    if (!open) setShowErrorDetails(false)
  }, [open])

  const requiredVersionLabel = useMemo(() => {
    const v =
      status?.requiredPython?.version ??
      discovery?.requiredPython?.version ??
      null
    if (!v) return null
    return v
  }, [discovery, status])

  const installRecommendation = useMemo(() => {
    return (
      status?.installRecommendation ??
      discovery?.installRecommendation ??
      null
    )
  }, [discovery, status])

  const installAvailable = useMemo(() => {
    return Boolean(installRecommendation && installRecommendation.strategy !== 'none')
  }, [installRecommendation])

  const isPythonInstalled = useMemo(() => {
    if (discovery && discovery.interpreters.length > 0) return true
    if (status?.interpreterPath) return true
    return false
  }, [discovery, status])

  if (!workspaceRoot) {
    return (
      <Modal open={open} onClose={onClose} title={t('python.picker.title')} width={520}>
        <p className="text-sm text-[var(--color-text-secondary)]">{t('python.picker.notConfigured')}</p>
      </Modal>
    )
  }

  const handleCreate = async (tool: 'uv' | 'venv') => {
    setBusy(true)
    try {
      await createVenv(workspaceRoot, tool)
    } finally {
      setBusy(false)
    }
  }

  const handleSelectOther = async () => {
    if (!isTauriRuntime()) return
    try {
      const { open: openDialog } = await import('@tauri-apps/plugin-dialog')
      const selected = await openDialog({
        directory: false,
        multiple: false,
        title: t('python.picker.selectOther'),
        filters:
          typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('win')
            ? [{ name: 'Python', extensions: ['exe'] }]
            : undefined,
      })
      const path = Array.isArray(selected) ? selected[0] : selected
      if (path && typeof path === 'string') {
        setBusy(true)
        try {
          await selectInterpreter(workspaceRoot, path)
        } finally {
          setBusy(false)
        }
      }
    } catch (err) {
      console.warn('[PythonEnvPicker] dialog failed', err)
    }
  }

  const handleInstall = async () => {
    setBusy(true)
    try {
      await installSmart(workspaceRoot)
    } finally {
      setBusy(false)
    }
  }

  const handleSelectCandidate = async (path: string) => {
    setBusy(true)
    try {
      await selectInterpreter(workspaceRoot, path)
    } finally {
      setBusy(false)
    }
  }

  const handleOpenVenvDir = async () => {
    const venvDir = joinPath(workspaceRoot, '.venv')
    try {
      const { open: shellOpen } = await import('@tauri-apps/plugin-shell')
      await shellOpen(venvDir)
    } catch (err) {
      console.warn('[PythonEnvPicker] open .venv failed', err)
      addToast({
        type: 'error',
        message: t('python.picker.openDirFailed'),
      })
    }
  }

  const handleOpenTerminal = () => {
    openTerminalPanel(true)
    addToast({
      type: 'info',
      message: t('python.picker.terminalOpened'),
      duration: 3000,
    })
  }

  const handlePurge = async () => {
    setBusy(true)
    try {
      await purge(workspaceRoot)
    } finally {
      setBusy(false)
    }
  }

  const handleOpenPythonDownload = async () => {
    try {
      const { open: shellOpen } = await import('@tauri-apps/plugin-shell')
      await shellOpen('https://www.python.org/downloads/')
    } catch {
      window.open('https://www.python.org/downloads/', '_blank', 'noopener,noreferrer')
    }
  }

  const isolationLabel = status?.isIsolated
    ? t('python.picker.isolatedYes')
    : t('python.picker.isolatedNo')

  const currentInterpreterPath = status?.interpreterPath ?? null
  const hasVenv = Boolean(status?.isIsolated || discovery?.markers.hasVenvDir)
  const errorLines = lastError ? lastError.split('\n') : []
  const errorIsLong = errorLines.length > 2 || (lastError?.length ?? 0) > 140

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t('python.picker.title')}
      width={680}
      footer={(
        <>
          <Button variant="secondary" onClick={() => void refresh(workspaceRoot)}>
            {t('python.picker.refresh')}
          </Button>
          <Button variant="primary" onClick={onClose}>
            {t('python.picker.close')}
          </Button>
        </>
      )}
    >
      <div className="flex flex-col gap-4 text-sm">
        {!isPythonInstalled && !job && (
          <section className="rounded-[var(--radius-md)] border border-[var(--color-warning)]/40 bg-[var(--color-warning)]/8 px-4 py-3">
            <p className="text-xs text-[var(--color-text-primary)]">
              {t('python.picker.notInstalledNotice')}
            </p>
            <div className="mt-2">
              <Button variant="secondary" size="sm" onClick={() => void handleOpenPythonDownload()}>
                {t('python.picker.openPythonDownload')}
              </Button>
            </div>
          </section>
        )}

        <section className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-3">
          <h3 className="text-sm font-semibold text-[var(--color-text-primary)] mb-2">
            {t('python.picker.current')}
          </h3>
          {status?.interpreterPath ? (
            <div className="flex flex-col gap-1 text-xs text-[var(--color-text-secondary)]">
              <div className="flex gap-2">
                <span className="w-24 text-[var(--color-text-tertiary)]">
                  {t('python.picker.versionLabel')}
                </span>
                <span className="text-[var(--color-text-primary)]">
                  {status.version ?? '—'}
                </span>
              </div>
              <div className="flex gap-2">
                <span className="w-24 text-[var(--color-text-tertiary)]">
                  {t('python.picker.toolLabel')}
                </span>
                <span className="text-[var(--color-text-primary)] capitalize">{status.tool}</span>
              </div>
              <div className="flex gap-2">
                <span className="w-24 text-[var(--color-text-tertiary)]">
                  {t('python.picker.pathLabel')}
                </span>
                <span className="text-[var(--color-text-primary)] break-all">
                  {status.interpreterPath}
                </span>
              </div>
              {status.packagesCount != null && (
                <div className="flex gap-2">
                  <span className="w-24 text-[var(--color-text-tertiary)]">
                    {t('python.picker.packagesLabel')}
                  </span>
                  <span className="text-[var(--color-text-primary)]">
                    {status.packagesCount}
                  </span>
                </div>
              )}
              {requiredVersionLabel && (
                <div className="flex gap-2">
                  <span className="w-24 text-[var(--color-text-tertiary)]">
                    {t('python.picker.requiredLabel')}
                  </span>
                  <span className="text-[var(--color-text-primary)]">
                    {requiredVersionLabel}
                    {status.requiredPython?.source ? (
                      <span className="ml-1 text-[var(--color-text-tertiary)]">
                        ({status.requiredPython.source})
                      </span>
                    ) : null}
                  </span>
                </div>
              )}
              <div className="mt-1 inline-flex items-center gap-1">
                <span
                  className={`inline-block h-2 w-2 rounded-full ${
                    status.isIsolated ? 'bg-emerald-500' : 'bg-amber-500'
                  }`}
                />
                <span>{isolationLabel}</span>
              </div>
            </div>
          ) : (
            <p className="text-xs text-[var(--color-text-secondary)]">
              {t('python.picker.notConfigured')}
            </p>
          )}

          {lastError && (
            <div className="mt-2 text-xs text-[var(--color-error)]">
              <button
                type="button"
                onClick={() => setShowErrorDetails((v) => !v)}
                className="inline-flex items-center gap-1 hover:underline"
              >
                <span>{t('python.picker.errorPrefix')}</span>
                {errorIsLong && (
                  <span className="text-[var(--color-text-tertiary)]">
                    [{showErrorDetails ? t('python.picker.collapse') : t('python.picker.expand')}]
                  </span>
                )}
              </button>
              {!errorIsLong || showErrorDetails ? (
                <pre className="mt-1 max-h-40 overflow-auto rounded-[var(--radius-sm)] border border-[var(--color-error)]/30 bg-[var(--color-error)]/8 p-2 text-[11px] whitespace-pre-wrap break-all">
                  {lastError}
                </pre>
              ) : (
                <span className="ml-1 text-[var(--color-text-secondary)]">
                  {errorLines[0]?.slice(0, 120) ?? ''}…
                </span>
              )}
            </div>
          )}

          {job && (
            <div className="mt-3">
              <button
                type="button"
                className="text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] inline-flex items-center gap-1"
                onClick={() => setShowLogs((v) => !v)}
              >
                <span>
                  {job.kind === 'creating'
                    ? t('python.picker.jobCreating')
                    : t('python.picker.jobInstalling')}
                </span>
                <span>[{showLogs ? t('python.picker.collapse') : t('python.picker.expand')}]</span>
              </button>
              {showLogs && (
                <pre className="mt-1 max-h-44 overflow-auto rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface-container-low)] p-2 text-[11px] text-[var(--color-text-secondary)] whitespace-pre-wrap break-all">
                  {job.lines.length === 0
                    ? t('python.picker.jobNoOutputYet')
                    : job.lines.slice(-30).join('\n')}
                </pre>
              )}
            </div>
          )}
        </section>

        <section className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-3">
          <h3 className="text-sm font-semibold text-[var(--color-text-primary)] mb-2">
            {t('python.picker.actionsTitle')}
          </h3>
          <div className="flex flex-wrap gap-2">
            <Button variant="primary" size="sm" disabled={busy} onClick={() => void handleCreate('uv')}>
              {t('python.picker.createUv')}
              {requiredVersionLabel ? ` (${requiredVersionLabel})` : ''}
            </Button>
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => void handleCreate('venv')}>
              {t('python.picker.createVenv')}
              {requiredVersionLabel ? ` (${requiredVersionLabel})` : ''}
            </Button>
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => void handleSelectOther()}>
              {t('python.picker.selectOther')}
            </Button>
            {installAvailable && (
              <Button
                variant="secondary"
                size="sm"
                disabled={busy}
                onClick={() => void handleInstall()}
                title={installRecommendation?.strategy ? STRATEGY_LABEL[installRecommendation.strategy] : ''}
              >
                {t('python.picker.installSmart')}
                {installRecommendation?.target ? ` (${installRecommendation.target})` : ''}
              </Button>
            )}
            {hasVenv && (
              <>
                <Button variant="secondary" size="sm" onClick={() => void handleOpenVenvDir()}>
                  {t('python.picker.openVenvDir')}
                </Button>
                <Button variant="secondary" size="sm" onClick={handleOpenTerminal}>
                  {t('python.picker.openTerminal')}
                </Button>
                <Button variant="danger" size="sm" disabled={busy} onClick={() => void handlePurge()}>
                  {t('python.picker.purge')}
                </Button>
              </>
            )}
          </div>
        </section>

        <section className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-4 py-3">
          <h3 className="text-sm font-semibold text-[var(--color-text-primary)] mb-2">
            {t('python.picker.candidatesTitle')}
          </h3>
          {discovery && discovery.interpreters.length > 0 ? (
            <ul className="flex flex-col gap-1">
              {discovery.interpreters.map((cand) => {
                const isCurrent =
                  currentInterpreterPath !== null && samePath(currentInterpreterPath, cand.path)
                return (
                  <li key={cand.path}>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void handleSelectCandidate(cand.path)}
                      className={`w-full rounded-[var(--radius-sm)] border px-3 py-2 text-left text-xs transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${
                        isCurrent
                          ? 'border-[var(--color-accent)] bg-[var(--color-accent)]/8 ring-1 ring-[var(--color-accent)]/40'
                          : 'border-[var(--color-border)] hover:bg-[var(--color-surface-hover)]'
                      }`}
                    >
                      <div className="flex items-center justify-between gap-2">
                        <span className="flex items-center gap-2 text-[var(--color-text-primary)]">
                          {isCurrent && (
                            <span
                              aria-hidden="true"
                              className="material-symbols-outlined text-[14px] text-[var(--color-accent)]"
                            >
                              check
                            </span>
                          )}
                          {cand.version ?? 'Python'}
                        </span>
                        <span className="text-[var(--color-text-tertiary)]">
                          {cand.isVenv ? '.venv' : cand.source}
                        </span>
                      </div>
                      <div className="text-[var(--color-text-secondary)] break-all">{cand.path}</div>
                    </button>
                  </li>
                )
              })}
            </ul>
          ) : (
            <p className="text-xs text-[var(--color-text-secondary)]">
              {t('python.picker.candidatesEmpty')}
            </p>
          )}
        </section>
      </div>
    </Modal>
  )
}

function joinPath(root: string, name: string): string {
  const sep =
    typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('win')
      ? '\\'
      : '/'
  if (root.endsWith(sep) || root.endsWith('/')) {
    return `${root}${name}`
  }
  return `${root}${sep}${name}`
}

function samePath(a: string, b: string): boolean {
  if (a === b) return true
  const isWin =
    typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('win')
  if (isWin) {
    return a.replace(/\\/g, '/').toLowerCase() === b.replace(/\\/g, '/').toLowerCase()
  }
  return false
}
