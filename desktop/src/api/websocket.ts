// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import type { ClientMessage, ServerMessage } from '../types/chat'
import { getAuthToken, getBaseUrl } from './client'

type MessageHandler = (msg: ServerMessage) => void
type ConnectListener = (sessionId: string) => void

const CONNECT_TIMEOUT_MS = 10_000
const PING_INTERVAL_MS = 30_000
const PONG_TIMEOUT_MS = 45_000
const PONG_WATCHER_TICK_MS = 5_000
const MAX_RECONNECT_ATTEMPTS = 12
const MAX_RECONNECT_DELAY_MS = 30_000
const PENDING_MAX_COUNT = 200
const PENDING_MAX_BYTES = 262_144
const MAX_CONCURRENT_CONNECTIONS = 32
const HANDLER_ERROR_NOTIFY_THROTTLE_MS = 10_000

const USER_OUTBOX_MAX_ENTRIES = 50
const USER_OUTBOX_STORAGE_MAX_BYTES = 524_288
const USER_OUTBOX_STORAGE_PREFIX = 'sen.userOutbox.'

const FRAME_WORKER_PARSE_MIN_BYTES = 262_144
const FRAME_QUEUE_MAX = 2048
const FRAME_PARSE_TIMEOUT_MS = 10_000

type OutboxEntry = {
  clientMsgId: string
  serialized: string
  enqueuedAt: number
}

const CRITICAL_MESSAGE_TYPES = new Set<string>([
  'user_message',
  'stop_generation',
  'approval_decision',
  'permission_response',
  'set_runtime_config',
  'start_design_generation',
  'start_plan_execution',
  'set_debug_submode',
  'set_permission_mode',
  'set_coding_mode',
  'set_pii_config',
])

function isCriticalMessage(message: ClientMessage): boolean {
  const t = (message as { type?: string }).type
  if (!t) return false
  if (CRITICAL_MESSAGE_TYPES.has(t)) return true
  if (t.startsWith('debug_')) return true
  if (t.startsWith('approval_')) return true
  return false
}

function encodeWebSocketToken(token: string): string {
  const bytes = new TextEncoder().encode(token)
  let binary = ''
  for (const byte of bytes) {
    binary += String.fromCharCode(byte)
  }
  return btoa(binary).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/g, '')
}

function serializeMessage(message: ClientMessage): string | null {
  try {
    return JSON.stringify(message)
  } catch {
    return null
  }
}

type ConnectionState = 'idle' | 'connecting' | 'open' | 'closed' | 'abandoned'

type FrameJob = {
  raw: string
  done: boolean
  ok: boolean
  msg?: ServerMessage
}

type PendingEntry = {
  message: ClientMessage
  serialized: string
  bytes: number
  critical: boolean
  enqueuedAt: number
}

type Connection = {
  ws: WebSocket
  handlers: Set<MessageHandler>
  reconnectTimer: ReturnType<typeof setTimeout> | null
  connectTimer: ReturnType<typeof setTimeout> | null
  reconnectAttempt: number
  pingInterval: ReturnType<typeof setInterval> | null
  pongWatcher: ReturnType<typeof setInterval> | null
  intentionalClose: boolean
  pendingMessages: PendingEntry[]
  pendingBytes: number
  lastPongAt: number
  lastActivityAt: number
  state: ConnectionState
  errorCount: number
  parseFailures: number
  pathPrefix: string
  lastHandlerErrorNotifyAt: number
  lastServerSeq: number
  lastSeqGapNotifyAt: number
  frameQueue: FrameJob[]
}

class WebSocketManager {
  private connections = new Map<string, Connection>()
  private connectListeners = new Set<ConnectListener>()
  private runtimeConfigResolvers = new Map<string, (success: boolean) => void>()
  private preRegisteredHandlers = new Map<string, Set<MessageHandler>>()
  private userOutbox = new Map<string, Map<string, OutboxEntry>>()
  private userOutboxLoaded = new Set<string>()
  private frameWorker: Worker | null = null
  private frameWorkerFailed = false
  private nextFrameRequestId = 1
  private framePending = new Map<
    number,
    {
      job: FrameJob
      sessionId: string
      conn: Connection
      timer: ReturnType<typeof setTimeout>
    }
  >()

