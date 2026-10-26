import { defineConfig } from 'vite'

export default defineConfig({
  build: {
    rollupOptions: {
      input: 'index.html',
    },
  },
  server: {
    proxy: {
      '/healthz': 'http://127.0.0.1:8080',
      '/api': {
        target: 'http://127.0.0.1:8080',
      },
    },
  },
})
