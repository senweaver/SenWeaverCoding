// SPDX-License-Identifier: MIT
//
// xterm.js host component shared by the bottom Terminal Panel.
//
// Two render modes:
//
//   - mode='pty'           : owns a real PTY session via terminalApi.
//                            stdin -> backend (interactive shell).
//
//   - mode='agent-mirror'  : read-only viewer fed by the bottom-panel
//                            store ring buffer. Replays the buffer on
//                            mount and subscribes via
//                            registerMirrorWriter so subsequent
//                            BackgroundShellSignal events stream live.
//
// All PTY lifecycle (spawn / resize / exit / dispose) lives here so
// the rest of the panel UI can stay declarative.

import { useCallback, useEffect, useImperativeHandle, useRef, type Ref } from 'react'
import type { Terminal as XTermTerminal } from '@xterm/xterm'
import type { FitAddon as XTermFitAddon } from '@xterm/addon-fit'

import { terminalApi } from '../../api/terminal'
import {
  readMirrorBuffer,
  registerMirrorWriter,
} from '../../stores/terminalPanelStore'

export type XtermViewHandle = {
  focus: () => void
  fit: () => void
  clear: () => void
  appendChunk: (text: string) => void
}

export type XtermViewProps = {
  tabId: string
  mode: 'pty' | 'agent-mirror'
  active: boolean
  initialCwd?: string
  onSpawned?: (info: { sessionId: number; shell: string; cwd: string }) => void
  onExited?: (info: { code: number; signal?: string | null }) => void
  onError?: (message: string) => void
  forwardRef?: Ref<XtermViewHandle>
}

const TERMINAL_THEME = {
  background: '#121212',
  foreground: '#d7d2d0',
  cursor: '#ffb59f',
  selectionBackground: '#5f4a40',
  black: '#1f1f1f',
  red: '#ff6d67',
  green: '#7ef18a',
  yellow: '#f8c55f',
  blue: '#77a8ff',
  magenta: '#d699ff',
  cyan: '#61d6d6',
  white: '#d7d2d0',
  brightBlack: '#8f8683',
  brightRed: '#ff8a85',
  brightGreen: '#9ff7a7',
  brightYellow: '#ffdd7a',
  brightBlue: '#a6c5ff',
  brightMagenta: '#e3b8ff',
  brightCyan: '#8ceeee',
  brightWhite: '#ffffff',
}