  private parseFrameJobSync(job: FrameJob) {
    try {
      job.msg = JSON.parse(job.raw) as ServerMessage
      job.ok = true
    } catch {
      job.ok = false
    }
    job.done = true
  }

  private ensureFrameWorker(): Worker | null {
    if (this.frameWorkerFailed) return null
    if (this.frameWorker) return this.frameWorker
    try {
      this.frameWorker = new Worker(
        new URL('../workers/frameParse.worker.ts', import.meta.url),
        { type: 'module' },
      )
      this.frameWorker.onmessage = (
        event: MessageEvent<{ id: number; ok: boolean; result?: ServerMessage }>,
      ) => {
        const { id, ok, result } = event.data
        const entry = this.framePending.get(id)
        if (!entry) return
        this.framePending.delete(id)
        clearTimeout(entry.timer)
        if (ok && result !== undefined) {
          entry.job.msg = result
          entry.job.ok = true
          entry.job.done = true
        } else {
          this.parseFrameJobSync(entry.job)
        }
        this.drainFrameQueue(entry.sessionId, entry.conn)
      }
      this.frameWorker.onerror = () => {
        this.frameWorkerFailed = true
        const failed = this.frameWorker
        this.frameWorker = null
        try {
          failed?.terminate()
        } catch (err) {
          console.warn('[wsManager] frame worker terminate failed', err)
        }
        this.settlePendingFrameJobsSync()
      }
    } catch {
      this.frameWorkerFailed = true
      this.frameWorker = null
    }
    return this.frameWorker
  }

  private settlePendingFrameJobsSync() {
    const entries = [...this.framePending.values()]
    this.framePending.clear()
    for (const entry of entries) {
      clearTimeout(entry.timer)
      if (!entry.job.done) this.parseFrameJobSync(entry.job)
    }
    for (const entry of entries) {
      this.drainFrameQueue(entry.sessionId, entry.conn)
    }
  }

  private requestFrameParse(sessionId: string, conn: Connection, job: FrameJob): boolean {
    const worker = this.ensureFrameWorker()
    if (!worker) return false
    const id = this.nextFrameRequestId++
    const timer = setTimeout(() => {
      const entry = this.framePending.get(id)
      if (!entry) return
      this.framePending.delete(id)
      console.warn('[wsManager] frame parse worker timed out; falling back to sync parse')
      const stalled = this.frameWorker
      this.frameWorker = null
      try {
        stalled?.terminate()
      } catch (err) {
        console.warn('[wsManager] stalled frame worker terminate failed', err)
      }
      if (!entry.job.done) this.parseFrameJobSync(entry.job)
      this.drainFrameQueue(entry.sessionId, entry.conn)
      this.settlePendingFrameJobsSync()
    }, FRAME_PARSE_TIMEOUT_MS)
    this.framePending.set(id, { job, sessionId, conn, timer })
    try {
      worker.postMessage({ id, raw: job.raw })
      return true
    } catch (err) {
      console.warn('[wsManager] frame worker postMessage failed', err)
      this.framePending.delete(id)
      clearTimeout(timer)
      return false
    }
  }

  private drainFrameQueue(sessionId: string, conn: Connection) {
    while (conn.frameQueue.length > 0 && conn.frameQueue[0]?.done) {
      const job = conn.frameQueue.shift()
      if (!job) break
      if (!job.ok) {
        conn.parseFailures++
        console.warn(
          `[wsManager] ws message parse failure (session=${sessionId}, total=${conn.parseFailures})`,
        )
        continue
      }
      this.deliverFrame(sessionId, conn, job.msg as ServerMessage)
    }
  }

