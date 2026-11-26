// A Bun plugin for the developer workflow. It resolves the virtual
// `anth:client` module the generated app code imports; a preload can register
// it once with `Bun.plugin(anthPlugin())`.

export interface AnthBuild {
  onResolve(
    options: { filter: RegExp },
    callback: (args: { path: string }) => { path: string; namespace: string } | undefined,
  ): void;
  onLoad(
    options: { filter: RegExp; namespace: string },
    callback: (args: { path: string }) => { contents: string; loader: string } | undefined,
  ): void;
}

export function anthPlugin(): { name: string; setup(build: AnthBuild): void } {
  return {
    name: "anthracite",
    setup(build) {
      build.onResolve({ filter: /^anth:client$/ }, () => ({
        path: "anth:client",
        namespace: "anthracite",
      }));
      build.onLoad({ filter: /.*/, namespace: "anthracite" }, () => ({
        contents: 'export const virtual = true; export const generated = "anth:client";',
        loader: "js",
      }));
    },
  };
}

export default anthPlugin;

// `bunfig.toml` preload = ["@anth-panel/bun-plugin"] registers the plugin.
const runtime = (globalThis as { Bun?: { plugin?: (plugin: unknown) => void } }).Bun;
if (runtime?.plugin) {
  runtime.plugin(anthPlugin());
}
