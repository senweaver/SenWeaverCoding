// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.

import { Component, type ErrorInfo, type ReactNode } from 'react'

type Props = {
  children: ReactNode
}

type State = {
  error: Error | null
  componentStack: string | null
}

export class AppErrorBoundary extends Component<Props, State> {
  state: State = { error: null, componentStack: null }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error }
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    this.setState({ componentStack: info.componentStack ?? null })
    console.error('[AppErrorBoundary]', error, info.componentStack)
  }

  private handleReload = () => {
    window.location.reload()
  }

  render(): ReactNode {
    const { error, componentStack } = this.state
    if (!error) return this.props.children
    return (
      <div
        style={{
          height: '100vh',
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          justifyContent: 'center',
          gap: 12,
          padding: 24,
          fontFamily:
            '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif',
          color: '#1a1a1a',
          background: '#fcfcfc',
        }}
      >
        <div style={{ fontSize: 16, fontWeight: 600 }}>
          应用启动失败 / Application failed to start
        </div>
        <div style={{ fontSize: 13, color: '#666', maxWidth: 720, textAlign: 'center' }}>
          {error.name}: {error.message}
        </div>
        {error.stack && (
          <pre
            style={{
              fontSize: 11,
              maxHeight: 240,
              maxWidth: 800,
              width: '100%',
              overflow: 'auto',
              background: '#f4f4f4',
              border: '1px solid #ddd',
              borderRadius: 6,
              padding: 12,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-all',
            }}
          >
            {error.stack}
          </pre>
        )}
        {componentStack && (
          <pre
            style={{
              fontSize: 11,
              maxHeight: 200,
              maxWidth: 800,
              width: '100%',
              overflow: 'auto',
              background: '#f4f4f4',
              border: '1px solid #ddd',
              borderRadius: 6,
              padding: 12,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-all',
            }}
          >
            {componentStack}
          </pre>
        )}
        <button
          type="button"
          onClick={this.handleReload}
          style={{
            padding: '6px 14px',
            fontSize: 13,
            border: '1px solid #888',
            borderRadius: 6,
            background: '#fff',
            cursor: 'pointer',
          }}
        >
          重新加载 / Reload
        </button>
      </div>
    )
  }
}