  private deliverFrame(sessionId: string, conn: Connection, msg: ServerMessage) {
    if (
      typeof msg !== 'object' ||
      msg === null ||
      typeof (msg as { type?: unknown }).type !== 'string'
    ) {
      conn.parseFailures++
      console.warn(
        `[wsManager] ws message failed shape validation (session=${sessionId}, total=${conn.parseFailures})`,
      )
      return
    }
    const msgType = (msg as { type?: string }).type
    const frameSeq = (msg as { seq?: unknown }).seq
    if (typeof frameSeq === 'number' && Number.isFinite(frameSeq)) {
      if (conn.lastServerSeq > 0 && frameSeq > conn.lastServerSeq + 1) {
        const missed = frameSeq - conn.lastServerSeq - 1
        console.warn(
          `[wsManager] frame sequence gap (session=${sessionId}, missed=${missed})`,
        )
        const now = Date.now()
        if (now - conn.lastSeqGapNotifyAt > HANDLER_ERROR_NOTIFY_THROTTLE_MS) {
          conn.lastSeqGapNotifyAt = now
          this.broadcastSystemNotification(
            sessionId,
            'ws_frame_gap',
            'Frame sequence gap detected; client state may be out of sync.',
            { missed },
          )
        }
      }
      if (frameSeq > conn.lastServerSeq) {
        conn.lastServerSeq = frameSeq
      }
    }
    if (msgType === 'pong') {
      conn.lastPongAt = Date.now()
      return
    }
    let handlerFailed = false
    for (const handler of conn.handlers) {
      try {
        handler(msg)
      } catch (err) {
        handlerFailed = true
        console.warn(`[wsManager] handler threw for session ${sessionId}`, err)
      }
    }
    if (handlerFailed && msgType !== 'system_notification') {
      const now = Date.now()
      if (now - conn.lastHandlerErrorNotifyAt > HANDLER_ERROR_NOTIFY_THROTTLE_MS) {
        conn.lastHandlerErrorNotifyAt = now
        this.broadcastSystemNotification(
          sessionId,
          'ws_handler_error',
          'A message handler failed; client state may be out of sync.',
          { messageType: msgType },
        )
      }
    }
  }

  private outboxFor(sessionId: string): Map<string, OutboxEntry> {
    if (!this.userOutboxLoaded.has(sessionId)) {
      this.userOutboxLoaded.add(sessionId)
      try {
        const raw = sessionStorage.getItem(`${USER_OUTBOX_STORAGE_PREFIX}${sessionId}`)
        if (raw) {
          const parsed = JSON.parse(raw) as OutboxEntry[]
          if (Array.isArray(parsed)) {
            const restored = new Map<string, OutboxEntry>()
            for (const entry of parsed) {
              if (
                entry &&
                typeof entry.clientMsgId === 'string' &&
                typeof entry.serialized === 'string'
              ) {
                restored.set(entry.clientMsgId, {
                  clientMsgId: entry.clientMsgId,
                  serialized: entry.serialized,
                  enqueuedAt:
                    typeof entry.enqueuedAt === 'number' ? entry.enqueuedAt : Date.now(),
                })
              }
            }
            if (restored.size > 0) {
              const existing = this.userOutbox.get(sessionId)
              if (existing) {
                for (const [key, value] of restored) {
                  if (!existing.has(key)) existing.set(key, value)
                }
              } else {
                this.userOutbox.set(sessionId, restored)
              }
            }
          }
        }
      } catch (err) {
        console.warn('[wsManager] user outbox restore failed', err)
      }
    }
    let box = this.userOutbox.get(sessionId)
    if (!box) {
      box = new Map()
      this.userOutbox.set(sessionId, box)
    }
    return box
  }

  private persistOutbox(sessionId: string) {
    try {
      const box = this.userOutbox.get(sessionId)
      const key = `${USER_OUTBOX_STORAGE_PREFIX}${sessionId}`
      if (!box || box.size === 0) {
        sessionStorage.removeItem(key)
        return
      }
      const entries = [...box.values()]
        .filter((e) => e.serialized.length <= USER_OUTBOX_STORAGE_MAX_BYTES)
        .sort((a, b) => a.enqueuedAt - b.enqueuedAt)
      if (entries.length === 0) {
        sessionStorage.removeItem(key)
        return
      }
      sessionStorage.setItem(key, JSON.stringify(entries))
    } catch (err) {
      console.warn('[wsManager] user outbox persist failed', err)
    }
  }

  private registerUserMessage(sessionId: string, message: ClientMessage) {
    const clientMsgId = (message as { clientMsgId?: unknown }).clientMsgId
    if (typeof clientMsgId !== 'string' || clientMsgId.length === 0) return
    const serialized = serializeMessage(message)
    if (serialized === null) return
    const box = this.outboxFor(sessionId)
    box.set(clientMsgId, { clientMsgId, serialized, enqueuedAt: Date.now() })
    while (box.size > USER_OUTBOX_MAX_ENTRIES) {
      const oldest = [...box.values()].sort((a, b) => a.enqueuedAt - b.enqueuedAt)[0]
      if (!oldest) break
      box.delete(oldest.clientMsgId)
      console.warn(
        `[wsManager] user outbox overflow; dropping oldest clientMsgId=${oldest.clientMsgId} session=${sessionId}`,
      )
    }
    this.persistOutbox(sessionId)
  }

