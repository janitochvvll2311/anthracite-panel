import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";

/**
 * The dev server proxies the API to `anth-panel run`, so the browser talks to
 * one origin and no CORS rule is needed. The default matches the daemon's
 * `--addr`; `ANTH_PANEL_ADDR` overrides it when that port is taken.
 */
const backend = process.env.ANTH_PANEL_ADDR ?? "http://127.0.0.1:8080";

export default defineConfig(({ command }) => {
  // The harness is a development page: `public/` exists only so it can import a
  // plugin bundle from this origin. Vite copies `public/` into every build, so
  // leaving it on would ship plugin bundles inside the shell, where no page
  // references them. `ANTH_HARNESS=1` builds the harness on purpose.
  const harness = command === "serve" || Boolean(process.env.ANTH_HARNESS);

  return {
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
    publicDir: harness ? "public" : false,
    // `index.html` is the only production entry. The harness is reachable in
    // development, where Vite serves it from the project root, and a build that
    // included it would ship a test page and the bundle behind it.
    build: {
      outDir: "dist",
      rollupOptions: {
        input: {
          main: fileURLToPath(new URL("./index.html", import.meta.url)),
          ...(harness
            ? { harness: fileURLToPath(new URL("./harness.html", import.meta.url)) }
            : {}),
        },
      },
      emptyOutDir: true,
    },
  };
});
