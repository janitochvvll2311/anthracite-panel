import { defineConfig } from 'vite'

// In development the Rust API is not embedded, so Vite serves the UI and proxies
// the API routes to a locally running `anth-panel api`/`run`.
export default defineConfig({
  server: {
    proxy: {
      '/healthz': 'http://127.0.0.1:8080',
      '/command': 'http://127.0.0.1:8080',
    },
  },
})