  confirmUserMessage(sessionId: string, clientMsgId: string) {
    const box = this.outboxFor(sessionId)
    if (box.delete(clientMsgId)) {
      this.persistOutbox(sessionId)
    }
  }

  retryUserMessage(sessionId: string, clientMsgId: string) {
    const entry = this.outboxFor(sessionId).get(clientMsgId)
    if (!entry) return
    const conn = this.connections.get(sessionId)
    if (conn && conn.ws.readyState === WebSocket.OPEN) {
      try {
        conn.ws.send(entry.serialized)
        return
      } catch (err) {
        console.warn('[wsManager] retryUserMessage send failed', err)
      }
    }
    this.connect(sessionId, { force: this.isAbandoned(sessionId) })
  }

  private resendUserOutbox(sessionId: string, conn: Connection) {
    const box = this.outboxFor(sessionId)
    if (box.size === 0) return
    const entries = [...box.values()].sort((a, b) => a.enqueuedAt - b.enqueuedAt)
    for (const entry of entries) {
      try {
        conn.ws.send(entry.serialized)
      } catch (err) {
        console.warn('[wsManager] user outbox resend failed', err)
        break
      }
    }
  }

  notifyRuntimeConfigUpdated(sessionId: string, requestId: string, success = true) {
    const key = `${sessionId}\u0000${requestId}`
    const resolver = this.runtimeConfigResolvers.get(key)
    if (!resolver) return
    this.runtimeConfigResolvers.delete(key)
    resolver(success)
  }

  waitForRuntimeConfigUpdated(
    sessionId: string,
    requestId: string,
    timeoutMs = 5000,
  ): Promise<boolean> {
    return new Promise((resolve, reject) => {
      const key = `${sessionId}\u0000${requestId}`
      const timer = setTimeout(() => {
        if (this.runtimeConfigResolvers.get(key) !== resolver) return
        this.runtimeConfigResolvers.delete(key)
        reject(new Error('runtime config sync timeout'))
      }, timeoutMs)
      const resolver = (success: boolean) => {
        clearTimeout(timer)
        resolve(success)
      }
      this.runtimeConfigResolvers.set(key, resolver)
    })
  }

  sendRuntimeConfig(
    sessionId: string,
    selection: { providerId: string; modelId: string },
    options?: { persist?: boolean },
  ): Promise<boolean> {
    const persist = options?.persist ?? true
    const requestId = crypto.randomUUID()
    const wait = this.waitForRuntimeConfigUpdated(sessionId, requestId)
    this.send(sessionId, {
      type: 'set_runtime_config',
      requestId,
      persist,
      providerId: selection.providerId,
      modelId: selection.modelId,
    })
    return wait.catch(() => false)
  }

  isConnected(sessionId: string): boolean {
    const conn = this.connections.get(sessionId)
    return conn?.ws.readyState === WebSocket.OPEN
  }

  isAbandoned(sessionId: string): boolean {
    return this.connections.get(sessionId)?.state === 'abandoned'
  }

  onConnected(listener: ConnectListener): () => void {
    this.connectListeners.add(listener)
    return () => {
      this.connectListeners.delete(listener)
    }
  }

  private notifyConnected(sessionId: string) {
    for (const listener of this.connectListeners) {
      try {
        listener(sessionId)
      } catch (err) {
        console.warn('[wsManager] onConnected listener failed', err)
      }
    }
  }

  private broadcastSystemNotification(
    sessionId: string,
    subtype: string,
    message: string,
    data?: unknown,
    level: 'info' | 'warning' | 'error' = 'warning',
  ) {
    const conn = this.connections.get(sessionId)
    if (!conn) return
    const payload: ServerMessage = {
      type: 'system_notification',
      subtype,
      level,
      message,
      data,
    }
    for (const handler of conn.handlers) {
      try {
        handler(payload)
      } catch (err) {
        console.warn('[wsManager] system_notification dispatch failed', err)
      }
    }
  }

