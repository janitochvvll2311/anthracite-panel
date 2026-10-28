import { defineConfig, type Plugin } from 'vite'

/**
 * Drops Vite's HMR client from the served HTML.
 *
 * Mobile browsers close the HMR websocket while the native file picker is
 * open; on return the client pings the server and reloads the page, losing the
 * chosen file. `server.hmr = false` stops module updates but Vite still
 * injects the client, so strip the tag too. Reached through `ANTH_HMR=off`,
 * which `scripts/dev.sh` sets by default.
 */
function withoutHmrClient(): Plugin {
  return {
    name: 'anth-without-hmr-client',
    apply: 'serve',
    enforce: 'post',
    transformIndexHtml(html) {
      return html.replace(
        /<script type="module" src="[^"]*\/@vite\/client"><\/script>\s*/,
        '',
      )
    },
  }
}

export default defineConfig({
  build: {
    rollupOptions: {
      input: 'index.html',
    },
  },
  plugins: process.env.ANTH_HMR === 'off' ? [withoutHmrClient()] : [],
  server: {
    hmr: process.env.ANTH_HMR === 'off' ? false : undefined,
    proxy: {
      '/healthz': 'http://127.0.0.1:8080',
      // Sandboxed plugin UI documents and their bridge live beside the API.
      '/p': 'http://127.0.0.1:8080',
      '/api': {
        target: 'http://127.0.0.1:8080',
        // The terminal is a WebSocket at /api/terminal; without this the
        // proxy completes the HTTP handshake and leaves the upgrade hanging.
        ws: true,
      },
    },
  },
})
