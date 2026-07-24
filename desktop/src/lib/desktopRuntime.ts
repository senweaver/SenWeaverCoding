// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { getDefaultBaseUrl, setAuthToken, setBaseUrl } from '../api/client'

function sleep(ms: number) {
  return new Promise<void>((resolve) => setTimeout(resolve, ms))
}

export function isTauriRuntime() {
  if (typeof window === 'undefined') return false
  return '__TAURI_INTERNALS__' in window || '__TAURI__' in window
}

const HEALTH_FETCH_MS = 2_500
const BROWSER_HEALTH_ATTEMPTS = 32
const HEALTH_BACKOFF_INITIAL_MS = 200
const HEALTH_BACKOFF_MAX_MS = 1_500
const RESTART_AFTER_STREAK_FAILURES = 3
const MIN_RESTART_INTERVAL_MS = 12_000
const STATUS_FALLBACK_POLL_MS = 5_000
const BOOTSTRAP_AUTO_RESTART_AT_MS = 60_000
const BOOTSTRAP_HARD_CAP_MS = 180_000
const BACKEND_STATE_EVENT = 'backend://state-change'

export type DesktopBootEventKind =
  | 'gateway-pending'
  | 'gateway-acquired'
  | 'health-failed'
  | 'health-restart-attempt'
  | 'health-ok'
  | 'bootstrap-failed'

export type DesktopBootEvent = {
  kind: DesktopBootEventKind
  elapsedMs: number
  detail?: string
}

export type DesktopBootObserver = (event: DesktopBootEvent) => void

export type ServerStatusState = 'pending' | 'starting' | 'ready' | 'failed'

export type ServerStatusSnapshot = {
  state: ServerStatusState
  url?: string
  error?: string
  elapsedMs?: number
  logDir?: string
}

function healthFetchSignal(ms: number): AbortSignal | undefined {
  try {
    return AbortSignal.timeout(ms)
  } catch {
    return undefined
  }
}

async function probeHealthOnce(serverUrl: string): Promise<boolean> {
  const response = await fetch(
    `${serverUrl.replace(/\/$/, '')}/health?_=${Date.now()}`,
    {
      cache: 'no-store',
      signal: healthFetchSignal(HEALTH_FETCH_MS),
    },
  )
  return response.ok
}

async function waitForHealth(
  serverUrl: string,
  maxAttempts: number,
  signal?: AbortSignal,
) {
  let lastError: unknown
  let delayMs = HEALTH_BACKOFF_INITIAL_MS

  for (let attempt = 0; attempt < maxAttempts; attempt++) {
    signal?.throwIfAborted()
    try {
      if (await probeHealthOnce(serverUrl)) {
        return
      }
      lastError = new Error('healthcheck non-2xx')
    } catch (error) {
      lastError = error
    }

    await sleep(delayMs)
    delayMs = Math.min(HEALTH_BACKOFF_MAX_MS, Math.round(delayMs * 1.6))
  }

  throw lastError instanceof Error ? lastError : new Error('Local server healthcheck failed')
}

const FETCH_SETTINGS_MAX_ATTEMPTS = 6
const FETCH_SETTINGS_BUDGET_MS = 20_000

export async function fetchSettingsWithRetry(
  fetchAll: () => Promise<void>,
  options?: { signal?: AbortSignal },
) {
  let attempt = 0
  let delay = 300
  const startedAt = Date.now()
  let lastError: unknown
  while (attempt < FETCH_SETTINGS_MAX_ATTEMPTS) {
    options?.signal?.throwIfAborted()
    try {
      await fetchAll()
      return
    } catch (error) {
      lastError = error
      console.warn('[desktop] fetchSettings failed, retrying', error)
      attempt += 1
      if (Date.now() - startedAt >= FETCH_SETTINGS_BUDGET_MS) {
        break
      }
      await sleep(delay)
      delay = Math.min(10_000, Math.round(delay * 1.6))
    }
  }
  console.warn(
    '[desktop] fetchSettings still failing after retries; rendering main UI in degraded mode so the user can open Settings to configure',
    lastError,
  )
}

type TauriInvoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>

let invokeRef: TauriInvoke | null = null

async function ensureInvoke(): Promise<TauriInvoke | null> {
  if (!isTauriRuntime()) return null
  if (invokeRef) return invokeRef
  const mod = (await import(/* @vite-ignore */ '@tauri-apps/api/core')) as {
    invoke: TauriInvoke
  }
  invokeRef = mod.invoke
  return invokeRef
}

export async function getServerStatusSnapshot(): Promise<ServerStatusSnapshot | null> {
  const invoke = await ensureInvoke()
  if (!invoke) return null
  try {
    const snapshot = await invoke<ServerStatusSnapshot>('get_server_status')
    return snapshot ?? null
  } catch {
    return null
  }
}

export async function requestGatewayRestart(force = false): Promise<boolean> {
  const invoke = await ensureInvoke()
  if (!invoke) return false
  try {
    await invoke<void>('restart_embedded_gateway', { force })
    return true
  } catch (error) {
    console.warn('[desktop] restart_embedded_gateway rejected', error)
    return false
  }
}

export async function openLogDir(): Promise<string | null> {
  const invoke = await ensureInvoke()
  if (!invoke) return null
  try {
    return await invoke<string>('open_log_dir')
  } catch (error) {
    console.warn('[desktop] open_log_dir failed', error)
    return null
  }
}

export type ServerStatusListener = (snapshot: ServerStatusSnapshot) => void

export async function subscribeServerStatus(
  listener: ServerStatusListener,
): Promise<() => void> {
  if (!isTauriRuntime()) return () => {}
  try {
    const { listen } = (await import(
      /* @vite-ignore */ '@tauri-apps/api/event'
    )) as {
      listen: <T>(
        event: string,
        handler: (event: { payload: T }) => void,
      ) => Promise<() => void>
    }
    const unlisten = await listen<ServerStatusSnapshot>(
      BACKEND_STATE_EVENT,
      (event) => {
        const payload = event.payload
        if (payload && typeof payload.state === 'string') {
          listener(payload)
        }
      },
    )
    return unlisten
  } catch (error) {
    console.warn('[desktop] subscribe backend state failed', error)
    return () => {}
  }
}