  getConnectedSessionIds(): string[] {
    return [...this.connections.keys()]
  }

  connect(sessionId: string, options?: { pathPrefix?: string; force?: boolean }) {
    const existing = this.connections.get(sessionId)
    if (existing && existing.state === 'abandoned') {
      existing.state = 'idle'
      existing.reconnectAttempt = 0
    }
    if (options?.force && existing) {
      existing.intentionalClose = true
      this.stopTimers(existing)
      if (existing.reconnectTimer) {
        clearTimeout(existing.reconnectTimer)
        existing.reconnectTimer = null
      }
      if (existing.connectTimer) {
        clearTimeout(existing.connectTimer)
        existing.connectTimer = null
      }
      existing.reconnectAttempt = 0
      try {
        existing.ws.onmessage = null
        existing.ws.close()
      } catch (err) {
        console.warn('[wsManager] force reconnect close failed', err)
      }
    }
    if (
      !options?.force &&
      existing &&
      !existing.intentionalClose &&
      (
        existing.ws.readyState === WebSocket.OPEN ||
        existing.ws.readyState === WebSocket.CONNECTING ||
        existing.reconnectTimer !== null
      )
    ) {
      return
    }

    this.enforceConnectionPoolLimit(sessionId)

    const wsUrl = getBaseUrl().replace(/^http/, 'ws')
    const pathPrefix = options?.pathPrefix ?? existing?.pathPrefix ?? '/ws'
    let ws: WebSocket
    try {
      const token = getAuthToken()
      ws = token
        ? new WebSocket(`${wsUrl}${pathPrefix}/${sessionId}`, [
            `bearer64.${encodeWebSocketToken(token)}`,
          ])
        : new WebSocket(`${wsUrl}${pathPrefix}/${sessionId}`)
    } catch (err) {
      console.warn('[wsManager] WebSocket constructor threw', err)
      const placeholder = existing ?? this.createEmptyConnection()
      placeholder.state = 'closed'
      placeholder.intentionalClose = false
      this.connections.set(sessionId, placeholder)
      this.scheduleReconnect(sessionId, placeholder)
      return
    }

    const handlers = existing?.handlers ?? new Set<MessageHandler>()
    {
      const pre = this.preRegisteredHandlers.get(sessionId)
      if (pre) {
        for (const h of pre) handlers.add(h)
        this.preRegisteredHandlers.delete(sessionId)
      }
    }
    const conn: Connection = {
      ws,
      handlers,
      reconnectTimer: null,
      connectTimer: null,
      reconnectAttempt: existing?.reconnectAttempt ?? 0,
      pingInterval: null,
      pongWatcher: null,
      intentionalClose: false,
      pendingMessages: existing?.pendingMessages ?? [],
      pendingBytes: existing?.pendingBytes ?? 0,
      lastPongAt: 0,
      lastActivityAt: Date.now(),
      state: 'connecting',
      errorCount: existing?.errorCount ?? 0,
      parseFailures: existing?.parseFailures ?? 0,
      pathPrefix,
      lastHandlerErrorNotifyAt: 0,
      lastServerSeq: 0,
      lastSeqGapNotifyAt: 0,
      frameQueue: [],
    }
    this.connections.set(sessionId, conn)

    conn.connectTimer = setTimeout(() => {
      if (conn.ws.readyState === WebSocket.CONNECTING) {
        console.warn(`[wsManager] connect timeout for session ${sessionId}`)
        try {
          conn.ws.close()
        } catch (err) {
          console.warn('[wsManager] forced close after connect timeout failed', err)
        }
      }
    }, CONNECT_TIMEOUT_MS)

    ws.onopen = () => {
      if (conn.connectTimer) {
        clearTimeout(conn.connectTimer)
        conn.connectTimer = null
      }
      conn.state = 'open'
      conn.reconnectAttempt = 0
      conn.lastPongAt = Date.now()
      conn.lastActivityAt = Date.now()
      this.startPingLoop(sessionId, conn)
      this.startPongWatcher(sessionId, conn)
      this.notifyConnected(sessionId)
      this.flushPendingMessages(conn)
      this.resendUserOutbox(sessionId, conn)
    }

    ws.onmessage = (event) => {
      conn.lastActivityAt = Date.now()
      const raw = event.data
      if (typeof raw !== 'string') {
        conn.parseFailures++
        console.warn(
          `[wsManager] non-text ws frame ignored (session=${sessionId}, total=${conn.parseFailures})`,
        )
        return
      }
      const job: FrameJob = { raw, done: false, ok: false }
      conn.frameQueue.push(job)
      if (conn.frameQueue.length > FRAME_QUEUE_MAX) {
        for (const stalled of conn.frameQueue) {
          if (!stalled.done) this.parseFrameJobSync(stalled)
        }
      }
      if (
        job.done ||
        raw.length < FRAME_WORKER_PARSE_MIN_BYTES ||
        !this.requestFrameParse(sessionId, conn, job)
      ) {
        if (!job.done) this.parseFrameJobSync(job)
      }
      this.drainFrameQueue(sessionId, conn)
    }

    ws.onclose = (event) => {
      if (conn.connectTimer) {
        clearTimeout(conn.connectTimer)
        conn.connectTimer = null
      }
      this.stopTimers(conn)
      conn.state = 'closed'
      if (!conn.intentionalClose && this.connections.get(sessionId) === conn) {
        if (event.code === 1008 || event.code === 4401 || event.code === 4403) {
          console.warn(
            `[wsManager] non-recoverable close code=${event.code} session=${sessionId}; abandoning`,
          )
          this.markAbandoned(sessionId, conn, `close_${event.code}`)
          return
        }
        this.scheduleReconnect(sessionId, conn)
      }
    }

    ws.onerror = (event) => {
      conn.errorCount++
      console.warn(
        `[wsManager] ws onerror (session=${sessionId}, total=${conn.errorCount})`,
        event,
      )
    }
  }

