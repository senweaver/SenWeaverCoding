// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
//
// Bootstraps Monaco for the desktop app:
//
//  1. Pins `@monaco-editor/react` to the locally-bundled `monaco`
//     package so we never reach for the AMD CDN at runtime — Tauri
//     ships offline and the renderer has no internet guarantee.
//  2. Wires up Monaco's worker pool via Vite's native `?worker`
//     imports so each language's heavy parsing runs off the main
//     thread and is included in the production bundle without a
//     dedicated plugin.
//
// Side-effect import only: callers should `import './monacoSetup'`
// (no symbols) before mounting any `<Editor />` instance.  Import
// dedup ensures `setMonacoEnv` runs at most once per renderer.

import { loader } from '@monaco-editor/react'
import * as monaco from 'monaco-editor'

import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker'
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker'
import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker'
import htmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker'
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker'

let initialised = false

export function setupMonacoEnvironment() {
  if (initialised) return
  initialised = true

  ;(self as unknown as { MonacoEnvironment: monaco.Environment }).MonacoEnvironment = {
    getWorker(_workerId: string, label: string) {
      if (label === 'json') return new jsonWorker()
      if (label === 'css' || label === 'scss' || label === 'less') return new cssWorker()
      if (label === 'html' || label === 'handlebars' || label === 'razor') return new htmlWorker()
      if (label === 'typescript' || label === 'javascript') return new tsWorker()
      return new editorWorker()
    },
  }

  loader.config({ monaco })
}

setupMonacoEnvironment()

export { monaco }
