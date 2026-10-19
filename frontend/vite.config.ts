import { defineConfig } from 'vite'

// In development the Rust API is not embedded, so Vite serves the UI and proxies
// the API routes to a locally running `anth-panel api`/`run`.
export default defineConfig({
  build: {
    rollupOptions: {
      // Only the app shell is a build entry. Dev harnesses such as
      // `src/components/anth-terminal.test.html`,
      // `src/components/anth-process-monitor.test.html` and
      // `src/components/anth-file-explorer.test.html` stay out of dist/.
      input: 'index.html',
    },
  },
  server: {
    proxy: {
      '/healthz': 'http://127.0.0.1:8080',
      '/api': {
        target: 'http://127.0.0.1:8080',
        ws: true,
      },
    },
  },
})