  private createEmptyConnection(): Connection {
    return {
      ws: { readyState: WebSocket.CLOSED, close: () => {}, send: () => {} } as unknown as WebSocket,
      handlers: new Set(),
      reconnectTimer: null,
      connectTimer: null,
      reconnectAttempt: 0,
      pingInterval: null,
      pongWatcher: null,
      intentionalClose: false,
      pendingMessages: [],
      pendingBytes: 0,
      lastPongAt: 0,
      lastActivityAt: Date.now(),
      state: 'idle',
      errorCount: 0,
      parseFailures: 0,
      pathPrefix: '/ws',
      lastHandlerErrorNotifyAt: 0,
      lastServerSeq: 0,
      lastSeqGapNotifyAt: 0,
      frameQueue: [],
    }
  }

  private flushPendingMessages(conn: Connection) {
    while (conn.pendingMessages.length > 0) {
      const entry = conn.pendingMessages.shift()!
      conn.pendingBytes = Math.max(0, conn.pendingBytes - entry.bytes)
      try {
        conn.ws.send(entry.serialized)
      } catch (err) {
        console.warn('[wsManager] flush send failed', err)
        conn.pendingMessages.unshift(entry)
        conn.pendingBytes += entry.bytes
        break
      }
    }
  }

  private enforceConnectionPoolLimit(incomingSessionId: string) {
    if (this.connections.size < MAX_CONCURRENT_CONNECTIONS) return
    const candidates = [...this.connections.entries()]
      .filter(([sid, c]) => sid !== incomingSessionId && c.handlers.size === 0)
      .sort((a, b) => a[1].lastActivityAt - b[1].lastActivityAt)
    const victim = candidates[0]
    if (!victim) return
    const victimId = victim[0]
    console.warn(
      `[wsManager] connection pool full (${this.connections.size}); evicting idle session=${victimId}`,
    )
    this.disconnect(victimId)
  }

  disconnect(sessionId: string) {
    const conn = this.connections.get(sessionId)
    if (!conn) return

    conn.intentionalClose = true
    this.stopTimers(conn)
    if (conn.reconnectTimer) {
      clearTimeout(conn.reconnectTimer)
      conn.reconnectTimer = null
    }
    if (conn.connectTimer) {
      clearTimeout(conn.connectTimer)
      conn.connectTimer = null
    }
    conn.pendingMessages = []
    conn.pendingBytes = 0
    conn.frameQueue = []
    conn.state = 'closed'

    try {
      conn.ws.close()
    } catch (err) {
      console.warn('[wsManager] disconnect close failed', err)
    }
    this.connections.delete(sessionId)
  }

