import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";

/**
 * The dev server proxies the API to `anth-panel run`, so the browser talks to
 * one origin and no CORS rule is needed. The default matches the daemon's
 * `--addr`; `ANTH_PANEL_ADDR` overrides it when that port is taken.
 */
const backend = process.env.ANTH_PANEL_ADDR ?? "http://127.0.0.1:8080";

export default defineConfig({
  // `/p` is where a plugin's declared anchors live, so the panel reaches them
  // through the same origin as everything else.
  server: {
    port: 5173,
    proxy: { "/api": backend, "/healthz": backend, "/p": backend },
  },
  // `preview` serves the built output, so it needs the same proxy to be a
  // faithful check of what a deployment will do.
  preview: {
    port: 4173,
    proxy: { "/api": backend, "/healthz": backend, "/p": backend },
  },
  // A harness page has to be an explicit input: Vite's dev server rewrites any
  // unknown path to `index.html`, so a page dropped into `public/` is never
  // reached. `index.html` stays the only production entry.
  build: {
    outDir: "dist",
    rollupOptions: {
      input: {
        main: fileURLToPath(new URL("./index.html", import.meta.url)),
        harness: fileURLToPath(new URL("./harness.html", import.meta.url)),
      },
    },
    // The v0.1.0 tree embedded its build output through `rust-embed`; v0.2.0
    // has no embedder yet, so the output stays a plain folder.
    emptyOutDir: true,
  },
});