export function XtermView(props: XtermViewProps) {
  const {
    tabId,
    mode,
    active,
    initialCwd,
    onSpawned,
    onExited,
    onError,
    forwardRef,
  } = props

  const hostRef = useRef<HTMLDivElement | null>(null)
  const terminalRef = useRef<XTermTerminal | null>(null)
  const fitRef = useRef<XTermFitAddon | null>(null)
  const sessionIdRef = useRef<number | null>(null)
  const unlistenRef = useRef<Array<() => void>>([])
  const disposedRef = useRef(false)

  // Stash the latest callbacks in refs so the boot effect below does not
  // need to depend on them. TerminalPanel passes inline arrow functions
  // for onSpawned / onExited / onError; if those went into the effect
  // deps the whole xterm session would be torn down and re-spawned on
  // every render of the parent, dropping the shell's initial banner.
  const onSpawnedRef = useRef(onSpawned)
  const onExitedRef = useRef(onExited)
  const onErrorRef = useRef(onError)
  useEffect(() => {
    onSpawnedRef.current = onSpawned
    onExitedRef.current = onExited
    onErrorRef.current = onError
  })

  const fitRafRef = useRef<number | null>(null)
  const fit = useCallback(() => {
    if (fitRafRef.current !== null) return
    const schedule = (cb: () => void): number => {
      if (typeof window !== 'undefined' && typeof window.requestAnimationFrame === 'function') {
        return window.requestAnimationFrame(cb)
      }
      return setTimeout(cb, 16) as unknown as number
    }
    fitRafRef.current = schedule(() => {
      fitRafRef.current = null
      if (typeof document !== 'undefined' && document.hidden) return
      const host = hostRef.current
      if (host) {
        const rect = host.getBoundingClientRect()
        if (rect.width <= 0 || rect.height <= 0) return
      }
      const terminal = terminalRef.current
      const fitAddon = fitRef.current
      if (!terminal || !fitAddon) return
      try {
        fitAddon.fit()
      } catch {
        /* host element may be 0x0 transiently; ignore */
      }
      if (mode === 'pty') {
        const sid = sessionIdRef.current
        if (sid != null) {
          void terminalApi.resize(sid, terminal.cols, terminal.rows).catch(() => {})
        }
      }
    })
  }, [mode])

  useEffect(() => {
    return () => {
      if (fitRafRef.current !== null) {
        if (typeof window !== 'undefined' && typeof window.cancelAnimationFrame === 'function') {
          window.cancelAnimationFrame(fitRafRef.current)
        }
        clearTimeout(fitRafRef.current as unknown as ReturnType<typeof setTimeout>)
        fitRafRef.current = null
      }
    }
  }, [])

  useImperativeHandle(
    forwardRef,
    () =>
      ({
        focus: () => terminalRef.current?.focus(),
        fit,
        clear: () => terminalRef.current?.clear(),
        appendChunk: (text: string) => terminalRef.current?.write(text),
      }) satisfies XtermViewHandle,
    [fit],
  )

  useEffect(() => {
    if (!active) return
    fit()
    terminalRef.current?.focus()
  }, [active, fit])

  useEffect(() => {
    let observer: ResizeObserver | null = null
    let cancelled = false
    disposedRef.current = false

    const boot = async () => {
      const host = hostRef.current
      if (!host) return

      const [{ Terminal }, { FitAddon }] = await Promise.all([
        import('@xterm/xterm'),
        import('@xterm/addon-fit'),
      ])

      if (cancelled) return

      const terminal = new Terminal({
        cursorBlink: mode === 'pty',
        disableStdin: mode === 'agent-mirror',
        convertEol: false,
        fontFamily: "var(--font-mono), 'SFMono-Regular', Consolas, monospace",
        fontSize: 12,
        lineHeight: 1.25,
        scrollback: 4000,
        theme: TERMINAL_THEME,
      })
      const fitAddon = new FitAddon()
      terminal.loadAddon(fitAddon)
      terminal.open(host)
      terminalRef.current = terminal
      fitRef.current = fitAddon
      try {
        fitAddon.fit()
      } catch {
        /* size 0 transiently */
      }

      observer = new ResizeObserver(() => fit())
      observer.observe(host)

      if (mode === 'pty') {
        if (!terminalApi.isAvailable()) {
          terminal.writeln(
            '\x1b[31mTerminal is only available inside the desktop runtime.\x1b[0m',
          )
          onErrorRef.current?.('terminal runtime unavailable')
          return
        }

        const PENDING_RING_MAX = 512
        const pendingPayloads: Array<{ session_id: number; data: string }> = []

        const outputUnlisten = await terminalApi.onOutput((payload) => {
          const sid = sessionIdRef.current
          if (sid != null) {
            if (payload.session_id === sid) terminal.write(payload.data)
            return
          }
          pendingPayloads.push(payload)
          if (pendingPayloads.length > PENDING_RING_MAX) pendingPayloads.shift()
        })
        const exitUnlisten = await terminalApi.onExit((payload) => {
          if (payload.session_id !== sessionIdRef.current) return
          const signal = payload.signal ? `, ${payload.signal}` : ''
          terminal.writeln(`\r\n[process exited: ${payload.code}${signal}]`)
          sessionIdRef.current = null
          onExitedRef.current?.({ code: payload.code, signal: payload.signal ?? null })
        })
        unlistenRef.current = [outputUnlisten, exitUnlisten]

        terminal.onData((data) => {
          const sid = sessionIdRef.current
          if (sid != null) {
            void terminalApi.write(sid, data).catch((err) => {
              const msg = err instanceof Error ? err.message : String(err)
              onErrorRef.current?.(msg)
            })
          }
        })

        try {
          const result = await terminalApi.spawn({
            cols: terminal.cols,
            rows: terminal.rows,
            cwd: initialCwd,
          })
          if (cancelled) {
            await terminalApi.kill(result.session_id).catch(() => {})
            return
          }
          sessionIdRef.current = result.session_id

          while (pendingPayloads.length > 0) {
            const next = pendingPayloads.shift()
            if (next && next.session_id === result.session_id) {
              terminal.write(next.data)
            }
          }

          onSpawnedRef.current?.({
            sessionId: result.session_id,
            shell: result.shell,
            cwd: result.cwd,
          })
          fit()
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err)
          terminal.writeln(`\x1b[31m[spawn failed: ${msg}]\x1b[0m`)
          onErrorRef.current?.(msg)
          unlistenRef.current.forEach((u) => u())
          unlistenRef.current = []
        }
      } else {
        for (const chunk of readMirrorBuffer()) terminal.write(chunk)
        const unsubscribe = registerMirrorWriter((chunk) => {
          terminal.write(chunk)
        })
        unlistenRef.current = [unsubscribe]
      }
    }

    void boot()

    return () => {
      cancelled = true
      disposedRef.current = true
      observer?.disconnect()
      observer = null
      const sid = sessionIdRef.current
      if (mode === 'pty' && sid != null) {
        void terminalApi.kill(sid).catch(() => {})
      }
      sessionIdRef.current = null
      unlistenRef.current.forEach((u) => {
        try {
          u()
        } catch {
          /* ignore */
        }
      })
      unlistenRef.current = []
      terminalRef.current?.dispose()
      terminalRef.current = null
      fitRef.current = null
    }

  // Intentionally NOT depending on `fit`, `onSpawned`, `onExited`, `onError`:
  // they are either stable callbacks (fit) or accessed through refs above.
  // Including unstable inline callbacks here would tear down the xterm
  // session on every parent render and lose the shell's initial banner.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tabId, mode, initialCwd])

  return (
    <div
      ref={hostRef}
      data-testid={`xterm-host-${tabId}`}
      className="h-full w-full overflow-hidden bg-[#121212] p-2"
    />
  )
}