  disconnectAll() {
    for (const sessionId of [...this.connections.keys()]) {
      this.disconnect(sessionId)
    }
  }

  forceReconnectAll() {
    for (const sessionId of [...this.connections.keys()]) {
      const conn = this.connections.get(sessionId)
      if (!conn || conn.state === 'abandoned') continue
      this.connect(sessionId, { force: true })
    }
  }

  send(sessionId: string, message: ClientMessage) {
    const isTrackedUserMessage =
      (message as { type?: string }).type === 'user_message' &&
      typeof (message as { clientMsgId?: unknown }).clientMsgId === 'string'
    if (isTrackedUserMessage) {
      this.registerUserMessage(sessionId, message)
    }

    let conn = this.connections.get(sessionId)
    if (!conn) {
      this.connect(sessionId)
      conn = this.connections.get(sessionId)
      if (!conn) return
    }

    if (conn.state === 'abandoned') {
      console.warn(
        `[wsManager] dropping send to abandoned session=${sessionId} type=${(message as { type?: string }).type}`,
      )
      return
    }

    if (conn.ws.readyState === WebSocket.OPEN) {
      if (conn.pendingMessages.length > 0) {
        this.flushPendingMessages(conn)
      }
      try {
        conn.ws.send(JSON.stringify(message))
      } catch (err) {
        console.warn('[wsManager] send threw, queueing', err)
        if (!isTrackedUserMessage) {
          this.enqueuePending(sessionId, conn, message)
        }
      }
      return
    }

    if ((message as { type?: string }).type === 'ping') {
      if (
        (conn.ws.readyState === WebSocket.CLOSED ||
          conn.ws.readyState === WebSocket.CLOSING) &&
        !conn.intentionalClose &&
        !conn.reconnectTimer
      ) {
        this.scheduleReconnect(sessionId, conn)
      }
      return
    }

    if (!isTrackedUserMessage) {
      this.enqueuePending(sessionId, conn, message)
    }

    if (
      conn.ws.readyState === WebSocket.CLOSED ||
      conn.ws.readyState === WebSocket.CLOSING
    ) {
      if (!conn.intentionalClose && !conn.reconnectTimer) {
        this.scheduleReconnect(sessionId, conn)
      }
    }
  }

  private enqueuePending(sessionId: string, conn: Connection, message: ClientMessage) {
    const serialized = serializeMessage(message)
    if (serialized === null) {
      console.warn(
        `[wsManager] dropping unserializable message type=${(message as { type?: string }).type} session=${sessionId}`,
      )
      return
    }
    const bytes = serialized.length
    const critical = isCriticalMessage(message)
    const entry: PendingEntry = {
      message,
      serialized,
      bytes,
      critical,
      enqueuedAt: Date.now(),
    }

    while (
      (conn.pendingMessages.length >= PENDING_MAX_COUNT ||
        conn.pendingBytes + bytes > PENDING_MAX_BYTES) &&
      conn.pendingMessages.length > 0
    ) {
      const victimIdx = conn.pendingMessages.findIndex((e) => !e.critical)
      if (victimIdx < 0) {
        if (!critical) {
          console.warn(
            `[wsManager] pending queue full of critical messages; dropping new non-critical type=${(message as { type?: string }).type} session=${sessionId}`,
          )
          return
        }
        const oldest = conn.pendingMessages.shift()!
        conn.pendingBytes = Math.max(0, conn.pendingBytes - oldest.bytes)
        console.warn(
          `[wsManager] pending queue overflow; evicting oldest critical type=${(oldest.message as { type?: string }).type} session=${sessionId}`,
        )
      } else {
        const evicted = conn.pendingMessages.splice(victimIdx, 1)[0]
        if (!evicted) break
        conn.pendingBytes = Math.max(0, conn.pendingBytes - evicted.bytes)
        console.warn(
          `[wsManager] pending queue overflow; evicting non-critical type=${(evicted.message as { type?: string }).type} session=${sessionId}`,
        )
      }
    }

    conn.pendingMessages.push(entry)
    conn.pendingBytes += bytes
  }

