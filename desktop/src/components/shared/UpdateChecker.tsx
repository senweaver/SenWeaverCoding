import { useEffect } from 'react'
import { useTranslation } from '../../i18n'
import { MarkdownRenderer } from '../markdown/MarkdownRenderer'
import { isTauriRuntime } from '../../lib/desktopRuntime'
import { useUpdateStore } from '../../stores/updateStore'
import { useDockEdgeOffset } from '../../hooks/useDockEdgeOffset'
import { formatBytes } from '../../lib/formatBytes'

const UP_TO_DATE_AUTO_DISMISS_MS = 4000
const ERROR_AUTO_DISMISS_MS = 6000

export function UpdateChecker() {
  const t = useTranslation()
  const status = useUpdateStore((s) => s.status)
  const availableVersion = useUpdateStore((s) => s.availableVersion)
  const currentVersion = useUpdateStore((s) => s.currentVersion)
  const latestVersion = useUpdateStore((s) => s.latestVersion)
  const releaseNotes = useUpdateStore((s) => s.releaseNotes)
  const progressPercent = useUpdateStore((s) => s.progressPercent)
  const downloadedBytes = useUpdateStore((s) => s.downloadedBytes)
  const totalBytes = useUpdateStore((s) => s.totalBytes)
  const error = useUpdateStore((s) => s.error)
  const shouldPrompt = useUpdateStore((s) => s.shouldPrompt)
  const manualCheckActive = useUpdateStore((s) => s.manualCheckActive)
  const initialize = useUpdateStore((s) => s.initialize)
  const installUpdate = useUpdateStore((s) => s.installUpdate)
  const dismissPrompt = useUpdateStore((s) => s.dismissPrompt)
  const clearManualCheck = useUpdateStore((s) => s.clearManualCheck)
  const dockEdgeOffset = useDockEdgeOffset()
  const floaterStyle = {
    top: '0.75rem',
    left:
      dockEdgeOffset > 0
        ? `calc(50vw - ${dockEdgeOffset / 2}px)`
        : '50vw',
    transform: 'translateX(-50%)',
  } as const

  useEffect(() => {
    void initialize()
  }, [initialize])

  useEffect(() => {
    if (!manualCheckActive) return
    if (status === 'up-to-date') {
      const handle = window.setTimeout(clearManualCheck, UP_TO_DATE_AUTO_DISMISS_MS)
      return () => window.clearTimeout(handle)
    }
    if (status === 'error') {
      const handle = window.setTimeout(clearManualCheck, ERROR_AUTO_DISMISS_MS)
      return () => window.clearTimeout(handle)
    }
    if (status === 'available' || status === 'downloading' || status === 'restarting') {
      clearManualCheck()
    }
    return undefined
  }, [manualCheckActive, status, clearManualCheck])

  if (!isTauriRuntime()) return null

  const showUpdateCard =
    shouldPrompt && !!availableVersion && ['available', 'downloading', 'restarting'].includes(status)

  const showCheckingToast = manualCheckActive && status === 'checking'
  const showUpToDateToast = manualCheckActive && status === 'up-to-date'
  const showErrorToast = manualCheckActive && status === 'error'

  if (!showUpdateCard && !showCheckingToast && !showUpToDateToast && !showErrorToast) {
    return null
  }

  if (showCheckingToast) {
    return (
      <div className="fixed z-[200] max-w-sm" style={floaterStyle}>
        <div className="flex items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-4 py-2 shadow-[var(--shadow-dropdown)]">
          <span
            className="inline-block h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-[var(--color-text-accent)] border-t-transparent"
            aria-hidden="true"
          />
          <p className="text-sm text-[var(--color-text-primary)]">{t('update.toast.checking')}</p>
        </div>
      </div>
    )
  }

  if (showUpToDateToast) {
    const versionLabel = latestVersion ?? currentVersion ?? ''
    return (
      <div className="fixed z-[200] max-w-sm" style={floaterStyle}>
        <div className="flex items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-4 py-2 shadow-[var(--shadow-dropdown)]">
          <p className="flex-1 text-sm text-[var(--color-text-primary)]">
            {t('update.toast.upToDate', { version: versionLabel })}
          </p>
          <button
            type="button"
            onClick={clearManualCheck}
            className="text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors"
          >
            {t('update.toast.dismiss')}
          </button>
        </div>
      </div>
    )
  }

  if (showErrorToast) {
    return (
      <div className="fixed z-[200] max-w-sm" style={floaterStyle}>
        <div className="flex items-center gap-3 rounded-[var(--radius-lg)] border border-[var(--color-error)]/40 bg-[var(--color-surface-container-low)] px-4 py-2 shadow-[var(--shadow-dropdown)]">
          <p className="flex-1 text-sm text-[var(--color-error)]">
            {t('update.toast.error', { error: error ?? '' })}
          </p>
          <button
            type="button"
            onClick={clearManualCheck}
            className="text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors"
          >
            {t('update.toast.dismiss')}
          </button>
        </div>
      </div>
    )
  }

  const hasKnownProgress = typeof totalBytes === 'number' && totalBytes > 0
  const downloadedText = formatBytes(downloadedBytes)
  const statusText =
    status === 'restarting'
      ? t('update.restarting')
      : status === 'downloading'
        ? hasKnownProgress
          ? t('update.downloading')
          : t('update.progressBytes', { downloaded: downloadedText })
        : null

  return (
    <div className="fixed z-[200] max-w-sm" style={floaterStyle}>
      <div className="bg-[var(--color-surface-container-low)] border border-[var(--color-border)] rounded-[var(--radius-lg)] shadow-[var(--shadow-dropdown)] p-4">
        <p className="text-sm font-medium text-[var(--color-text-primary)]">
          {t('update.available', { version: availableVersion ?? '' })}
        </p>

        {releaseNotes && (
          <div className="mt-2 max-h-40 overflow-y-auto rounded-lg border border-[var(--color-border)]/60 bg-[var(--color-surface)]/70 px-3 py-2">
            <MarkdownRenderer
              content={releaseNotes}
              className="text-xs leading-5 text-[var(--color-text-secondary)] [&_h1]:mb-2 [&_h1]:text-sm [&_h1]:font-semibold [&_h2]:mb-1.5 [&_h2]:text-xs [&_h2]:font-semibold [&_p]:my-1.5 [&_p]:text-xs [&_p]:leading-5 [&_ul]:my-1.5 [&_ol]:my-1.5"
            />
          </div>
        )}

        {(status === 'downloading' || status === 'restarting') && (
          <div className="mt-3">
            <div className="h-1.5 bg-[var(--color-surface)] rounded-full overflow-hidden">
              {hasKnownProgress || status === 'restarting' ? (
                <div
                  className="h-full bg-[var(--color-text-accent)] transition-all duration-300"
                  style={{ width: `${Math.min(progressPercent, 100)}%` }}
                />
              ) : (
                <div className="h-full w-1/3 rounded-full bg-[var(--color-text-accent)]/75 animate-pulse" />
              )}
            </div>
            {statusText && (
              <p className="text-xs text-[var(--color-text-tertiary)] mt-1">
                {statusText}
                {status === 'downloading' && hasKnownProgress ? ` ${progressPercent}%` : ''}
              </p>
            )}
          </div>
        )}

        {error && (
          <p className="mt-2 text-xs text-[var(--color-error)]">
            {t('update.failed', { error })}
          </p>
        )}

        {status === 'available' && (
          <div className="mt-3 flex gap-2">
            <button
              onClick={() => void installUpdate()}
              className="px-3 py-1 text-xs font-medium rounded-[var(--radius-md)] bg-[var(--color-text-accent)] text-white hover:opacity-90 transition-opacity"
            >
              {t('update.now')}
            </button>
            <button
              onClick={dismissPrompt}
              className="px-3 py-1 text-xs text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors"
            >
              {t('update.later')}
            </button>
          </div>
        )}
      </div>
    </div>
  )
}
