import { useEffect, useMemo, useState } from 'react'
import { useSettingsStore } from '../../stores/settingsStore'
import { useSessionRuntimeStore, DRAFT_RUNTIME_SELECTION_KEY } from '../../stores/sessionRuntimeStore'
import { useProviderStore } from '../../stores/providerStore'
import { useTabStore } from '../../stores/tabStore'
import { useLspStore } from '../../stores/lspStore'
import { useUIStore } from '../../stores/uiStore'
import { useWorkspaceFilesStore } from '../../stores/workspaceFilesStore'
import { usePythonEnvStore } from '../../stores/pythonEnvStore'
import { inferLanguageFromPath } from '../../lib/extLanguage'
import type { CodingModeId } from '../../types/codingMode'
import { useTranslation, useCodingModeText } from '../../i18n'
import { PythonEnvPicker } from '../workspace/PythonEnvPicker'

function detectIndentSpec(text: string): string {
  if (!text) return 'Spaces: 2'
  const lines = text.split('\n').slice(0, 200)
  let tabCount = 0
  let spaceCount = 0
  const spaceWidthVotes: Record<number, number> = {}
  for (const line of lines) {
    if (!line.length) continue
    if (line.startsWith('\t')) {
      tabCount += 1
      continue
    }
    const m = line.match(/^( +)/)
    if (m && m[1]) {
      spaceCount += 1
      const w = m[1].length
      if (w === 2 || w === 4 || w === 8) {
        spaceWidthVotes[w] = (spaceWidthVotes[w] ?? 0) + 1
      }
    }
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
}

const STATUS_AUTONOMOUS_MODES = new Set<CodingModeId>(['agent', 'harness'])
const STATUS_READONLY_MODES = new Set<CodingModeId>(['ask'])

export function StatusBar() {
  const t = useTranslation()
  const tCodingMode = useCodingModeText()
  const activeTabId = useTabStore((s) => s.activeTabId)
  const providers = useProviderStore((s) => s.providers)

  const runtimeSelection = useSessionRuntimeStore((s) =>
    activeTabId
      ? s.selections[activeTabId] ?? s.selections[DRAFT_RUNTIME_SELECTION_KEY]
      : s.selections[DRAFT_RUNTIME_SELECTION_KEY],
  )

  const codingMode = useSettingsStore((s) => s.codingMode)
  const codingModes = useSettingsStore((s) => s.codingModes)
  const settingsModel = useSettingsStore((s) => s.currentModel)
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
  const codingModeIsPlan = codingMode === 'plan'

  const codingModeBadgeClass = codingModeIsAutonomous
    ? 'flex items-center gap-1 rounded-full bg-[var(--color-error)]/12 px-1.5 py-px text-[var(--color-error)]'
    : codingModeIsReadonly
    ? 'flex items-center gap-1 rounded-full bg-[var(--color-surface-container)] px-1.5 py-px text-[var(--color-text-tertiary)]'
    : codingModeIsPlan
    ? 'flex items-center gap-1 rounded-full bg-[var(--color-plan-accent-container)] px-1.5 py-px text-[var(--color-on-plan-accent-container)]'
    : 'flex items-center gap-1'

  const codingModeIconClass = codingModeIsAutonomous
    ? 'material-symbols-outlined text-[11px] text-[var(--color-error)]'
    : codingModeIsReadonly
    ? 'material-symbols-outlined text-[11px] text-[var(--color-text-tertiary)]'
    : codingModeIsPlan
    ? 'material-symbols-outlined text-[11px] text-[var(--color-on-plan-accent-container)]'
    : 'material-symbols-outlined text-[11px] text-[var(--color-text-tertiary)]'

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

  const diagnosticsByUri = useLspStore((s) => s.diagnosticsByUri)
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
  const activeBuffer = useWorkspaceFilesStore((s) => {
    if (!s.root || !s.activeTab) return null
    return s.files[`${s.root}::${s.activeTab}`] ?? null
  })

  const languageId = useMemo(() => {
    if (!activeWorkspaceTab) return null
    return inferLanguageFromPath(activeWorkspaceTab) || 'plaintext'
  }, [activeWorkspaceTab])

  const encodingLabel = useMemo(() => {
    if (!activeBuffer) return null
    if (activeBuffer.encoding === 'base64') return 'binary'
    return 'UTF-8'
  }, [activeBuffer])

  const indentLabel = useMemo(() => {
    if (!activeBuffer || activeBuffer.encoding === 'base64') return null
    return detectIndentSpec(activeBuffer.draft ?? activeBuffer.original ?? '')
  }, [activeBuffer])

  const cursorLabel = useMemo(() => {
    if (!editorCursor || editorCursor.relPath !== activeWorkspaceTab) return null
    const sel = editorCursor.selectedCharCount && editorCursor.selectedCharCount > 0
      ? ` (${editorCursor.selectedCharCount} sel)`
      : ''
    return `Ln ${editorCursor.line}, Col ${editorCursor.column}${sel}`
  }, [activeWorkspaceTab, editorCursor])

  const lspServers = useLspStore((s) => s.servers)
  const lspServerStatus = useLspStore((s) => s.serverStatus)
  const lspEnabled = useLspStore((s) => s.enabled)
  const lspIndicator = useMemo(() => {
    if (!lspEnabled) return { label: 'LSP off', tone: 'off' as const }
    if (!languageId) return { label: 'LSP', tone: 'idle' as const }
    const matching = lspServers.filter(
      (s) => s.enabled && s.languageId === languageId,
    )
    if (matching.length === 0) {
      return { label: `LSP: no server (${languageId})`, tone: 'idle' as const }
    }
    let bestTone: 'ready' | 'starting' | 'failed' | 'idle' = 'idle'
    for (const srv of matching) {
      const st = lspServerStatus[srv.id]?.status ?? srv.lifecycleStatus
      if (st === 'ready') {
        bestTone = 'ready'
        break
      }
      if (st === 'starting' && bestTone !== 'failed') bestTone = 'starting'
      if (st === 'failed') bestTone = 'failed'
    }
    const labelMap: Record<typeof bestTone, string> = {
      ready: `LSP: ready (${languageId})`,
      starting: `LSP: starting (${languageId})`,
      failed: `LSP: failed (${languageId})`,
      idle: `LSP: idle (${languageId})`,
    }
    return { label: labelMap[bestTone], tone: bestTone }
  }, [languageId, lspEnabled, lspServerStatus, lspServers])

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
        <span className={codingModeBadgeClass} title={codingModeTitle}>
          <span className={codingModeIconClass}>{codingModeGlyph}</span>
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
            title="Cursor position"
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
