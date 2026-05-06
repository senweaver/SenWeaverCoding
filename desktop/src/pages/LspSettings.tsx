import { useEffect, useMemo, useState } from 'react'
import { Button } from '../components/shared/Button'
import { Input } from '../components/shared/Input'
import { useTranslation } from '../i18n'
import { listLspTemplates, lspTemplate, useLspStore } from '../stores/lspStore'
import type { LspServerRecord, LspUpsertPayload } from '../types/lsp'

type Mode = 'managed' | 'manual'

type Draft = {
  id: string
  languageId: string
  displayName: string
  enabled: boolean
  managed: boolean
  command: string
  args: string
  envText: string
  fileExtensions: string
  initOptions: string
}

function emptyDraft(): Draft {
  return {
    id: '',
    languageId: '',
    displayName: '',
    enabled: false,
    managed: true,
    command: '',
    args: '',
    envText: '',
    fileExtensions: '',
    initOptions: '',
  }
}

function draftFromRecord(s: LspServerRecord): Draft {
  return {
    id: s.id,
    languageId: s.languageId,
    displayName: s.displayName,
    enabled: s.enabled,
    managed: s.managed,
    command: s.command ?? '',
    args: s.args.join(' '),
    envText: Object.entries(s.env).map(([k, v]) => `${k}=${v}`).join('\n'),
    fileExtensions: s.fileExtensions.join(', '),
    initOptions: s.initializationOptions ? JSON.stringify(s.initializationOptions, null, 2) : '',
  }
}

function parseArgs(text: string): string[] {
  return text
    .split(/\s+/)
    .map((s) => s.trim())
    .filter(Boolean)
}

function parseEnv(text: string): Record<string, string> {
  const out: Record<string, string> = {}
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim()
    if (!line) continue
    const eq = line.indexOf('=')
    if (eq <= 0) continue
    const key = line.slice(0, eq).trim()
    const value = line.slice(eq + 1).trim()
    if (key) out[key] = value
  }
  return out
}

function parseExtensions(text: string): string[] {
  return text
    .split(/[\s,]+/)
    .map((s) => s.trim().replace(/^\./, ''))
    .filter(Boolean)
}

function parseInitOptions(text: string): unknown {
  const trimmed = text.trim()
  if (!trimmed) return null
  try {
    return JSON.parse(trimmed)
  } catch {
    return null
  }
}

function buildPayload(draft: Draft): LspUpsertPayload {
  return {
    id: draft.id.trim(),
    languageId: draft.languageId.trim(),
    displayName: draft.displayName.trim() || draft.id.trim(),
    enabled: draft.enabled,
    managed: draft.managed,
    command: draft.command.trim() ? draft.command.trim() : null,
    args: parseArgs(draft.args),
    env: parseEnv(draft.envText),
    fileExtensions: parseExtensions(draft.fileExtensions),
    initializationOptions: parseInitOptions(draft.initOptions),
  }
}

const STATUS_TONE: Record<string, string> = {
  ready: 'bg-emerald-500/10 text-emerald-600 border-emerald-500/20',
  starting: 'bg-blue-500/10 text-blue-600 border-blue-500/20',
  failed: 'bg-rose-500/10 text-rose-600 border-rose-500/20',
  stopped: 'bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)] border-[var(--color-border)]',
  installed: 'bg-emerald-500/10 text-emerald-600 border-emerald-500/20',
  installing: 'bg-amber-500/10 text-amber-600 border-amber-500/20',
  not_installed: 'bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)] border-[var(--color-border)]',
}