export async function initializeDesktopServerUrl(options?: {
  signal?: AbortSignal
  onEvent?: DesktopBootObserver
}) {
  const fallbackUrl = getDefaultBaseUrl()
  const queryUrl =
    typeof window !== 'undefined'
      ? new URLSearchParams(window.location.search).get('serverUrl')
      : null
  const requestedUrl = queryUrl?.trim() || fallbackUrl
  const startedAt = Date.now()
  const emit = options?.onEvent
  const notify = (kind: DesktopBootEventKind, detail?: string) => {
    if (!emit) return
    emit({ kind, elapsedMs: Date.now() - startedAt, detail })
  }

  if (!isTauriRuntime()) {
    setBaseUrl(requestedUrl)
    let pollDelay = HEALTH_BACKOFF_INITIAL_MS
    for (;;) {
      options?.signal?.throwIfAborted()
      try {
        await waitForHealth(requestedUrl, BROWSER_HEALTH_ATTEMPTS, options?.signal)
        notify('health-ok')
        return requestedUrl
      } catch (error) {
        notify('health-failed', error instanceof Error ? error.message : String(error))
        console.warn('[desktop] browser health check failed, retrying', error)
        await sleep(pollDelay)
        pollDelay = Math.min(HEALTH_BACKOFF_MAX_MS, Math.round(pollDelay * 1.6))
      }
    }
  }

  const invoke = (await ensureInvoke()) as TauriInvoke

  let pendingTicks = 0
  let healthFailureStreak = 0
  let lastRestartAttemptAt = 0
  let urlPollDelay = HEALTH_BACKOFF_INITIAL_MS
  let autoForceRestartFired = false
  let hardCapReportedAt = 0
  let lastObservedGenerationStartMs = 0

  const refreshGenerationWindow = async () => {
    try {
      const snap = await invoke<ServerStatusSnapshot>('get_server_status')
      if (snap && typeof snap.elapsedMs === 'number') {
        const observedStart = Date.now() - snap.elapsedMs
        if (observedStart > lastObservedGenerationStartMs + 5_000) {
          lastObservedGenerationStartMs = observedStart
          autoForceRestartFired = false
          hardCapReportedAt = 0
        }
      }
    } catch {
    }
  }

  const generationElapsed = () =>
    lastObservedGenerationStartMs > 0
      ? Date.now() - lastObservedGenerationStartMs
      : Date.now() - startedAt

  const maybeAutoForceRestart = async () => {
    if (autoForceRestartFired) return
    if (generationElapsed() < BOOTSTRAP_AUTO_RESTART_AT_MS) return
    autoForceRestartFired = true
    notify('health-restart-attempt', 'auto-force-restart')
    try {
      await invoke<void>('restart_embedded_gateway', { force: true })
      console.info('[desktop] auto force-restart requested after 60s without ready')
    } catch (err) {
      console.info('[desktop] auto force-restart rejected', err)
    }
  }

  const maybeReportFinalFailure = (detail: string) => {
    if (generationElapsed() < BOOTSTRAP_HARD_CAP_MS) return
    if (hardCapReportedAt > 0 && Date.now() - hardCapReportedAt < BOOTSTRAP_HARD_CAP_MS) {
      return
    }
    hardCapReportedAt = Date.now()
    notify('bootstrap-failed', detail)
  }

  for (;;) {
    options?.signal?.throwIfAborted()
    await refreshGenerationWindow()
    maybeReportFinalFailure('hard cap reached without ready')
    await maybeAutoForceRestart()
    let serverUrl: string | undefined
    try {
      const candidate = await invoke<string>('get_server_url')
      if (candidate.startsWith('http://') || candidate.startsWith('https://')) {
        serverUrl = candidate
      }
    } catch {
      pendingTicks += 1
      if (pendingTicks % 40 === 0) {
        notify('gateway-pending')
        console.info(
          `[desktop] embedded gateway still warming up (${Math.round((Date.now() - startedAt) / 1000)}s elapsed)`,
        )
      }
      await sleep(urlPollDelay)
      urlPollDelay = Math.min(HEALTH_BACKOFF_MAX_MS, Math.round(urlPollDelay * 1.6))
      continue
    }

    if (!serverUrl) {
      await sleep(urlPollDelay)
      urlPollDelay = Math.min(HEALTH_BACKOFF_MAX_MS, Math.round(urlPollDelay * 1.6))
      continue
    }

    urlPollDelay = HEALTH_BACKOFF_INITIAL_MS
    notify('gateway-acquired', serverUrl)
    setBaseUrl(serverUrl)
    try {
      const token = await invoke<string>('get_server_token')
      if (token) {
        setAuthToken(token)
      }
    } catch {
    }
    try {
      await waitForHealth(serverUrl, BROWSER_HEALTH_ATTEMPTS, options?.signal)
      notify('health-ok')
      return serverUrl
    } catch (error) {
      healthFailureStreak += 1
      const msg = error instanceof Error ? error.message : String(error)
      notify('health-failed', msg)
      console.warn(
        `[desktop] /health probe failed (streak=${healthFailureStreak}); will retry`,
        error,
      )
      if (healthFailureStreak >= RESTART_AFTER_STREAK_FAILURES) {
        const now = Date.now()
        if (now - lastRestartAttemptAt >= MIN_RESTART_INTERVAL_MS) {
          lastRestartAttemptAt = now
          healthFailureStreak = 0
          notify('health-restart-attempt')
          try {
            await invoke<void>('restart_embedded_gateway', { force: false })
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

export const DESKTOP_RUNTIME_TUNABLES = {
  STATUS_FALLBACK_POLL_MS,
  BOOTSTRAP_AUTO_RESTART_AT_MS,
  BOOTSTRAP_HARD_CAP_MS,
}
