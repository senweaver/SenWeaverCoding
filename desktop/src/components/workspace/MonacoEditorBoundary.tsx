// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { Component, type ErrorInfo, type ReactNode } from 'react'
import { t } from '../../i18n'

type Props = {
  children: ReactNode
}

type State = {
  error: Error | null
}

export class MonacoEditorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error('[MonacoEditorBoundary]', error, info.componentStack)
  }

  private handleReload = () => {
    window.location.reload()
  }

  render(): ReactNode {
    const { error } = this.state
    if (!error) return this.props.children
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-2 px-4 text-center">
        <span className="material-symbols-outlined text-[20px] text-[var(--color-text-tertiary)]">
          error
        </span>
        <div className="text-xs font-medium text-[var(--color-text-secondary)]">
          {t('files.editorCrashed')}
        </div>
        <pre className="max-h-32 w-full max-w-md overflow-auto whitespace-pre-wrap break-all rounded bg-[var(--color-surface-hover)] p-2 text-left text-[10px] text-[var(--color-text-tertiary)]">
          {error.message}
        </pre>
        <button
          type="button"
          onClick={this.handleReload}
          className="rounded bg-[var(--color-accent)] px-3 py-1 text-xs font-medium text-[var(--color-on-accent)] hover:opacity-90"
        >
          {t('files.editorReload')}
        </button>
      </div>
    )
  }
}
