import { defineConfig } from "vite";

/**
 * The dev server proxies the API to `anth-panel run`, so the browser talks to
 * one origin and no CORS rule is needed. The default matches the daemon's
 * `--addr`; `ANTH_PANEL_ADDR` overrides it when that port is taken.
 */
const backend = process.env.ANTH_PANEL_ADDR ?? "http://127.0.0.1:8080";

export default defineConfig({
  server: { port: 5173, proxy: { "/api": backend, "/healthz": backend } },
  // `preview` serves the built output, so it needs the same proxy to be a
  // faithful check of what a deployment will do.
  preview: { port: 4173, proxy: { "/api": backend, "/healthz": backend } },
  build: {
    outDir: "dist",
    // The v0.1.0 tree embedded its build output through `rust-embed`; v0.2.0
    // has no embedder yet, so the output stays a plain folder.
    emptyOutDir: true,
  },
});
