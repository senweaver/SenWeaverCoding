import { getDefaultBaseUrl, setBaseUrl } from '../api/client'

function sleep(ms: number) {
  return new Promise<void>((resolve) => setTimeout(resolve, ms))
}

export function isTauriRuntime() {
  if (typeof window === 'undefined') return false
  return '__TAURI_INTERNALS__' in window || '__TAURI__' in window
}

const HEALTH_FETCH_MS = 2_500

const BROWSER_HEALTH_ATTEMPTS = 40
const TAURI_HEALTH_ATTEMPTS = 256

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

  for (;;) {
    options?.signal?.throwIfAborted()
    let serverUrl: string | undefined
    try {
      const candidate = await invoke<string>('get_server_url')
      if (candidate.startsWith('http://') || candidate.startsWith('https://')) {
        serverUrl = candidate
      }
    } catch {

      await sleep(150)
      continue
    }

    if (!serverUrl) {
      await sleep(150)
      continue
    }

    setBaseUrl(serverUrl)
    try {
      await waitForHealth(serverUrl, TAURI_HEALTH_ATTEMPTS)
      return serverUrl
    } catch (error) {
      console.warn('[desktop] /health failed with a known gateway URL — restarting gateway', error)
      try {
        await invoke<void>('restart_embedded_gateway')
      } catch (restartErr) {
        console.warn('[desktop] restart_embedded_gateway invoke failed', restartErr)
      }
      await sleep(600)
    }
  }
}
