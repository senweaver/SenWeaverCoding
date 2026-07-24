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
 
  optimizeDeps: {
    include: [
      'monaco-editor',
      '@monaco-editor/react',
      '@tauri-apps/api/core',
      '@tauri-apps/api/event',
      '@tauri-apps/api/window',
      '@tauri-apps/api/webviewWindow',
    ],
  },
  worker: {
    format: 'es',
  },
  build: {
    rollupOptions: {
      output: {
        // Split heavy, independently-loadable libraries out of the main chunk
        // so first paint doesn't pay for the full syntax-highlight + math
        // stacks. shiki (grammars + regex engine) and katex are large and only
        // needed once code/math actually renders.
        manualChunks(id) {
          if (id.includes('node_modules')) {
            if (id.includes('shiki') || id.includes('react-shiki')) return 'vendor-shiki'
            if (id.includes('katex')) return 'vendor-katex'
            if (id.includes('mermaid')) return 'vendor-mermaid'
            if (id.includes('echarts')) return 'vendor-echarts'
            // Anchor to the jsdiff PACKAGE directory: a bare '/diff/' substring
            // also matched monaco-editor's internal esm/vs/**/diff/** modules,
            // ripping a slice of monaco into an eagerly-loaded chunk and
            // creating a circular vendor-diff <-> monaco dependency.
            if (
              id.includes('react-diff-viewer') ||
              id.includes('node_modules/diff/') ||
              id.includes('node_modules\\diff\\') ||
              id.includes('/.pnpm/diff@')
            ) {
              return 'vendor-diff'
            }
          }
          return undefined
        },
      },
    },
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