export function LspSettings() {
  const t = useTranslation()
  const enabled = useLspStore((s) => s.enabled)
  const servers = useLspStore((s) => s.servers)
  const selectedId = useLspStore((s) => s.selectedId)
  const isLoading = useLspStore((s) => s.isLoading)
  const error = useLspStore((s) => s.error)
  const diagnosticsByUri = useLspStore((s) => s.diagnosticsByUri)
  const installProgress = useLspStore((s) => s.installProgress)
  const serverStatus = useLspStore((s) => s.serverStatus)
  const fetch = useLspStore((s) => s.fetch)
  const setGlobalEnabled = useLspStore((s) => s.setGlobalEnabled)
  const createServer = useLspStore((s) => s.createServer)
  const updateServer = useLspStore((s) => s.updateServer)
  const deleteServer = useLspStore((s) => s.deleteServer)
  const toggleServer = useLspStore((s) => s.toggleServer)
  const installServer = useLspStore((s) => s.installServer)
  const restartServer = useLspStore((s) => s.restartServer)
  const selectServer = useLspStore((s) => s.selectServer)

  const [draft, setDraft] = useState<Draft>(emptyDraft())
  const [isCreating, setIsCreating] = useState(false)
  const [showAddMenu, setShowAddMenu] = useState(false)
  const [isSaving, setIsSaving] = useState(false)

  useEffect(() => {
    void fetch()
  }, [fetch])

  const selected = useMemo(
    () => servers.find((s) => s.id === selectedId) ?? null,
    [servers, selectedId],
  )

  useEffect(() => {
    if (selected) {
      setDraft(draftFromRecord(selected))
      setIsCreating(false)
    }
  }, [selected])

  const startAddTemplate = (templateId: string) => {
    setShowAddMenu(false)
    selectServer(null)
    setIsCreating(true)
    if (templateId === 'custom') {
      setDraft({ ...emptyDraft(), managed: false })
    } else {
      const tpl = lspTemplate(templateId)
      if (tpl) {
        setDraft({
          id: tpl.id,
          languageId: tpl.languageId,
          displayName: tpl.displayName,
          enabled: tpl.enabled,
          managed: tpl.managed,
          command: tpl.command ?? '',
          args: tpl.args.join(' '),
          envText: '',
          fileExtensions: tpl.fileExtensions.join(', '),
          initOptions: tpl.initializationOptions
            ? JSON.stringify(tpl.initializationOptions, null, 2)
            : '',
        })
      }
    }
  }

  const handleSave = async () => {
    setIsSaving(true)
    try {
      const payload = buildPayload(draft)
      if (!payload.id || !payload.languageId) return
      if (isCreating) {
        const created = await createServer(payload)
        selectServer(created.id)
      } else if (selected) {
        await updateServer(selected.id, payload)
      }
    } finally {
      setIsSaving(false)
    }
  }

  const liveStatus = (serverId: string) => serverStatus[serverId]?.status
  const installPhase = (serverId: string) => installProgress[serverId]

  const recentDiagnostics = useMemo(() => {
    if (!selected) return []
    const out: Array<{ uri: string; message: string; severity: number | undefined }> = []
    for (const [uri, entry] of Object.entries(diagnosticsByUri)) {
      if (entry.serverId === selected.id || entry.serverId === selected.languageId) {
        for (const diag of entry.diagnostics) {
          out.push({ uri, message: diag.message, severity: diag.severity })
          if (out.length >= 8) break
        }
      }
      if (out.length >= 8) break
    }
    return out
  }, [diagnosticsByUri, selected])

  return (
    <div className="flex flex-col gap-4 max-w-5xl">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-base font-semibold text-[var(--color-text-primary)]">
            {t('settings.lsp.title')}
          </h2>
          <p className="text-xs text-[var(--color-text-tertiary)] mt-0.5">
            {t('settings.lsp.description')}
          </p>
        </div>
        <label className="flex items-center gap-2 text-xs text-[var(--color-text-secondary)]">
          <input
            type="checkbox"
            checked={enabled}
            onChange={(e) => void setGlobalEnabled(e.target.checked)}
            className="h-4 w-4 rounded border-[var(--color-border)] text-[var(--color-brand)]"
          />
          {t('settings.lsp.globalEnable')}
        </label>
      </div>

      {error && (
        <div className="text-xs text-[var(--color-error)] px-3 py-2 rounded-lg border border-[var(--color-error)]/30 bg-[var(--color-error)]/10">
          {error}
        </div>
      )}

      <div className="flex gap-3 min-h-[420px]">
        {}
        <div className="w-[260px] flex-shrink-0 flex flex-col gap-2 border border-[var(--color-border)] rounded-xl p-2 bg-[var(--color-surface-container-low)]">
          <div className="flex flex-col gap-1 flex-1 overflow-y-auto">
            {isLoading && servers.length === 0 ? (
              <div className="text-xs text-[var(--color-text-tertiary)] py-4 text-center">
                {t('common.loading')}
              </div>
            ) : (
              servers.map((s) => {
                const status = liveStatus(s.id)
                const installLabel = s.installState.status
                const isSelected = selected?.id === s.id
                return (
                  <button
                    key={s.id}
                    onClick={() => {
                      selectServer(s.id)
                      setIsCreating(false)
                    }}
                    className={`flex flex-col items-start gap-1 px-3 py-2 rounded-lg text-left transition-colors ${
                      isSelected
                        ? 'bg-[var(--color-surface-selected)] text-[var(--color-text-primary)]'
                        : 'hover:bg-[var(--color-surface-hover)] text-[var(--color-text-secondary)]'
                    }`}
                  >
                    <div className="flex items-center gap-2 w-full">
                      <span className="material-symbols-outlined text-[16px]">
                        {s.managed ? 'cloud_download' : 'tune'}
                      </span>
                      <span className="text-xs font-medium truncate flex-1">
                        {s.displayName || s.id}
                      </span>
                      <span
                        className={`px-1.5 py-0.5 text-[10px] font-medium rounded border ${
                          STATUS_TONE[status ?? installLabel] ?? STATUS_TONE.not_installed
                        }`}
                      >
                        {status ?? installLabel}
                      </span>
                    </div>
                    <div className="text-[10px] text-[var(--color-text-tertiary)] font-mono truncate w-full">
                      {s.languageId} · {s.fileExtensions.join(', ') || '—'}
                    </div>
                  </button>
                )
              })
            )}
          </div>

          <div className="relative">
            <Button
              variant="secondary"
              size="sm"
              className="w-full"
              onClick={() => setShowAddMenu((v) => !v)}
            >
              <span className="material-symbols-outlined text-[14px]">add</span>
              {t('settings.lsp.addServer')}
            </Button>
            {showAddMenu && (
              <div className="absolute bottom-full mb-2 left-0 right-0 z-10 rounded-lg border border-[var(--color-border)] bg-[var(--color-surface)] shadow-lg overflow-hidden">
                {listLspTemplates().map((tpl) => (
                  <button
                    key={tpl.id}
                    className="w-full flex flex-col items-start gap-0.5 px-3 py-2 text-left hover:bg-[var(--color-surface-hover)]"
                    onClick={() => startAddTemplate(tpl.id)}
                  >
                    <span className="text-xs font-medium text-[var(--color-text-primary)]">
                      {tpl.displayName}
                    </span>
                    <span className="text-[10px] text-[var(--color-text-tertiary)]">
                      {tpl.languageId}
                    </span>
                  </button>
                ))}
                <button
                  className="w-full flex flex-col items-start gap-0.5 px-3 py-2 text-left hover:bg-[var(--color-surface-hover)] border-t border-[var(--color-border)]"
                  onClick={() => startAddTemplate('custom')}
                >
                  <span className="text-xs font-medium text-[var(--color-text-primary)]">
                    {t('settings.lsp.customTemplate')}
                  </span>
                  <span className="text-[10px] text-[var(--color-text-tertiary)]">
                    {t('settings.lsp.customTemplateHint')}
                  </span>
                </button>
              </div>
            )}
          </div>
        </div>

        {}
        <div className="flex-1 border border-[var(--color-border)] rounded-xl p-4 bg-[var(--color-surface-container-low)] flex flex-col gap-3 min-w-0">
          {selected || isCreating ? (
            <>
              <ModeToggle
                mode={draft.managed ? 'managed' : 'manual'}
                onChange={(mode: Mode) => setDraft((d) => ({ ...d, managed: mode === 'managed' }))}
              />

              <div className="grid grid-cols-2 gap-3">
                <Input
                  label={t('settings.lsp.field.id')}
                  value={draft.id}
                  onChange={(e) => setDraft((d) => ({ ...d, id: e.target.value }))}
                  placeholder="rust-analyzer"
                  disabled={!isCreating}
                />
                <Input
                  label={t('settings.lsp.field.languageId')}
                  value={draft.languageId}
                  onChange={(e) => setDraft((d) => ({ ...d, languageId: e.target.value }))}
                  placeholder="rust"
                />
                <Input
                  label={t('settings.lsp.field.displayName')}
                  value={draft.displayName}
                  onChange={(e) => setDraft((d) => ({ ...d, displayName: e.target.value }))}
                  placeholder="rust-analyzer"
                />
                <Input
                  label={t('settings.lsp.field.fileExtensions')}
                  value={draft.fileExtensions}
                  onChange={(e) => setDraft((d) => ({ ...d, fileExtensions: e.target.value }))}
                  placeholder="rs, rs.in"
                />
              </div>

              {!draft.managed && (
                <>
                  <Input
                    label={t('settings.lsp.field.command')}
                    value={draft.command}
                    onChange={(e) => setDraft((d) => ({ ...d, command: e.target.value }))}
                    placeholder="/usr/local/bin/rust-analyzer"
                  />
                  <Input
                    label={t('settings.lsp.field.args')}
                    value={draft.args}
                    onChange={(e) => setDraft((d) => ({ ...d, args: e.target.value }))}
                    placeholder="--stdio"
                  />
                  <div className="flex flex-col gap-1">
                    <label className="text-xs font-medium text-[var(--color-text-primary)]">
                      {t('settings.lsp.field.env')}
                    </label>
                    <textarea
                      className="text-xs font-mono px-3 py-2 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] min-h-[60px]"
                      value={draft.envText}
                      onChange={(e) => setDraft((d) => ({ ...d, envText: e.target.value }))}
                      placeholder="RA_LOG=info"
                    />
                  </div>
                </>
              )}

              <div className="flex flex-col gap-1">
                <label className="text-xs font-medium text-[var(--color-text-primary)]">
                  {t('settings.lsp.field.initOptions')}
                </label>
                <textarea
                  className="text-xs font-mono px-3 py-2 rounded-[var(--radius-md)] bg-[var(--color-surface-container-low)] border border-[var(--color-border)] text-[var(--color-text-primary)] outline-none focus:border-[var(--color-border-focus)] min-h-[80px]"
                  value={draft.initOptions}
                  onChange={(e) => setDraft((d) => ({ ...d, initOptions: e.target.value }))}
                  placeholder='{ "checkOnSave": true }'
                />
              </div>

              <label className="flex items-center gap-2 text-xs text-[var(--color-text-secondary)]">
                <input
                  type="checkbox"
                  checked={draft.enabled}
                  onChange={(e) => setDraft((d) => ({ ...d, enabled: e.target.checked }))}
                  className="h-4 w-4 rounded border-[var(--color-border)] text-[var(--color-brand)]"
                />
                {t('settings.lsp.field.enabled')}
              </label>

              {selected && draft.managed && (
                <ManagedActions
                  server={selected}
                  progress={installPhase(selected.id)}
                  onInstall={() => void installServer(selected.id)}
                />
              )}

              {selected && (
                <div className="flex items-center gap-2 flex-wrap">
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => void toggleServer(selected.id)}
                  >
                    <span className="material-symbols-outlined text-[14px]">power_settings_new</span>
                    {selected.enabled
                      ? t('settings.lsp.action.disable')
                      : t('settings.lsp.action.enable')}
                  </Button>
                  <Button
                    variant="secondary"
                    size="sm"
                    onClick={() => void restartServer(selected.id)}
                  >
                    <span className="material-symbols-outlined text-[14px]">restart_alt</span>
                    {t('settings.lsp.action.restart')}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-[var(--color-error)] hover:text-[var(--color-error)]"
                    onClick={() => void deleteServer(selected.id)}
                  >
                    <span className="material-symbols-outlined text-[14px]">delete</span>
                    {t('common.delete')}
                  </Button>
                </div>
              )}

              <div className="flex justify-end gap-2 pt-2 border-t border-[var(--color-border-separator)]">
                {isCreating && (
                  <Button
                    variant="ghost"
                    size="md"
                    onClick={() => {
                      setIsCreating(false)
                      selectServer(null)
                    }}
                  >
                    {t('common.cancel')}
                  </Button>
                )}
                <Button size="md" onClick={() => void handleSave()} loading={isSaving}>
                  {isCreating ? t('common.add') : t('common.save')}
                </Button>
              </div>

              {selected && recentDiagnostics.length > 0 && (
                <div className="border-t border-[var(--color-border-separator)] pt-3 mt-2">
                  <div className="text-[11px] uppercase tracking-wider text-[var(--color-text-tertiary)] mb-2">
                    {t('settings.lsp.recentDiagnostics')}
                  </div>
                  <div className="flex flex-col gap-1.5">
                    {recentDiagnostics.map((d, idx) => (
                      <div
                        key={`${d.uri}-${idx}`}
                        className="text-xs px-2 py-1 rounded bg-[var(--color-surface-container)] border border-[var(--color-border)]"
                      >
                        <div className="text-[var(--color-text-primary)] truncate">
                          {d.message}
                        </div>
                        <div className="text-[10px] text-[var(--color-text-tertiary)] font-mono truncate">
                          {d.uri}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </>
          ) : (
            <div className="flex-1 flex items-center justify-center text-xs text-[var(--color-text-tertiary)]">
              {t('settings.lsp.empty')}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

function ModeToggle({ mode, onChange }: { mode: Mode; onChange: (m: Mode) => void }) {
  const t = useTranslation()
  return (
    <div className="flex bg-[var(--color-surface-container)] rounded-lg p-0.5 w-fit">
      {(['managed', 'manual'] as Mode[]).map((m) => (
        <button
          key={m}
          onClick={() => onChange(m)}
          className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors ${
            mode === m
              ? 'bg-[var(--color-surface)] text-[var(--color-text-primary)] shadow-sm'
              : 'text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]'
          }`}
        >
          {t(m === 'managed' ? 'settings.lsp.mode.managed' : 'settings.lsp.mode.manual')}
        </button>
      ))}
    </div>
  )
}

function ManagedActions({
  server,
  progress,
  onInstall,
}: {
  server: LspServerRecord
  progress?: { phase: string; percent?: number | null; message?: string }
  onInstall: () => void
}) {
  const t = useTranslation()
  const phase = progress?.phase
  const inProgress =
    phase === 'resolving' ||
    phase === 'downloading' ||
    phase === 'extracting' ||
    phase === 'verifying'

  const installed = server.installState.status === 'installed'
  const failed = server.installState.status === 'failed' || phase === 'failed'

  const description = (() => {
    if (server.installState.status === 'installed') {
      return `${t('settings.lsp.installState.installed')} · ${server.installState.version}`
    }
    if (server.installState.status === 'installing' || inProgress) {
      const pct = typeof progress?.percent === 'number' ? `${progress.percent}%` : '…'
      return `${t('settings.lsp.installState.installing')} · ${pct}`
    }
    if (server.installState.status === 'failed') {
      return `${t('settings.lsp.installState.failed')} · ${server.installState.reason}`
    }
    return t('settings.lsp.installState.notInstalled')
  })()

  return (
    <div className="flex flex-col gap-2 px-3 py-2 rounded-lg bg-[var(--color-surface-container)] border border-[var(--color-border)]">
      <div className="flex items-center gap-2 text-xs text-[var(--color-text-secondary)]">
        <span className="material-symbols-outlined text-[16px]">cloud_download</span>
        <span className="flex-1 truncate">{description}</span>
      </div>
      {(progress?.percent ?? null) !== null && (
        <div className="h-1.5 bg-[var(--color-surface)] rounded-full overflow-hidden">
          <div
            className="h-full bg-[var(--color-brand)] transition-all"
            style={{ width: `${progress?.percent ?? 0}%` }}
          />
        </div>
      )}
      <div className="flex items-center gap-2">
        <Button variant="primary" size="sm" onClick={onInstall} disabled={inProgress}>
          <span className="material-symbols-outlined text-[14px]">download</span>
          {installed
            ? t('settings.lsp.action.reinstall')
            : failed
            ? t('settings.lsp.action.retry')
            : t('settings.lsp.action.install')}
        </Button>
      </div>
    </div>
  )
}
