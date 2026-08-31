// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { useEffect, useMemo, useState } from 'react'
import { useShallow } from 'zustand/react/shallow'
import { useSettingsStore } from '../../stores/settingsStore'
import { DRAFT_RUNTIME_SELECTION_KEY, useSessionRuntimeStore } from '../../stores/sessionRuntimeStore'
import { useProviderStore } from '../../stores/providerStore'
import { useTabStore } from '../../stores/tabStore'
import { useChatStore } from '../../stores/chatStore'
import { resolveEffectiveRuntimeSelection } from '../../utils/runtimeSelection'
import { resolveLspServerDisplayStatus, useLspStore } from '../../stores/lspStore'
import { useActiveWorkspaceRoot } from '../../lib/activeWorkDir'
import { useUIStore } from '../../stores/uiStore'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import { usePythonEnvStore } from '../../stores/pythonEnvStore'
import { inferLanguageFromPath } from '../../lib/extLanguage'
import type { CodingModeId } from '../../types/codingMode'
import { CODING_MODE_ACCENT } from '../../types/codingMode'
import { useTranslation, useCodingModeText } from '../../i18n'
import { PythonEnvPicker } from '../workspace/PythonEnvPicker'

const INDENT_SCAN_MAX_CHARS = 16_384
const INDENT_SCAN_MAX_LINES = 200

function detectIndentSpec(text: string): string {
  if (!text) return 'Spaces: 2'
  let tabCount = 0
  let spaceCount = 0
  const spaceWidthVotes: Record<number, number> = {}
  const limit = Math.min(text.length, INDENT_SCAN_MAX_CHARS)
  let lineStart = 0
  let lineCount = 0
  while (lineStart < limit && lineCount < INDENT_SCAN_MAX_LINES) {
    let lineEnd = text.indexOf('\n', lineStart)
    if (lineEnd === -1 || lineEnd > limit) lineEnd = limit
    if (lineEnd > lineStart) {
      const first = text.charCodeAt(lineStart)
      if (first === 9) {
        tabCount += 1
      } else if (first === 32) {
        let cursor = lineStart
        while (cursor < lineEnd && text.charCodeAt(cursor) === 32) cursor += 1
        spaceCount += 1
        const w = cursor - lineStart
        if (w === 2 || w === 4 || w === 8) {
          spaceWidthVotes[w] = (spaceWidthVotes[w] ?? 0) + 1
        }
      }
    }
    lineCount += 1
    lineStart = lineEnd + 1
  }
  if (tabCount > spaceCount && tabCount > 0) return 'Tabs'
  if (spaceCount === 0) return 'Spaces: 2'
  let bestWidth = 2
  let bestVotes = -1
  for (const k of Object.keys(spaceWidthVotes)) {
    const w = Number(k)
    const v = spaceWidthVotes[w] ?? 0
    if (v > bestVotes) {
      bestVotes = v
      bestWidth = w
    }
  }
  return `Spaces: ${bestWidth}`
}

const STATUS_MODE_GLYPH: Record<CodingModeId, string> = {
  auto: 'auto_awesome',
  vibe: 'bolt',
  agent: 'robot_2',
  spec: 'description',
  plan: 'architecture',
  ask: 'help',
  tdd: 'science',
  debug: 'bug_report',
  architect: 'design_services',
  pair: 'group',
  context: 'data_object',
  mvai: 'hub',
  harness: 'precision_manufacturing',
  curator: 'auto_stories',
  designer: 'palette',
}

const STATUS_AUTONOMOUS_MODES = new Set<CodingModeId>(['agent', 'harness'])
const STATUS_READONLY_MODES = new Set<CodingModeId>(['ask'])

const EMPTY_DIAGNOSTICS_BY_URI: Record<
  string,
  { serverId: string; version: number | null; diagnostics: never[] }
> = Object.freeze({}) as Record<
  string,
  { serverId: string; version: number | null; diagnostics: never[] }
>

