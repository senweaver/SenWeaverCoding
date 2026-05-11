import { getDefaultBaseUrl, setBaseUrl } from '../api/client'

function sleep(ms: number) {
  return new Promise<void>((resolve) => setTimeout(resolve, ms))
}

export function isTauriRuntime() {
  if (typeof window === 'undefined') return false
  return '__TAURI_INTERNALS__' in window || '__TAURI__' in window
}

const HEALTH_FETCH_MS = 3_000
const BROWSER_HEALTH_ATTEMPTS = 80
const RESTART_AFTER_STREAK_FAILURES = 24
const MIN_RESTART_INTERVAL_MS = 90_000

function healthFetchSignal(ms: number): AbortSignal | undefined {
  try {
    return AbortSignal.timeout(ms)
  } catch {
    return undefined
  }
}

async function waitForHealth(serverUrl: string, maxAttempts: number) {
  let lastError: unknown

  for (let attempt = 0; attempt < maxAttempts; attempt++) {
    try {
      const response = await fetch(`${serverUrl.replace(/\/$/, '')}/health`, {
        cache: 'no-store',
        signal: healthFetchSignal(HEALTH_FETCH_MS),
      })
      if (response.ok) {
        return
      }
      lastError = new Error(`healthcheck returned ${response.status}`)
    } catch (error) {
      lastError = error
    }

    await sleep(250)
  }

  throw lastError instanceof Error ? lastError : new Error('Local server healthcheck failed')
}

export async function fetchSettingsWithRetry(
  fetchAll: () => Promise<void>,
  options?: { signal?: AbortSignal },
) {
  let i = 0
  for (;;) {
    options?.signal?.throwIfAborted()
    try {
      await fetchAll()
      return
    } catch (error) {
      console.warn('[desktop] fetchSettings failed, retrying', error)
      const delay = Math.min(10_000, 400 + i * 150)
      i += 1
      await sleep(delay)
    }
  }
}

export async function initializeDesktopServerUrl(options?: { signal?: AbortSignal }) {
  const fallbackUrl = getDefaultBaseUrl()
  const queryUrl =
    typeof window !== 'undefined'
      ? new URLSearchParams(window.location.search).get('serverUrl')
      : null
  const requestedUrl = queryUrl?.trim() || fallbackUrl

  if (!isTauriRuntime()) {
    setBaseUrl(requestedUrl)
    for (;;) {
      options?.signal?.throwIfAborted()
      try {
        await waitForHealth(requestedUrl, BROWSER_HEALTH_ATTEMPTS)
        return requestedUrl
      } catch (error) {
        console.warn('[desktop] browser health check failed, retrying', error)
        await sleep(600)
      }
    }
  }

  const { invoke } = await import(/* @vite-ignore */ '@tauri-apps/api/core')

  let pendingTicks = 0
  let healthFailureStreak = 0
  let lastRestartAttemptAt = 0

  for (;;) {
    options?.signal?.throwIfAborted()
    let serverUrl: string | undefined
    try {
      const candidate = await invoke<string>('get_server_url')
      if (candidate.startsWith('http://') || candidate.startsWith('https://')) {
        serverUrl = candidate
      }
    } catch {
      pendingTicks += 1
      if (pendingTicks % 80 === 0) {
        console.info(
          `[desktop] embedded gateway still warming up (${(pendingTicks * 250) / 1000}s elapsed)`,
        )
      }
      await sleep(250)
      continue
    }

    if (!serverUrl) {
      await sleep(250)
      continue
    }

    setBaseUrl(serverUrl)
    try {
      await waitForHealth(serverUrl, BROWSER_HEALTH_ATTEMPTS)
      return serverUrl
    } catch (error) {
      healthFailureStreak += 1
      console.warn(
        `[desktop] /health probe failed (streak=${healthFailureStreak}); will retry`,
        error,
      )
      if (healthFailureStreak >= RESTART_AFTER_STREAK_FAILURES) {
        const now = Date.now()
        if (now - lastRestartAttemptAt >= MIN_RESTART_INTERVAL_MS) {
          lastRestartAttemptAt = now
          healthFailureStreak = 0
          try {
            await invoke<void>('restart_embedded_gateway')
            console.info('[desktop] requested embedded gateway restart')
          } catch (restartErr) {
            console.info(
              '[desktop] restart_embedded_gateway rejected (gateway likely still booting)',
              restartErr,
            )
          }
        }
      }
      await sleep(800)
    }
  }
}
