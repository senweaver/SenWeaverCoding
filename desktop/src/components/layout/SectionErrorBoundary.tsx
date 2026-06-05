// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { Component, type ErrorInfo, type ReactNode } from 'react'

type Props = {
  children: ReactNode
  label?: string
  resetKeys?: unknown[]
}

type State = {
  error: Error | null
}

export class SectionErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error }
  }

  componentDidUpdate(prevProps: Props): void {
    if (!this.state.error) return
    const prev = prevProps.resetKeys
    const next = this.props.resetKeys
    if (!prev || !next) return
    if (prev.length !== next.length || prev.some((v, i) => v !== next[i])) {
      this.setState({ error: null })
    }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error(
      `[SectionErrorBoundary${this.props.label ? `:${this.props.label}` : ''}]`,
      error,
      info.componentStack,
    )
  }

  private handleRetry = () => {
    this.setState({ error: null })
  }

  render(): ReactNode {
    const { error } = this.state
    if (!error) return this.props.children
    return (
      <div className="flex h-full min-h-[120px] w-full flex-col items-center justify-center gap-2 p-6 text-center">
        <span className="material-symbols-outlined text-[28px] text-[var(--color-error)]">
          report
        </span>
        <div className="text-sm font-medium text-[var(--color-text-primary)]">
          {this.props.label
            ? `“${this.props.label}” 渲染出错 / This section failed to render`
            : '此区域渲染出错 / This section failed to render'}
        </div>
        <div className="max-w-[520px] break-words text-xs text-[var(--color-text-secondary)]">
          {error.name}: {error.message}
        </div>
        <button
          type="button"
          onClick={this.handleRetry}
          className="mt-1 rounded-[var(--radius-md)] border border-[var(--color-border)] px-3 py-1 text-xs text-[var(--color-text-primary)] hover:bg-[var(--color-surface-hover)]"
        >
          重试 / Retry
        </button>
      </div>
    )
  }
}
