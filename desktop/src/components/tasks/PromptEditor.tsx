// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { PermissionModeSelector } from '../controls/PermissionModeSelector'
import { CodingModeSelector } from '../controls/CodingModeSelector'
import { ModelSelector } from '../controls/ModelSelector'
import { DirectoryPicker } from '../shared/DirectoryPicker'
import { useTranslation } from '../../i18n'
import type { PermissionMode } from '../../types/settings'
import type { CodingModeId } from '../../types/codingMode'

type Props = {
  value: string
  onChange: (value: string) => void
  placeholder?: string

  codingMode: CodingModeId
  onCodingModeChange: (mode: CodingModeId) => void

  permissionMode: PermissionMode
  onPermissionModeChange: (mode: PermissionMode) => void

  modelId: string
  onModelChange: (modelId: string) => void

  folderPath: string
  onFolderPathChange: (path: string) => void

  useWorktree: boolean
  onUseWorktreeChange: (checked: boolean) => void
}

export function PromptEditor({
  value,
  onChange,
  placeholder,
  codingMode,
  onCodingModeChange,
  permissionMode,
  onPermissionModeChange,
  modelId,
  onModelChange,
  folderPath,
  onFolderPathChange,
  useWorktree: _useWorktree,
  onUseWorktreeChange: _onUseWorktreeChange,
}: Props) {
  const t = useTranslation()
  return (
    <div className="overflow-visible rounded-xl border border-[var(--color-border)] transition-colors focus-within:border-[var(--color-border-focus)]">
      <textarea
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        rows={4}
        className="w-full resize-y bg-transparent px-3 py-2.5 text-xs leading-relaxed text-[var(--color-text-primary)] outline-none placeholder:text-[var(--color-text-tertiary)]"
        style={{ minHeight: 120 }}
      />

      <div className="flex flex-col gap-2 rounded-b-xl border-t border-[var(--color-border)]/40 bg-[var(--color-surface-container-low)] px-3 py-2">
        <div className="flex flex-wrap items-center gap-2 justify-between">
          <div className="flex flex-wrap items-center gap-2 min-w-0">
            <CodingModeSelector value={codingMode} onChange={onCodingModeChange} />
            <PermissionModeSelector value={permissionMode} onChange={onPermissionModeChange} workDir={folderPath || undefined} />
          </div>
          <ModelSelector value={modelId} onChange={onModelChange} />
        </div>

        <div className="flex items-center justify-between">
          <DirectoryPicker value={folderPath} onChange={onFolderPathChange} />
        </div>

        {permissionMode === 'bypassPermissions' && (
          <div className="flex items-center gap-1.5 rounded-lg bg-[var(--color-error)]/8 px-2 py-1.5 text-xs text-[var(--color-error)]">
            <span className="material-symbols-outlined text-[12px]">warning</span>
            {t('promptEditor.bypassWarning')}{folderPath ? ` ${t('promptEditor.within')} ${folderPath}` : ` ${t('promptEditor.selectFolder')}`}.
          </div>
        )}
      </div>
    </div>
  )
}