  onMessage(sessionId: string, handler: MessageHandler): () => void {
    const conn = this.connections.get(sessionId)
    if (!conn) {
      let pre = this.preRegisteredHandlers.get(sessionId)
      if (!pre) {
        pre = new Set()
        this.preRegisteredHandlers.set(sessionId, pre)
      }
      pre.add(handler)
      return () => {
        const set = this.preRegisteredHandlers.get(sessionId)
        if (set) {
          set.delete(handler)
          if (set.size === 0) this.preRegisteredHandlers.delete(sessionId)
        }
        this.connections.get(sessionId)?.handlers.delete(handler)
      }
    }
    conn.handlers.add(handler)
    return () => { conn.handlers.delete(handler) }
  }

  clearHandlers(sessionId: string) {
    const conn = this.connections.get(sessionId)
    if (conn) conn.handlers.clear()
    this.preRegisteredHandlers.delete(sessionId)
  }

  private stopTimers(conn: Connection) {
    if (conn.pingInterval) {
      clearInterval(conn.pingInterval)
      conn.pingInterval = null
    }
    if (conn.pongWatcher) {
      clearInterval(conn.pongWatcher)
      conn.pongWatcher = null
    }
  }

  private startPingLoop(sessionId: string, conn: Connection) {
    if (conn.pingInterval) {
      clearInterval(conn.pingInterval)
      conn.pingInterval = null
    }
    conn.pingInterval = setInterval(() => {
      if (this.connections.get(sessionId) !== conn) {
        this.stopTimers(conn)
        return
      }
      this.send(sessionId, { type: 'ping' })
    }, PING_INTERVAL_MS)
  }

  private startPongWatcher(sessionId: string, conn: Connection) {
    if (conn.pongWatcher) {
      clearInterval(conn.pongWatcher)
      conn.pongWatcher = null
    }
    conn.pongWatcher = setInterval(() => {
      if (this.connections.get(sessionId) !== conn) {
        this.stopTimers(conn)
        return
      }
      if (conn.state !== 'open') return
      if (conn.lastPongAt === 0) return
      const now = Date.now()
      const sincePong = now - conn.lastPongAt
      const sinceActivity = now - conn.lastActivityAt
      if (sincePong > PONG_TIMEOUT_MS && sinceActivity > PONG_TIMEOUT_MS) {
        console.warn(
          `[wsManager] connection silent (pong ${sincePong}ms / activity ${sinceActivity}ms) for session=${sessionId}; forcing reconnect`,
        )
        try {
          conn.ws.close()
        } catch (err) {
          console.warn('[wsManager] force close on pong timeout failed', err)
        }
      }
    }, PONG_WATCHER_TICK_MS)
  }

  private scheduleReconnect(sessionId: string, conn: Connection) {
    if (conn.reconnectTimer) {
      return
    }

    if (conn.reconnectAttempt >= MAX_RECONNECT_ATTEMPTS) {
      this.markAbandoned(sessionId, conn, 'max_attempts')
      return
    }

    const delay = Math.min(1000 * 2 ** conn.reconnectAttempt, MAX_RECONNECT_DELAY_MS)
    conn.reconnectAttempt++

    this.broadcastSystemNotification(
      sessionId,
      'ws_reconnecting',
      'WebSocket disconnected; attempting to reconnect.',
      { attempt: conn.reconnectAttempt },
      'info',
    )

    conn.reconnectTimer = setTimeout(() => {
      conn.reconnectTimer = null
      if (this.connections.get(sessionId) === conn && !conn.intentionalClose) {
        this.connect(sessionId)
      }
    }, delay)
  }

  private markAbandoned(sessionId: string, conn: Connection, reason: string) {
    conn.state = 'abandoned'
    this.stopTimers(conn)
    if (conn.reconnectTimer) {
      clearTimeout(conn.reconnectTimer)
      conn.reconnectTimer = null
    }
    if (conn.connectTimer) {
      clearTimeout(conn.connectTimer)
      conn.connectTimer = null
    }
    console.warn(
      `[wsManager] session=${sessionId} abandoned after ${conn.reconnectAttempt} attempts (reason=${reason})`,
    )
    this.broadcastSystemNotification(
      sessionId,
      'ws_unreachable',
      'WebSocket connection lost and exhausted reconnect attempts.',
      { reason, attempts: conn.reconnectAttempt },
    )
  }
}

export const wsManager = new WebSocketManager()