export function StatusBar() {
  const t = useTranslation()
  const tCodingMode = useCodingModeText()
  const activeTabId = useTabStore((s) => s.activeTabId)
  const providers = useProviderStore((s) => s.providers)
  const sessionRuntimeSelection = useSessionRuntimeStore((s) =>
    activeTabId ? s.selections[activeTabId] : undefined,
  )
  const draftRuntimeSelection = useSessionRuntimeStore((s) => s.selections[DRAFT_RUNTIME_SELECTION_KEY])
  const settingsModel = useSettingsStore((s) => s.currentModel)
  const activeProviderId = useProviderStore((s) => s.activeId)

  const runtimeSelection = useMemo(
    () => resolveEffectiveRuntimeSelection(
      activeTabId,
      providers,
      activeProviderId,
      settingsModel?.id,
    ),
    [activeTabId, providers, activeProviderId, settingsModel, sessionRuntimeSelection, draftRuntimeSelection],
  )

  const globalCodingMode = useSettingsStore((s) => s.codingMode)
  const sessionCodingMode = useChatStore((s) =>
    activeTabId ? s.sessionCodingMode[activeTabId] : undefined,
  )
  const codingMode = sessionCodingMode ?? globalCodingMode
  const connectionState = useChatStore((s) =>
    activeTabId ? s.sessions[activeTabId]?.connectionState : undefined,
  )
  const connectToSession = useChatStore((s) => s.connectToSession)
  const codingModes = useSettingsStore((s) => s.codingModes)
  const settingsProviderName = useSettingsStore((s) => s.activeProviderName)

  const codingModeBackendLabel = codingModes.find((m) => m.id === codingMode)?.label
  const codingModeLabel = tCodingMode(
    codingMode,
    'label',
    codingModeBackendLabel ?? codingMode.charAt(0).toUpperCase() + codingMode.slice(1),
  )

  const codingModeGlyph = STATUS_MODE_GLYPH[codingMode] ?? 'tune'
  const codingModeIsAutonomous = STATUS_AUTONOMOUS_MODES.has(codingMode)
  const codingModeIsReadonly = STATUS_READONLY_MODES.has(codingMode)
  const codingModeAccent = CODING_MODE_ACCENT[codingMode]

  const codingModeBadgeClass = codingModeAccent
    ? 'flex items-center gap-1 rounded-full px-1.5 py-px'
    : 'flex items-center gap-1'
  const codingModeBadgeStyle = codingModeAccent
    ? { backgroundColor: codingModeAccent.container, color: codingModeAccent.onContainer }
    : undefined

  const codingModeIconClass = 'material-symbols-outlined text-[11px]'
  const codingModeIconStyle = codingModeAccent
    ? { color: codingModeAccent.accent }
    : { color: 'var(--color-text-tertiary)' }

  const codingModeTitle = codingModeIsAutonomous
    ? `${codingModeLabel} · ${t('codingMode.tag.autonomous')}`
    : codingModeIsReadonly
    ? `${codingModeLabel} · ${t('codingMode.tag.readOnly')}`
    : codingModeLabel

  const { providerLabel, modelLabel } = useMemo(() => {
    if (runtimeSelection) {
      const provider = providers.find((p) => p.id === runtimeSelection.providerId)
      return {
        providerLabel: provider?.name ?? null,
        modelLabel: runtimeSelection.modelId,
      }
    }
    if (settingsModel) {
      return {
        providerLabel: settingsProviderName ?? null,
        modelLabel: settingsModel.name ?? settingsModel.id,
      }
    }
    return { providerLabel: null as string | null, modelLabel: null as string | null }
  }, [providers, runtimeSelection, settingsModel, settingsProviderName])

  const activeWorkspaceRoot = useActiveWorkspaceRoot()
  const diagnosticsByUri = useLspStore((s) => {
    if (activeWorkspaceRoot && activeWorkspaceRoot.length > 0) {
      return s.diagnosticsByWorkspace[activeWorkspaceRoot] ?? EMPTY_DIAGNOSTICS_BY_URI
    }
    return s.diagnosticsByUri
  })
  const { errorCount, warningCount } = useMemo(() => {
    let errors = 0
    let warnings = 0
    for (const entry of Object.values(diagnosticsByUri)) {
      const list = entry?.diagnostics ?? []
      for (const diag of list) {
        const sev = diag.severity ?? 1
        if (sev === 1) errors += 1
        else if (sev === 2) warnings += 1
      }
    }
    return { errorCount: errors, warningCount: warnings }
  }, [diagnosticsByUri])

  const editorCursor = useUIStore((s) => s.editorCursor)
  const activeWorkspaceTab = useWorkspaceFilesStore((s) => s.activeTab)
  const activeBufferMeta = useWorkspaceFilesStore(
    useShallow((s) => {
      if (!s.root || !s.activeTab) return null
      const buf = s.files[`${s.root}::${s.activeTab}`]
      if (!buf) return null
      return {
        encoding: buf.encoding,
        modifiedAt: buf.modifiedAt,
      }
    }),
  )

  const languageId = useMemo(() => {
    if (!activeWorkspaceTab) return null
    return inferLanguageFromPath(activeWorkspaceTab) || 'plaintext'
  }, [activeWorkspaceTab])

  const encodingLabel = useMemo(() => {
    if (!activeBufferMeta) return null
    if (activeBufferMeta.encoding === 'base64') return 'binary'
    return 'UTF-8'
  }, [activeBufferMeta])

  const indentLabel = useMemo(() => {
    if (!activeBufferMeta || activeBufferMeta.encoding === 'base64') return null
    const s = useWorkspaceFilesStore.getState()
    if (!s.root || !s.activeTab) return null
    const buf = s.files[`${s.root}::${s.activeTab}`]
    if (!buf) return null
    return detectIndentSpec(buf.draft ?? buf.original ?? '')
  }, [activeBufferMeta?.encoding, activeBufferMeta?.modifiedAt, activeWorkspaceTab])

  const cursorLabel = useMemo(() => {
    if (!editorCursor || editorCursor.relPath !== activeWorkspaceTab) return null
    const sel = editorCursor.selectedCharCount && editorCursor.selectedCharCount > 0
      ? ` (${editorCursor.selectedCharCount} sel)`
      : ''
    return `Ln ${editorCursor.line}, Col ${editorCursor.column}${sel}`
  }, [activeWorkspaceTab, editorCursor])

  const lspServers = useLspStore((s) => s.servers)
  const lspServerStatus = useLspStore((s) => s.serverStatus)
  const lspInstallProgress = useLspStore((s) => s.installProgress)
  const lspEnabled = useLspStore((s) => s.enabled)
  const lspIndicator = useMemo(() => {
    if (!lspEnabled) return { label: t('files.statusBar.lspOff'), tone: 'off' as const }
    if (!languageId) return { label: t('files.statusBar.lspIdle'), tone: 'idle' as const }
    const matching = lspServers.filter(
      (s) => s.enabled && s.languageId === languageId,
    )
    if (matching.length === 0) {
      return { label: t('files.statusBar.lspIdle'), tone: 'idle' as const }
    }
    let bestTone: 'ready' | 'starting' | 'failed' | 'idle' = 'idle'
    for (const srv of matching) {
      const display = resolveLspServerDisplayStatus(
        lspEnabled,
        srv,
        lspServerStatus[srv.id]?.status ?? srv.lifecycleStatus ?? null,
        lspInstallProgress[srv.id] ?? null,
      )
      if (display === 'ready') {
        bestTone = 'ready'
        break
      }
      if (display === 'starting' && bestTone !== 'failed') bestTone = 'starting'
      if (display === 'failed') bestTone = 'failed'
    }
    const labelMap: Record<typeof bestTone, string> = {
      ready: t('files.statusBar.lspReady'),
      starting: t('files.statusBar.lspStarting'),
      failed: t('files.statusBar.lspFailed'),
      idle: t('files.statusBar.lspIdle'),
    }
    return { label: labelMap[bestTone], tone: bestTone }
  }, [languageId, lspEnabled, lspInstallProgress, lspServerStatus, lspServers, t])

  const lspDotClass = useMemo(() => {
    switch (lspIndicator.tone) {
      case 'ready':
        return 'bg-[var(--color-success)]'
      case 'starting':
        return 'bg-[var(--color-warning)] animate-pulse'
      case 'failed':
        return 'bg-[var(--color-error)]'
      case 'off':
        return 'bg-[var(--color-text-tertiary)]/40'
      default:
        return 'bg-[var(--color-text-tertiary)]'
    }
  }, [lspIndicator.tone])


  const workspaceRoot = useWorkspaceFilesStore((s) => s.root)
  const pythonStatus = usePythonEnvStore((s) =>
    workspaceRoot ? s.statusByRoot[workspaceRoot] : undefined,
  )
  const pythonJob = usePythonEnvStore((s) =>
    workspaceRoot ? s.jobsByRoot[workspaceRoot] : undefined,
  )
  const subscribePython = usePythonEnvStore((s) => s.subscribe)
  useEffect(() => {
    if (!workspaceRoot) return
    subscribePython(workspaceRoot)
  }, [workspaceRoot, subscribePython])

  const [pythonPickerOpen, setPythonPickerOpen] = useState(false)

  const pythonSegment = useMemo(() => {
    if (!workspaceRoot) return null
    const isPython = languageId === 'python'
    const hasStatus = !!pythonStatus && (
      pythonStatus.isPythonProject ||
      pythonStatus.interpreterPath !== null ||
      pythonJob !== undefined
    )
    if (!isPython && !hasStatus) return null
    let label = t('python.statusBar.notSet')
    let dot = 'bg-[var(--color-text-tertiary)]'
    let tooltip = t('python.statusBar.tooltip')
    if (pythonJob?.kind === 'creating') {
      label = t('python.statusBar.creating')
      dot = 'bg-[var(--color-warning)] animate-pulse'
    } else if (pythonJob?.kind === 'installing') {
      label = t('python.statusBar.installing')
      dot = 'bg-[var(--color-warning)] animate-pulse'
    } else if (pythonStatus?.interpreterPath) {
      const version = pythonStatus.version ?? ''
      const pkg = pythonStatus.packagesCount
      const pkgSuffix = pkg != null ? ` · ${pkg}` : ''
      if (pythonStatus.isIsolated) {
        label = version
          ? t('python.statusBar.venv', { version, pkg: pkgSuffix })
          : t('python.statusBar.venvNoVersion', { pkg: pkgSuffix })
        dot = 'bg-emerald-500'
      } else {
        label = version
          ? t('python.statusBar.system', { version, pkg: pkgSuffix })
          : t('python.statusBar.systemNoVersion', { pkg: pkgSuffix })
        dot = 'bg-amber-500'
      }
      const tooltipParts = [
        pythonStatus.interpreterPath,
        pythonStatus.tool && pythonStatus.tool !== 'unknown'
          ? `tool: ${pythonStatus.tool}`
          : null,
        pythonStatus.requiredPython?.version
          ? `requires: ${pythonStatus.requiredPython.version}`
          : null,
      ].filter(Boolean)
      if (tooltipParts.length > 0) tooltip = tooltipParts.join(' · ')
    } else if (pythonStatus?.lastError) {
      dot = 'bg-[var(--color-error)]'
      tooltip = pythonStatus.lastError
    }
    return { label, dot, tooltip }
  }, [languageId, pythonJob, pythonStatus, t, workspaceRoot])

  const [version, setVersion] = useState<string | null>(null)
  useEffect(() => {
    let cancelled = false
    import('@tauri-apps/api/app')
      .then((mod) => mod.getVersion())
      .then((value) => { if (!cancelled) setVersion(value) })
      .catch(() => { if (!cancelled) setVersion(null) })
    return () => { cancelled = true }
  }, [])

  return (
    <div
      role="status"
      aria-label="Status bar"
      className="flex h-[22px] flex-shrink-0 items-center justify-between border-t border-[var(--color-border)] bg-[var(--color-surface-container-low)] px-3 text-[11px] text-[var(--color-text-secondary)]"
    >
      {}
      <div className="flex items-center gap-3">
        {activeTabId &&
          (connectionState === 'reconnecting' || connectionState === 'disconnected') && (
            <button
              type="button"
              onClick={() => connectToSession(activeTabId, { force: true })}
              title={t('chat.reconnect')}
              className="flex items-center gap-1 rounded-full px-1.5 py-px text-[var(--color-warning)] hover:bg-[var(--color-surface-hover)]"
            >
              <span
                className={`w-1.5 h-1.5 rounded-full ${
                  connectionState === 'reconnecting'
                    ? 'bg-[var(--color-warning)] animate-pulse'
                    : 'bg-[var(--color-error)]'
                }`}
              />
              <span className="text-[10px] uppercase tracking-wider font-bold">
                {connectionState === 'reconnecting'
                  ? t('statusBar.reconnecting')
                  : t('statusBar.disconnected')}
              </span>
            </button>
          )}
        <span className={codingModeBadgeClass} style={codingModeBadgeStyle} title={codingModeTitle}>
          <span className={codingModeIconClass} style={codingModeIconStyle}>{codingModeGlyph}</span>
          <span>{codingModeLabel}</span>
          {codingModeIsAutonomous && (
            <span className="text-[9px] uppercase tracking-wider font-bold">
              {t('codingMode.tag.autonomous')}
            </span>
          )}
          {codingModeIsReadonly && (
            <span className="text-[9px] uppercase tracking-wider font-bold">
              {t('codingMode.tag.readOnly')}
            </span>
          )}
        </span>

        {modelLabel && (
          <span className="flex items-center gap-1">
            <span className="text-[var(--color-text-tertiary)]">·</span>
            <span
              className="truncate"
              title={providerLabel ? `${providerLabel} · ${modelLabel}` : modelLabel}
            >
              {modelLabel}
              {providerLabel && (
                <span className="text-[var(--color-text-tertiary)]"> · {providerLabel}</span>
              )}
            </span>
          </span>
        )}
      </div>

      <div className="flex items-center gap-3">
        {cursorLabel && (
          <span
            className="tabular-nums text-[var(--color-text-tertiary)]"
            title="position"
          >
            {cursorLabel}
          </span>
        )}
        {indentLabel && (
          <span className="text-[var(--color-text-tertiary)]" title="Indentation">
            {indentLabel}
          </span>
        )}
        {encodingLabel && (
          <span className="text-[var(--color-text-tertiary)]" title="Encoding">
            {encodingLabel}
          </span>
        )}
        {pythonSegment && (
          <button
            type="button"
            onClick={() => setPythonPickerOpen(true)}
            className="flex items-center gap-1 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] transition-colors"
            title={pythonSegment.tooltip}
          >
            <span
              aria-hidden="true"
              className={`h-1.5 w-1.5 rounded-full ${pythonSegment.dot}`}
            />
            <span>{pythonSegment.label}</span>
          </button>
        )}
        {languageId && (
          <span className="text-[var(--color-text-tertiary)]" title="Language mode">
            {languageId}
          </span>
        )}
        <span
          className="flex items-center gap-1 text-[var(--color-text-tertiary)]"
          title={lspIndicator.label}
        >
          <span
            aria-hidden="true"
            className={`h-1.5 w-1.5 rounded-full ${lspDotClass}`}
          />
          <span>{lspIndicator.label}</span>
        </span>
        <span
          className="flex items-center gap-1.5 text-[11px] leading-none"
          title={t('files.statusBar.problemsTooltip')}
        >
          <span className="flex items-center gap-0.5">
            <span
              aria-hidden="true"
              className="material-symbols-outlined leading-none text-[var(--color-error)]"
              style={{
                fontSize: '11px',
                width: '11px',
                height: '11px',
                fontVariationSettings: "'FILL' 0, 'wght' 500, 'GRAD' 0, 'opsz' 20",
              }}
            >
              error
            </span>
            <span className="tabular-nums">{errorCount}</span>
          </span>
          <span className="flex items-center gap-0.5">
            <span
              aria-hidden="true"
              className="material-symbols-outlined leading-none text-[var(--color-warning)]"
              style={{
                fontSize: '11px',
                width: '11px',
                height: '11px',
                fontVariationSettings: "'FILL' 0, 'wght' 500, 'GRAD' 0, 'opsz' 20",
              }}
            >
              warning
            </span>
            <span className="tabular-nums">{warningCount}</span>
          </span>
        </span>
        <span className="text-[var(--color-text-tertiary)]">
          {version ? `v${version}` : ''}
        </span>
      </div>
      <PythonEnvPicker open={pythonPickerOpen} onClose={() => setPythonPickerOpen(false)} />
    </div>
  )
}
