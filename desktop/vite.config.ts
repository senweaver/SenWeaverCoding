import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src'),
    },
  },
  // Pre-bundle monaco-editor's heavy ESM tree.  Without this, the
  // first dev-mode load of the right-sidebar editor stalls for ~6 s
  // while Vite re-optimises.  The include path must match the actual
  // import in `lib/monacoSetup.ts` (`import * as monaco from
  // 'monaco-editor'`); a deeper path (e.g. `editor.api`) leaves the
  // bare `monaco-editor` specifier unoptimised and Vite ends up
  // shipping two divergent copies that fight `loader.config({ monaco
  // })` at runtime.
  optimizeDeps: {
    include: ['monaco-editor', '@monaco-editor/react'],
  },
  worker: {
    format: 'es',
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
})
