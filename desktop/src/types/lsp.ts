// SPDX-License-Identifier: MIT
// Mirror of `crate::config::schema::LspConfig` and friends.
//
// Wire shape:
// - GET /api/lsp -> { enabled, servers: LspServerRecord[] }
// - WebSocket events use the snake_case discriminator that
//   `serde(tag = "type", rename_all = "snake_case")` produces.

export type LspInstallState =
  | { status: 'not_installed' }
  | { status: 'installing' }
  | { status: 'installed'; version: string; path: string }
  | { status: 'failed'; reason: string }

export type LspServerRecord = {
  id: string
  languageId: string
  displayName: string
  enabled: boolean
  managed: boolean
  command: string | null
  args: string[]
  env: Record<string, string>
  fileExtensions: string[]
  initializationOptions: unknown | null
  installState: LspInstallState
}

export type LspListResponse = {
  enabled: boolean
  servers: LspServerRecord[]
}

export type LspUpsertPayload = {
  id: string
  languageId: string
  displayName: string
  enabled: boolean
  managed: boolean
  command: string | null
  args: string[]
  env: Record<string, string>
  fileExtensions: string[]
  initializationOptions: unknown | null
}

export type LspInstallProgressPhase =
  | { phase: 'resolving'; message: string }
  | { phase: 'downloading'; percent: number | null; bytesDownloaded: number; bytesTotal: number | null }
  | { phase: 'extracting'; message: string }
  | { phase: 'verifying'; message: string }
  | { phase: 'done'; version: string; path: string }
  | { phase: 'failed'; reason: string }

export type LspServerLifecycleStatus = 'starting' | 'ready' | 'stopped' | 'failed'

export type LspBroadcastEvent =
  | {
      type: 'lsp_diagnostics'
      serverId: string
      uri: string
      version?: number | null
      diagnostics: LspDiagnostic[]
    }
  | ({
      type: 'lsp_install_progress'
      serverId: string
    } & LspInstallProgressPhase)
  | {
      type: 'lsp_server_status'
      serverId: string
      languageId: string
      status: LspServerLifecycleStatus
      reason?: string | null
    }

export type LspDiagnosticSeverity = 1 | 2 | 3 | 4

export type LspPosition = { line: number; character: number }
export type LspRange = { start: LspPosition; end: LspPosition }

export type LspDiagnostic = {
  range: LspRange
  severity?: LspDiagnosticSeverity
  code?: string | number
  source?: string
  message: string
  tags?: number[]

  [key: string]: unknown
}
